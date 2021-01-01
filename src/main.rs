use std::collections::HashMap;
use std::os::unix::io::FromRawFd;
use std::net;

use wisp::kio::completion::CompletionType;
use wisp::kio::{Kio,tcp};

use nix::sys::socket;

use slab::Slab;

struct Pipe {
    reader: Option<tcp::Reader>,
    writer: Option<tcp::Writer>,
    buffer: Option<Box<[u8]>>,
}

fn main() -> anyhow::Result<()> {
    let mut uring = io_uring::IoUring::new(1024)?;
    let mut kio = Kio::new(&mut uring)?;

    let backend_addr: net::SocketAddr = "127.0.0.1:9001".parse()?;
    let frontend_addr: net::SocketAddr = "127.0.0.1:8080".parse()?;

    let listener = net::TcpListener::bind(frontend_addr)?;
    println!("listen {}", listener.local_addr()?);

    kio.accept(listener);

    let mut tasks = HashMap::new(); // TODO replace with some form of vector
    let mut pipes = Slab::new();

    loop {
        let (task_id, completion) = kio.wait()?;

        match completion {
            CompletionType::Accept(accept) => {
                let frontend = accept.socket?;

                // Create a new IPv4 TCP socket.
                // We need to use the nix package because there's no way to do this in the stdlib.
                let backend_fd = socket::socket(
                    socket::AddressFamily::Inet, // TODO support ipv6
                    socket::SockType::Stream,
                    socket::SockFlag::empty(),
                    socket::SockProtocol::Tcp,
                )?;

                let backend = unsafe { net::TcpStream::from_raw_fd(backend_fd) };

                let (frontend_reader, frontend_writer) = tcp::split(frontend);
                let (backend_reader, backend_writer) = tcp::split(backend);

                let incoming = Pipe {
                    reader: None,
                    writer: Some(backend_writer),
                    buffer: Some(vec![0u8; 2048].into_boxed_slice()),
                };

                let outgoing = Pipe {
                    reader: None,
                    writer: Some(frontend_writer),
                    buffer: Some(vec![0u8; 4096].into_boxed_slice()),
                };

                let outgoing_id = pipes.insert(outgoing);
                let incoming_id = pipes.insert(incoming);

                let buffer = vec![0u8; 2048].into_boxed_slice();

                // Connect to the backend first.
                //tasks.insert(kio.timeout(time::Duration::from_secs(5)), outgoing_id);
                tasks.insert(kio.connect_then(backend_reader, backend_addr), outgoing_id);
                //tasks.insert(kio.timeout(time::Duration::from_secs(5)), incoming_id);
                tasks.insert(kio.read(frontend_reader, buffer), incoming_id);

                // Queue up the accept again.
                kio.accept(accept.task.socket);
            }
            CompletionType::Connect(connect) => {
                let pipe_id = tasks.remove(&task_id).unwrap();

                if let Err(err) = connect.result {
                    println!("failed to connect to backend: {:?}", err);
                    continue;
                }

                //println!("connected to backend: {}", pipe_id);

                let buffer = vec![0u8; 4096].into_boxed_slice();
                //tasks.insert(kio.timeout(time::Duration::from_secs(10)), pipe_id);
                tasks.insert(kio.read(connect.task.socket, buffer), pipe_id);
            }
            CompletionType::Read(read) => {
                let pipe_id = tasks.remove(&task_id).unwrap();

                let size = match read.size {
                    Ok(size) => size,
                    Err(err) => {
                        // TODO
                        // pipe.reader.replace(read.task.socket);
                        // pipe.buffer.replace(read.task.buffer);
                        println!("failed to read: {}", err);
                        continue;
                    }
                };

                //println!("read: {} {}", pipe_id, size);

                if size == 0 {
                    pipes.remove(pipe_id);
                //println!("closing: {}", pipe_id);
                } else {
                    let pipe = pipes.get_mut(pipe_id).unwrap();

                    let writer = pipe.writer.take().unwrap();
                    let buffer = pipe.buffer.take().unwrap();

                    //tasks.insert(kio.timeout(time::Duration::from_secs(5)), pipe_id);
                    tasks.insert(kio.write_then(writer, read.task.buffer, 0..size), pipe_id);
                    //tasks.insert(kio.timeout(time::Duration::from_secs(5)), pipe_id);
                    tasks.insert(kio.read(read.task.socket, buffer), pipe_id);
                }
            }
            CompletionType::Write(write) => {
                let pipe_id = tasks.remove(&task_id).unwrap();

                let size = match write.size {
                    Ok(size) => size,
                    Err(err) => {
                        println!("failed to write: {}", err);
                        continue;
                    }
                };

                //println!("write: {} {}", pipe_id, size);

                let pipe = pipes.get_mut(pipe_id).unwrap();
                let task = write.task;

                if size == task.end - task.start {
                    pipe.writer.replace(task.socket);

                    if let Some(reader) = pipe.reader.take() {
                        //tasks.insert(kio.timeout(time::Duration::from_secs(5)), pipe_id);
                        kio.read(reader, task.buffer);
                    } else {
                        pipe.buffer.replace(task.buffer);
                    }
                } else {
                    // Continue writing the rest of data.
                    //tasks.insert(kio.timeout(time::Duration::from_secs(5)), pipe_id);
                    tasks.insert(
                        kio.write(task.socket, task.buffer, task.start + size..task.end),
                        pipe_id,
                    );
                }
            }
            CompletionType::Timeout(timeout) => {
                let _pipe_id = tasks.remove(&task_id).unwrap();

                if let Err(err) = timeout.result {
                    println!("failed to timeout: {}", err);
                    // TODO close pipe
                    continue;
                }

                //println!("timeout finished");
            }
            _ => {
                panic!("unknown completion")
            }
        }
    }
}
