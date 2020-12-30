use super::Task;
use std::{io, ptr};

use io_uring::IoUring;
use io_uring::opcode::{self, types};
use io_uring::squeue::Flags;

use anyhow::{anyhow, Result};
use slab::Slab;

pub struct Runtime {
    uring: IoUring,
    tasks: Slab<Task>,
}

impl Runtime {
    pub fn new(size: u32) -> Result<Self> {
        Ok(Self{
            uring: IoUring::new(size)?,
            tasks: Slab::with_capacity(size as _),
        })
    }

    /// Run the given task asynchronously.
    pub fn run(&mut self, task: Task) -> Result<usize> {
        self.run_flags(task, Flags::empty())
    }

    /// Run the given task after the previous task has finished.
    pub fn run_after(&mut self, task: Task) -> Result<usize> {
        self.run_flags(task, Flags::IO_HARDLINK)
    }

    /// Run the given task after the previous task has finished successfully.
    pub fn run_after_ok(&mut self, task: Task) -> Result<usize> {
        self.run_flags(task, Flags::IO_LINK)
    }

    /// Run the given task after all current tasks have finished and before any future tasks.
    pub fn run_barrier(&mut self, task: Task) -> Result<usize> {
        self.run_flags(task, Flags::IO_DRAIN)
    }

    fn run_flags(&mut self, mut task: Task, flags: Flags) -> Result<usize> {
        let entry = match task {
            Task::Accept{ fd } => {
                opcode::Accept::new(types::Fd(fd), ptr::null_mut(), ptr::null_mut()).build()
            },
            Task::Close { fd } => {
                opcode::Close::new(types::Fd(fd)).build()
            },
            /*
            Task::Connect{ fd, addr } => {
                let socket_addr = match addr {
                    V4(addr) => {
                        libc::sockaddr_in{
                           sin_family: libc::AF_INET,
                           sin_port: addr.port(),
                           sin_addr: addr.ip(),
                        }
                    },
                    V6(addr) => {
                        libc::sockaddr_in6{
                            sin6_family: libc::AF_INET,
                            sin6_port: addr.port(),
                            sin6_flowinfo: addr.flowinfo(),
                            sin6_addr: addr.ip(),
                            sin6_scope_id: addr.scope_id(),
                        }
                    },
                };

                opcode::Connect::new(types::Fd(fd), sock_addr.as_ptr(), mem::size_of(sock_addr) as _).build()
            },
            */
            Task::Read{ fd, ref mut buffer } => {
                opcode::Read::new(types::Fd(fd), buffer.as_mut_ptr(), buffer.len() as _).build()
            },
            Task::Write{ fd, ref mut buffer } => {
                opcode::Write::new(types::Fd(fd), buffer.as_mut_ptr(), buffer.len() as _).build()
            },
        };

        let task_id = self.tasks.insert(task);
        let entry = entry.user_data(task_id as _).flags(flags);

        let mut available = self.uring.submission().available();
        unsafe { 
            match available.push(entry) {
                Ok(_) => Ok(task_id),
                Err(_) => anyhow::bail!("failed to push task"),
            }
        }
    }

    pub fn wait(&mut self) -> Result<(usize, Task, Result<i32>)> {
        while self.uring.completion().is_empty() {
            self.uring.submitter().submit_and_wait(1)?;
        }

        let available = self.uring.completion().available();

        let entry = available.into_iter().next().unwrap();

        let ret = entry.result();
        let task_id = entry.user_data() as usize;
        let task = self.tasks.remove(task_id);

        let completion = if ret >= 0 {
           Ok(ret)
        } else {
           Err(anyhow!("{}", io::Error::from_raw_os_error(-ret)))
        };

        Ok((task_id, task, completion))
    }
}
