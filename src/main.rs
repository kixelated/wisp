use std::net;
use std::rc::Rc;
use std::os::unix::io::{AsRawFd, FromRawFd, RawFd};
use std::collections::HashMap;

use nix::sys::socket;

use wisp::{Runtime, Task};

enum Socket {
    Backend { frontend: net::TcpStream },
    Frontend { backend: net::TcpStream },
}

fn main() -> anyhow::Result<()> {
    let mut io = Runtime::new(256)?;

    let backend_addr: net::SocketAddr = "127.0.0.1:9001".parse().unwrap();
    let frontend_addr: net::SocketAddr = "127.0.0.1:8080".parse().unwrap();

    let accept = net::TcpListener::bind(frontend_addr)?;
    println!("listen {}", accept.local_addr()?);

    io.run(Task::Accept { socket: accept })?;

    let mut sockets = HashMap::new();

    loop {
        let (_id, task, ret) = io.wait()?;

        match task {
            Task::Accept { .. } => {
                let frontend = ret? as RawFd;
                let frontend = unsafe { net::TcpStream::from_raw_fd(frontend) };

                // Queue up the next accept.
                io.run(Task::Accept { socket: accept })?;

                // Create a new IPv4 TCP socket.
                // We need to use the nix package because there's no way to do this in the stdlib.
                let backend = socket::socket(
                    socket::AddressFamily::Inet,
                    socket::SockType::Stream,
                    socket::SockFlag::empty(),
                    socket::SockProtocol::Tcp,
                )?;
                let backend = unsafe { net::TcpStream::from_raw_fd(backend) };

                sockets.insert(frontend.as_raw_fd(), Socket::Frontend { backend });
                sockets.insert(backend.as_raw_fd(), Socket::Backend { frontend });

                //let addr = socket::InetAddr::from_std(&backend_addr);
                //let addr = socket::SockAddr::Inet(addr);

                io.run(Task::Connect {
                    socket: backend,
                    addr: backend_addr,
                })?;
            },
            Task::Close { socket } => {
                if let Err(err) = ret {
                    println!("failed to close socket: {:?}", err);
                }

                let fd = socket.as_raw_fd();
                sockets.remove(&fd);
            },
            Task::Connect { socket, .. } => {
                let fd = socket.as_raw_fd();

                let (frontend, backend) = match sockets.get(&fd) {
                    Some(Socket::Backend { frontend }) => (*frontend, socket),
                    Some(Socket::Frontend { .. }) => anyhow::bail!("impossible frontend in lookup"),
                    None => continue, // closed
                };

                if let Err(err) = ret {
                    println!("failed to connect to backend: {:?}", err);

                    // Close the frontend socket on a backend connection failure.
                    io.run(Task::Close { socket: frontend })?;
                    io.run(Task::Close { socket: backend })?;

                    continue;
                }

                let backend_buffer = vec![0u8; 4096].into_boxed_slice();
                let frontend_buffer = vec![0u8; 4096].into_boxed_slice();

                io.run(Task::Read {
                    socket: frontend,
                    buffer: frontend_buffer,
                })?;

                io.run(Task::Read {
                    socket: backend,
                    buffer: backend_buffer,
                })?;
            },
            Task::Read { socket, buffer } => {
                let fd = socket.as_raw_fd();

                let (is_frontend, frontend, backend) = match sockets.get(&fd) {
                    Some(Socket::Backend { frontend }) => (false, *frontend, socket),
                    Some(Socket::Frontend { backend }) => (true, socket, *backend),
                    None => continue, // closed
                };

                let size = match ret {
                    Ok(size) => size,
                    Err(err) => {
                        if is_frontend {
                            println!("failed to read from frontend: {}", err);
                        } else {
                            println!("failed to read from backend: {}", err);
                        }

                        io.run(Task::Close { socket: frontend })?;
                        io.run(Task::Close { socket: backend })?;

                        continue;
                    }
                };

                if size == 0 {
                    if is_frontend {
                        let shutdown = socket::shutdown(backend.as_raw_fd(), socket::Shutdown::Write);
                        if let Err(err) = shutdown {
                            println!("failed to send shutdown to backend: {}", err);

                            io.run(Task::Close { socket: frontend })?;
                            io.run(Task::Close { socket: backend })?;
                        }

                        continue;
                    } else {
                        let shutdown = socket::shutdown(frontend.as_raw_fd(), socket::Shutdown::Write);
                        if let Err(err) = shutdown {
                            println!("failed to send shutdown to frontend: {}", err);
                        }

                        // We're done when the backend is done transferring the response.
                        io.run(Task::Close { socket: frontend })?;
                        io.run(Task::Close { socket: backend })?;

                        continue;
                    }
                }

                if is_frontend {
                    io.run(Task::Write {
                        socket: backend,
                        buffer,
                        offset: 0,
                        size,
                    })?;
                } else {
                    io.run(Task::Write {
                        socket: frontend,
                        buffer,
                        offset: 0,
                        size,
                    })?;
                }
            }
            Task::Write {
                socket,
                buffer,
                offset,
                size,
            } => {
                let fd = socket.as_raw_fd();

                let (is_frontend, frontend, backend) = match sockets.get(&fd) {
                    Some(Socket::Backend { frontend }) => (false, *frontend, socket),
                    Some(Socket::Frontend { backend }) => (true, socket, *backend),
                    None => continue, // closed
                };

                let written = match ret {
                    Ok(size) => size,
                    Err(err) => {
                        if is_frontend {
                            println!("failed to write to frontend: {}", err);
                        } else {
                            println!("failed to write to backend: {}", err);
                        }

                        io.run(Task::Close { socket: frontend })?;
                        io.run(Task::Close { socket: backend })?;

                        continue;
                    }
                };

                if written == size {
                    if is_frontend {
                        io.run(Task::Read {
                            socket: backend,
                            buffer,
                        })?;
                    } else {
                        io.run(Task::Read {
                            socket: frontend,
                            buffer,
                        })?;
                    }
                } else {
                    io.run(Task::Write {
                        socket,
                        buffer,
                        offset: offset + size,
                        size: size - written,
                    })?;
                }
            }
        }
    }
}
