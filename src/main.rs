use std::collections::HashMap;
use std::net;
use std::os::unix::io::FromRawFd;

use wisp::kio::completion::CompletionType;
use wisp::kio::{buffer, tcp, Kio};

use nix::sys::socket;

use slab::Slab;

struct Pipe {
    reader: Option<tcp::Reader>,
    writer: Option<tcp::Writer>,
    buffer: Option<buffer::Slice>,
}

fn main() -> anyhow::Result<()> {
    let mut uring = io_uring::IoUring::new(1024)?;
    let mut kio = Kio::new(&mut uring)?;

    // 4k bytes each
    kio.prepare_buffers(1024, 4096)?;

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

                let incoming_buffer = buffer::Slice::new(1024);
                let outgoing_buffer = buffer::Slice::new(4096);

                let incoming = Pipe {
                    reader: None,
                    writer: Some(backend_writer),
                    buffer: Some(incoming_buffer),
                };

                let outgoing = Pipe {
                    reader: None,
                    writer: Some(frontend_writer),
                    buffer: Some(outgoing_buffer),
                };

                let outgoing_id = pipes.insert(outgoing);
                let incoming_id = pipes.insert(incoming);

                let buffer = buffer::Slice::new(1024);

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

                let buffer = buffer::Slice::new(4096);

                //tasks.insert(kio.timeout(time::Duration::from_secs(10)), pipe_id);
                tasks.insert(kio.read(connect.task.socket, buffer), pipe_id);
            }
            CompletionType::Read(read) => {
                let task = read.task;

                let pipe_id = tasks.remove(&task_id).unwrap();
                let pipe = match pipes.get_mut(pipe_id) {
                    Some(pipe) => pipe,
                    None => continue,
                };

                let size = match read.size {
                    Ok(size) => size,
                    Err(err) => {
                        // TODO
                        println!("failed to read: {}", err);
                        pipes.remove(pipe_id);

                        continue;
                    }
                };

                //println!("read: {} {}", pipe_id, size);

                if size == 0 {
                    pipes.remove(pipe_id);
                //println!("closing: {}", pipe_id);
                } else {
                    let writer = pipe.writer.take().unwrap();
                    let buffer = pipe.buffer.take().unwrap();

                    //tasks.insert(kio.timeout(time::Duration::from_secs(5)), pipe_id);
                    tasks.insert(kio.write_then(writer, task.buffer, 0..size), pipe_id);
                    //tasks.insert(kio.timeout(time::Duration::from_secs(5)), pipe_id);
                    tasks.insert(kio.read(task.socket, buffer), pipe_id);
                }
            }
            CompletionType::Write(write) => {
                let task = write.task;

                let pipe_id = tasks.remove(&task_id).unwrap();
                let pipe = match pipes.get_mut(pipe_id) {
                    Some(pipe) => pipe,
                    None => continue,
                };

                let size = match write.size {
                    Ok(size) => size,
                    Err(err) => {
                        println!("failed to write: {}", err);
                        pipes.remove(pipe_id);
                        continue;
                    }
                };

                //println!("write: {} {}", pipe_id, size);

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
