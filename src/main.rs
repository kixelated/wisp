use std::net;
use std::collections::HashMap;

use wisp::{Runtime};
use wisp::{task};
use wisp::completion::CompletionType;

enum State {
    Connecting {
        frontend: net::TcpStream,
    },
    ReadRequest {
        backend: net::TcpStream,
    },
    WriteRequest {
        frontend: net::TcpStream,
    },
    ReadResponse {
        frontend: net::TcpStream,
    },
    WriteResponse {
        backend: net::TcpStream,
    },
}

fn main() -> anyhow::Result<()> {
    let mut uring = io_uring::IoUring::new(1024)?;
    let mut io = Runtime::new(&mut uring)?;

    let backend_addr: net::SocketAddr = "127.0.0.1:9001".parse()?;
    let frontend_addr: net::SocketAddr = "127.0.0.1:8080".parse()?;

    let accept = net::TcpListener::bind(frontend_addr)?;
    println!("listen {}", accept.local_addr()?);

    io.run(task::Accept::new(accept).into())?;

    let mut connections = HashMap::new();

    loop {
        let (id, completion) = io.wait()?;

        match completion {
            CompletionType::Accept(accept) => {
                let frontend = accept.socket?;

                // Connect to the backend
                io.run(task::Connect::new(backend_addr)?.into())?;

                connections.insert(id, State::Connecting{frontend});

                // Queue up the accept again.
                io.run(accept.task.into())?;
            },
            CompletionType::Connect(connect) => {
                let state = connections.remove(&id);

                if let Err(err) = connect.result {
                    println!("failed to connect to backend: {:?}", err);
                    continue;
                }

                let frontend = match state {
                    Some(State::Connecting{ frontend }) => frontend,
                    Some(_) => anyhow::bail!("unexpected state"),
                    None => anyhow::bail!("no task in lookup"),
                };

                let task = connect.task;
                let backend = task.socket;
                let buffer = vec![0u8; 4096].into_boxed_slice();

                let id = io.run(task::Read::new(frontend, buffer).into())?;
                connections.insert(id, State::ReadRequest{backend});
            },
            CompletionType::Read(read) => {
                let state = connections.remove(&id);

                let size = match read.size {
                    Ok(size) => size,
                    Err(err) => {
                        println!("failed to read: {}", err);
                        continue;
                    }
                };

                let task = read.task;
                let (socket, buffer) = (task.socket, task.buffer);

                if size == 0 {
                    match state {
                        Some(State::ReadRequest{backend}) => {
                            backend.shutdown(net::Shutdown::Write)?;

                            let id = io.run(task::Read::new(backend, buffer).into())?;
                            connections.insert(id, State::ReadResponse{frontend: socket});
                        },
                        Some(State::ReadResponse{frontend}) => {
                            frontend.shutdown(net::Shutdown::Write)?;

                            // closed, both sockets are dropped
                        },
                        Some(_) => anyhow::bail!("unexpected state"),
                        None => anyhow::bail!("no task in lookup"),
                    }
                } else {
                    match state {
                        Some(State::ReadRequest{backend}) => {
                            let id = io.run(task::Write::new(backend, buffer, 0..size).into())?;
                            connections.insert(id, State::WriteRequest{frontend: socket});
                        },
                        Some(State::ReadResponse{frontend}) => {
                            let id = io.run(task::Write::new(frontend, buffer, 0..size).into())?;
                            connections.insert(id, State::WriteResponse{backend: socket});
                        },
                        Some(_) => anyhow::bail!("unexpected state"),
                        None => anyhow::bail!("no task in lookup"),
                    }
                }
            }
            CompletionType::Write(write) => {
                let state = connections.remove(&id);

                let written = match write.size {
                    Ok(size) => size,
                    Err(err) => {
                        println!("failed to write: {}", err);
                        continue;
                    }
                };

                let task = write.task;
                let (socket, buffer, range) = (task.socket, task.buffer, task.range);

                if written == range.end - range.start {
                    match state {
                        Some(State::WriteRequest{frontend}) => {
                            io.run(task::Read::new(frontend, buffer).into())?;
                            connections.insert(id, State::ReadRequest{backend: socket});
                        },
                        Some(State::WriteResponse{backend}) => {
                            let id = io.run(task::Read::new(backend, buffer).into())?;
                            connections.insert(id, State::ReadResponse{frontend: socket});
                        },
                        Some(_) => anyhow::bail!("unexpected state"),
                        None => anyhow::bail!("no task in lookup"),
                    }
                } else {
                    let id = io.run(task::Write::new(socket, buffer, range.start+written..range.end).into())?;
                    match state {
                        Some(state) => connections.insert(id, state),
                        None => anyhow::bail!("no task in lookup"),
                    };
                }
            }
        }
    }
}
