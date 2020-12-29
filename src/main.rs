use std::net::TcpListener;
use std::os::unix::io::AsRawFd;

use wisp::{Runtime, Task};

fn main() -> anyhow::Result<()> {
    let mut runtime = Runtime::new(256)?;

    let listener = TcpListener::bind(("127.0.0.1", 8080))?;
    println!("listen {}", listener.local_addr()?);

    let accept_task = Task::Accept{fd: listener.as_raw_fd()};
    let _accept_id = runtime.push(accept_task)?;

    loop {
        let (_id, task, ret) = runtime.run()?;

        match task {
            Task::Accept{..} => {
                let fd = ret?;
                let _accept_id = runtime.push(accept_task)?;

                let buffer = vec![0u8; 2048].into_boxed_slice();

                let read_task = Task::Read{
                    fd: fd, 
                    buffer: buffer,
                };

                let _read_id = runtime.push(read_task)?;
            },
            /*
            Task::Connect => {
                println!("todo");
            },
            */
            Task::Read{ fd, buffer, .. } => {
                let size = ret?;

                if size == 0 {
                    println!("EOF");
                    unsafe {
                        // TODO move to runtime?
                        libc::close(fd);
                    }
                }

                let buffer = buffer[..size];

                println!("read {}", String::from_utf8_lossy(buffer));

                let write_task = Task::Write{
                    fd: fd,
                    buffer: buffer,
                };

                let _write_id = runtime.push(write_task)?;
            },
            Task::Write{ buffer, .. } => {
                let size = ret?;
                let buffer = buffer[..size];

                println!("wrote {}", String::from_utf8_lossy(buffer));
            },
        }
    }
}
