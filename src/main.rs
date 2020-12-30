use std::net::TcpListener;
use std::os::unix::io::AsRawFd;

use wisp::{Runtime, Task};

fn main() -> anyhow::Result<()> {
    let mut runtime = Runtime::new(256)?;

    let listener = TcpListener::bind(("127.0.0.1", 8080))?;
    let listener_fd = listener.as_raw_fd();
    println!("listen {}", listener.local_addr()?);

    runtime.run(Task::Accept{fd: listener_fd})?;

    loop {
        let (_id, task, ret) = runtime.wait()?;

        match task {
            Task::Accept{..} => {
                let fd = ret?;
                let buffer = vec![0u8; 2048].into_boxed_slice();

                runtime.run(Task::Accept{fd: listener_fd})?;
                runtime.run(Task::Read{fd: fd, buffer: buffer})?;
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
                    println!("EOF");

                }

                let buffer = &buffer[..size];

                println!("read {}", String::from_utf8_lossy(buffer));

                let _ = runtime.run(Task::Write{fd: fd, buffer: buffer.into()})?;
                let _ = runtime.run_after(Task::Close{fd: fd})?;
            },
            Task::Write{ buffer, .. } => {
                let size = ret? as usize;
                let buffer = &buffer[..size];

                println!("wrote {}", String::from_utf8_lossy(buffer));
            },
        }
    }
}
