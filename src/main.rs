use std::net;
use std::os::unix::io::{AsRawFd,RawFd};
use std::collections::HashMap;

use nix::sys::socket;

use wisp::{Runtime, Task};

enum Socket {
    Backend{frontend: RawFd},
    Frontend{backend: RawFd},
}

fn main() -> anyhow::Result<()> {
    let mut io = Runtime::new(256)?;

    let backend_addr = net::SocketAddr::new(net::IpAddr::V4(net::Ipv4Addr::LOCALHOST), 9001);

    let accept = net::TcpListener::bind(("127.0.0.1", 8080))?;
    let accept_fd = accept.as_raw_fd();
    println!("listen {}", accept.local_addr()?);

    io.run(Task::Accept{fd: accept_fd})?;

    let mut sockets = HashMap::new();

    loop {
        let (_id, task, ret) = io.wait()?;

        match task {
            Task::Accept{..} => {
                let frontend = ret? as RawFd;

                // Queue up the next accept.
                io.run(Task::Accept{fd: accept_fd})?;

                // Create a new IPv4 TCP socket.
                // We need to use the nix package because there's no way to do this in the stdlib.
                let backend = socket::socket(socket::AddressFamily::Inet, socket::SockType::Stream, socket::SockFlag::empty(), None)?;

                sockets.insert(frontend, Socket::Frontend{ backend: backend });
                sockets.insert(backend, Socket::Backend{ frontend: frontend });


                let addr = socket::InetAddr::from_std(&backend_addr);
                let addr = socket::SockAddr::Inet(addr);

                /* TODO fix
                io.run(Task::Connect{
                    fd: backend,
                    addr: socket::SockAddr::Inet(addr),
                })?;
                */

                socket::connect(backend, &addr)?;

                let backend_buffer = vec![0u8; 4096].into_boxed_slice();
                let frontend_buffer = vec![0u8; 4096].into_boxed_slice();

                io.run(Task::Read{
                    fd: frontend,
                    buffer: frontend_buffer,
                })?;

                io.run(Task::Read{
                    fd: backend,
                    buffer: backend_buffer,
                })?;
            },
            Task::Close{ fd } => {
                if let Err(err) = ret {
                    println!("failed to close socket: {:?}", err);
                }

                sockets.remove(&fd);
            },
            Task::Connect{ fd, .. } => {
                let (frontend, backend) = match sockets.get(&fd) {
                    Some(Socket::Backend{frontend}) => (*frontend, fd),
                    Some(Socket::Frontend{..}) => anyhow::bail!("impossible frontend in lookup"),
                    None => continue, // closed
                };

                if let Err(err) = ret {
                    println!("failed to connect to backend: {:?}", err);

                    // Close the frontend socket on a backend connection failure.
                    io.run(Task::Close{fd: frontend})?;
                    io.run(Task::Close{fd: backend})?;

                    continue
                }

                let backend_buffer = vec![0u8; 4096].into_boxed_slice();
                let frontend_buffer = vec![0u8; 4096].into_boxed_slice();

                io.run(Task::Read{
                    fd: frontend,
                    buffer: frontend_buffer,
                })?;

                io.run(Task::Read{
                    fd: backend,
                    buffer: backend_buffer,
                })?;
            },
            Task::Read{ fd, buffer } => {
                let (frontend, backend) = match sockets.get(&fd) {
                    Some(Socket::Backend{frontend}) => (*frontend, fd),
                    Some(Socket::Frontend{backend}) => (fd, *backend),
                    None => continue, // closed
                };

                let size = match ret {
                    Ok(size) => size,
                    Err(err) => {
                        if frontend == fd {
                            println!("failed to read from frontend: {}", err);
                        } else {
                            println!("failed to read from backend: {}", err);
                        }

                        io.run(Task::Close{fd: frontend})?;
                        io.run(Task::Close{fd: backend})?;

                        continue
                    },
                };

                if size == 0 {
                    if fd == frontend {
                        if let Err(err) = socket::shutdown(backend, socket::Shutdown::Write) {
                            println!("failed to send shutdown to backend: {}", err);

                            io.run(Task::Close{fd: frontend})?;
                            io.run(Task::Close{fd: backend})?;
                        }

                        continue
                    } else {
                        // We're done when the backend is done transferring the response.
                        io.run(Task::Close{fd: frontend})?;
                        io.run(Task::Close{fd: backend})?;

                        continue
                    }
                }

                if fd == frontend {
                    io.run(Task::Write{
                        fd: backend,
                        buffer: buffer,
                        offset: 0,
                        size: size,
                    })?;
                } else {
                    io.run(Task::Write{
                        fd: frontend,
                        buffer: buffer,
                        offset: 0,
                        size: size,
                    })?;
                }
            },
            Task::Write{ fd, buffer, offset, size } => {
                let (frontend, backend) = match sockets.get(&fd) {
                    Some(Socket::Backend{frontend}) => (*frontend, fd),
                    Some(Socket::Frontend{backend}) => (fd, *backend),
                    None => continue, // closed
                };

                let written = match ret {
                    Ok(size) => size,
                    Err(err) => {
                        if frontend == fd {
                            println!("failed to write to frontend: {}", err);
                        } else {
                            println!("failed to write to backend: {}", err);
                        }

                        io.run(Task::Close{fd: frontend})?;
                        io.run(Task::Close{fd: backend})?;

                        continue
                    },
                };

                if written == size {
                    if fd == frontend {
                        io.run(Task::Read{
                            fd: backend,
                            buffer: buffer,
                        })?;
                    } else {
                        io.run(Task::Read{
                            fd: frontend,
                            buffer: buffer,
                        })?;
                    }
                } else {
                    io.run(Task::Write{
                        fd: fd,
                        buffer: buffer,
                        offset: offset+size,
                        size: size-written,
                    })?;
                }
            },
        }
    }
}
