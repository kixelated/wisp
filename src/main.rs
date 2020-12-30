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
                let frontend_fd = ret? as RawFd;

                println!("accepted new connection: {}", frontend_fd);

                // Queue up the next accept.
                io.run(Task::Accept{fd: accept_fd})?;

                // Create a new IPv4 TCP socket.
                // We need to use the nix package because there's no way to do this in the stdlib.
                let backend_fd = socket::socket(socket::AddressFamily::Inet, socket::SockType::Stream, socket::SockFlag::empty(), None)?;

                sockets.insert(frontend_fd, Socket::Frontend{ backend: backend_fd });
                sockets.insert(backend_fd, Socket::Backend{ frontend: frontend_fd });


                println!("connecting to backend with: {} {}", backend_fd, backend_addr);

                let addr = socket::InetAddr::from_std(&backend_addr);

                io.run(Task::Connect{
                    fd: backend_fd,
                    addr: socket::SockAddr::Inet(addr),
                })?;
            },
            Task::Close{ fd } => {
                if let Err(err) = ret {
                    println!("failed to close socket: {:?}", err);
                }

                println!("closed socket: {}", fd);

                sockets.remove(&fd);
            },
            Task::Connect{ fd, .. } => {
                let (frontend, backend) = match sockets.get(&fd) {
                    Some(Socket::Backend{frontend}) => (*frontend, fd),
                    Some(Socket::Frontend{..}) => anyhow::bail!("impossible frontend in lookup"),
                    None => anyhow::bail!("missing fd lookup"),
                };

                match ret {
                    Ok(val) => println!("{}", val),
                    Err(err) => {
                        println!("failed to connect to backend: {:?}", err);

                        // Close the frontend socket on a backend connection failure.
                        io.run(Task::Close{fd: frontend})?;
                        io.run(Task::Close{fd: backend})?;

                        continue
                    },
                }

                println!("connected to backend: {}", backend);
                println!("reading from backend: {}", frontend);
                println!("reading from frontend: {}", frontend);

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
                    None => anyhow::bail!("missing fd lookup"),
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
                    println!("read of size zero");

                    if fd == frontend {
                        if let Err(err) = socket::shutdown(backend, socket::Shutdown::Write) {
                            println!("failed to send shutdown to backend: {}", err);

                            io.run(Task::Close{fd: frontend})?;
                            io.run(Task::Close{fd: backend})?;
                        }

                        println!("issued shutdown to backend: {}", backend);

                        continue
                    } else {
                        // We're done when the backend is done transferring the response.
                        io.run(Task::Close{fd: frontend})?;
                        io.run(Task::Close{fd: backend})?;

                        println!("issued close: {} {}", frontend, backend);

                        continue
                    }
                }

                if fd == frontend {
                    println!("read {} bytes from frontend: {}", size, frontend);
                    println!("copying to backend: {}", backend);

                    io.run(Task::Write{
                        fd: backend,
                        buffer: buffer,
                        offset: 0,
                        size: size,
                    })?;
                } else {
                    println!("read {} bytes from backend: {}", size, backend);
                    println!("copying to frontend: {}", frontend);

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
                    None => anyhow::bail!("missing fd lookup"),
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
                        println!("wrote all {} bytes to frontend: {}", written, frontend);
                        println!("starting next read from backend: {}", backend);

                        io.run(Task::Read{
                            fd: backend,
                            buffer: buffer,
                        })?;
                    } else {
                        println!("wrote all {} bytes to backend: {}", written, backend);
                        println!("starting next read from frontend: {}", frontend);

                        io.run(Task::Read{
                            fd: frontend,
                            buffer: buffer,
                        })?;
                    }
                } else {
                    println!("incomplete write: {} {} != {}", fd, written, size);

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
