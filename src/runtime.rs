use super::Task;

use std::{io, ptr};
use std::collections::LinkedList;

use io_uring::IoUring;
use io_uring::opcode::{self, types};
use io_uring::squeue::{Entry,Flags};

use anyhow::{anyhow, Result};
use slab::Slab;

pub struct Runtime {
    uring: IoUring,
    tasks: Slab<Task>,
    backlog: LinkedList<Entry>,
}

impl Runtime {
    pub fn new(size: u32) -> Result<Self> {
        Ok(Self{
            uring: IoUring::new(size)?,
            tasks: Slab::with_capacity(size as _),
            backlog: LinkedList::new(),
        })
    }

    /// Run the given task asynchronously.
    pub fn run(&mut self, task: Task) -> Result<usize> {
        self.run_flags(task, Flags::empty())
    }

    /// Run the given task after the previous task has finished successfully.
    pub fn then_run(&mut self, task: Task) -> Result<usize> {
        self.run_flags(task, Flags::IO_LINK)
    }

    /// Run the given task after the previous task has finished.
    pub fn after_run(&mut self, task: Task) -> Result<usize> {
        self.run_flags(task, Flags::IO_HARDLINK)
    }

    /// Run the given task after all current tasks have finished and before any future tasks.
    pub fn drain_then_run(&mut self, task: Task) -> Result<usize> {
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
            Task::Connect{ fd, addr } => {
                let (addr, size) = addr.as_ffi_pair();
                opcode::Connect::new(types::Fd(fd), addr, size).build()
            },
            Task::Read{ fd, ref mut buffer } => {
                opcode::Read::new(types::Fd(fd), buffer.as_mut_ptr(), buffer.len() as _).build()
            },
            Task::Write{ fd, ref mut buffer, offset, size } => {
                let buffer = &mut buffer[offset..offset+size];
                opcode::Write::new(types::Fd(fd), buffer.as_mut_ptr(), buffer.len() as _).build()
            },
        };

        let task_id = self.tasks.insert(task);
        let entry = entry.user_data(task_id as _).flags(flags);

        let mut available = self.uring.submission().available();
        if available.is_full() {
            self.backlog.push_back(entry);
            println!("backlog size: {}", self.backlog.len());
        } else {
            unsafe { 
                if let Err(_) = available.push(entry) {
                    anyhow::bail!("failed to push task");
                }
            }
        }

        Ok(task_id)
    }

    pub fn wait(&mut self) -> Result<(usize, Task, Result<usize>)> {
        while self.uring.completion().is_empty() {
            self.uring.submitter().submit_and_wait(1)?;
        }

        let available = self.uring.completion().available();

        let entry = available.into_iter().next().unwrap();

        let ret = entry.result();
        let task_id = entry.user_data() as usize;
        let task = self.tasks.remove(task_id);

        if !self.backlog.is_empty() {
            let mut available = self.uring.submission().available();
            let space = available.capacity() - available.len();

            for _ in 0..space {
                let entry = match self.backlog.pop_front() {
                    Some(entry) => entry,
                    None => break,
                };

                unsafe { 
                    if let Err(_) = available.push(entry) {
                        anyhow::bail!("failed to push backlog task");
                    }
                }
            }

            println!("backlog size: {}", self.backlog.len());
        }

        let completion = if ret >= 0 {
           Ok(ret as usize)
        } else {
           Err(anyhow!("{}", io::Error::from_raw_os_error(-ret)))
        };

        Ok((task_id, task, completion))
    }
}
