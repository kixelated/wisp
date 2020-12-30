use std::net::TcpListener;
use std::os::unix::io::AsRawFd;

use wisp::{Runtime, Task};

fn main() -> anyhow::Result<()> {
    let mut io = Runtime::new(256)?;

    let listener = TcpListener::bind(("127.0.0.1", 8080))?;
    let listener_fd = listener.as_raw_fd();
    println!("listen {}", listener.local_addr()?);

    io.run(Task::Accept{fd: listener_fd})?;

    loop {
        let (_id, task, ret) = io.wait()?;

        match task {
            Task::Accept{..} => {
                let fd = ret?;
                let buffer = vec![0u8; 2048].into_boxed_slice();

                io.run(Task::Accept{fd: listener_fd})?;
                io.run(Task::Read{fd: fd, buffer: buffer})?;
            },
            Task::Close{..} => {
                println!("closed");
            },
            /*
            Task::Connect => {
                println!("todo");
            },
            */
            Task::Read{ fd, buffer, .. } => {
                let size = ret? as usize;

                if size == 0 {
                    println!("unexpected EOF");
                    io.run(Task::Close{fd: fd})?;
                    continue
                }

                let buffer = &buffer[..size];

                println!("read {}", String::from_utf8_lossy(buffer));

                let _ = io.run(Task::Write{fd: fd, buffer: buffer.into()})?;
                let _ = io.run_after(Task::Close{fd: fd})?;
            },
            Task::Write{ buffer, .. } => {
                let size = ret? as usize;
                let buffer = &buffer[..size];

                println!("wrote {}", String::from_utf8_lossy(buffer));
            },
        }
    }
}
