use std::{net, ops, time};
use std::collections::LinkedList;

use super::completion::CompletionType;
use super::task::{Task, TaskId, TaskType};
use super::{task, tcp, buffer};

use io_uring::squeue::{Entry, Flags};
use io_uring::IoUring;

use anyhow::Result;
use slab::Slab;

pub struct Runtime<'a> {
    submitter: io_uring::Submitter<'a>,
    submissions: io_uring::squeue::AvailableQueue<'a>,
    completions: io_uring::cqueue::AvailableQueue<'a>,

    tasks: Slab<TaskType>,
    backlog: LinkedList<Entry>,

    buffers: buffer::Pool,
}

impl<'a> Runtime<'a> {
    pub fn new(uring: &'a mut IoUring) -> Result<Self> {
        if !uring.params().is_feature_fast_poll() {
            anyhow::bail!("missing fast poll");
        }

        let (submitter, submissions, completions) = uring.split();

        Ok(Self {
            submitter,
            submissions: submissions.available(),
            completions: completions.available(),

            tasks: Slab::new(),
            backlog: LinkedList::new(),

            buffers: buffer::Pool::new(),
        })
    }

    // NOTE: will wait for the ring to idle.
    pub fn prepare_buffers(&mut self, count: usize, size: usize) -> Result<()>{
        let mut register_buffers = Vec::with_capacity(count);
        
        for id in 0..count {
            let buffer = buffer::Fixed::new(id, size);

            register_buffers.push(libc::iovec{
                iov_base: buffer.as_ptr() as _,
                iov_len: buffer.len(),
            });

            self.buffers.give(buffer);
        }

        self.submitter.register_buffers(register_buffers.as_slice())?;

        Ok(())
    }

    pub fn buffers(&mut self) -> &mut buffer::Pool {
        &mut self.buffers
    }

    pub fn accept(&mut self, socket: net::TcpListener) -> TaskId {
        self.run(task::Accept { socket }.into())
    }

    pub fn cancel(&mut self, id: TaskId) -> TaskId {
        self.run(task::Cancel { id }.into())
    }

    pub fn cancel_then(&mut self, id: TaskId) -> TaskId {
        self.run_then(task::Cancel { id }.into())
    }

    pub fn connect(&mut self, socket: tcp::Reader, addr: net::SocketAddr) -> TaskId {
        self.run(task::Connect::new(socket, addr).into())
    }

    pub fn connect_then(&mut self, socket: tcp::Reader, addr: net::SocketAddr) -> TaskId {
        self.run_then(task::Connect::new(socket, addr).into())
    }

    pub fn read(&mut self, socket: tcp::Reader, buffer: buffer::Slice) -> TaskId {
        self.run(task::Read { socket, buffer }.into())
    }

    pub fn read_then(&mut self, socket: tcp::Reader, buffer: buffer::Slice) -> TaskId {
        self.run_then(task::Read { socket, buffer }.into())
    }

    pub fn read_fixed(&mut self, socket: tcp::Reader, buffer: buffer::Fixed) -> TaskId {
        self.run(task::ReadFixed{socket, buffer}.into())
    }

    pub fn read_fixed_then(&mut self, socket: tcp::Reader, buffer: buffer::Fixed) -> TaskId {
        self.run_then(task::ReadFixed{socket, buffer}.into())
    }

    // Applies a timeout to the next chain of tasks.
    pub fn timeout(&mut self, duration: time::Duration) -> TaskId {
        self.run_then(task::Timeout::new(duration).into())
    }

    pub fn write<R>(&mut self, socket: tcp::Writer, buffer: buffer::Slice, range: R) -> TaskId
    where
        R: ops::RangeBounds<usize>,
    {
        self.run(task::Write::new(socket, buffer, range).into())
    }

    pub fn write_then<R>(&mut self, socket: tcp::Writer, buffer: buffer::Slice, range: R) -> TaskId
    where
        R: ops::RangeBounds<usize>,
    {
        self.run_then(task::Write::new(socket, buffer, range).into())
    }

    pub fn write_fixed<R>(&mut self, socket: tcp::Writer, buffer: buffer::Fixed, range: R) -> TaskId
    where
        R: ops::RangeBounds<usize>,
    {
        self.run(task::WriteFixed::new(socket, buffer, range).into())
    }

    pub fn write_fixed_then<R>(&mut self, socket: tcp::Writer, buffer: buffer::Fixed, range: R) -> TaskId
    where
        R: ops::RangeBounds<usize>,
    {
        self.run_then(task::WriteFixed::new(socket, buffer, range).into())
    }

    /// Run the given task asynchronously.
    pub fn run(&mut self, task: TaskType) -> TaskId {
        self.run_flags(task, Flags::empty())
    }

    /// Run the task and block the next task until this one has finished successfully.
    /// NOTE: The task will be considered a failure on a short read/write.
    pub fn run_then(&mut self, task: TaskType) -> TaskId {
        self.run_flags(task, Flags::IO_LINK)
    }

    /// Run the task and block the next task until this one has finished.
    pub fn run_before(&mut self, task: TaskType) -> TaskId {
        self.run_flags(task, Flags::IO_HARDLINK)
    }

    /// Run the given task after all current tasks have finished and before any future tasks.
    pub fn run_drain(&mut self, task: TaskType) -> TaskId {
        self.run_flags(task, Flags::IO_DRAIN)
    }

    fn run_flags(&mut self, mut task: TaskType, flags: Flags) -> TaskId {
        let entry = task.entry();
        let id = self.tasks.insert(task);
        let entry = entry.user_data(id as _).flags(flags);

        if self.submissions.is_full() {
            self.backlog.push_back(entry);
        } else {
            unsafe {
                self.submissions.push(entry).ok();
            }
        }

        id
    }

    pub fn wait(&mut self) -> Result<(TaskId, CompletionType)> {
        let entry = match self.completions.next() {
            Some(e) => e,
            None => {
                // Try to refresh once instead of the costlier syscall.
                self.completions.sync();

                match self.completions.next() {
                    Some(e) => e,
                    None => {
                        // Push any backlog items before submit/wait
                        self.run_backlog();

                        // Make sure we flush our new tasks first.
                        self.submissions.sync();

                        // Perform the syscall and wait for 1 task to be done.
                        self.submitter.submit_and_wait(1)?;

                        // Fetch the new completion.
                        self.completions.sync();

                        // Get the guaranteed completion.
                        self.completions.next().unwrap()
                    }
                }
            }
        };

        let ret = entry.result();
        let id = entry.user_data() as TaskId;
        let task = self.tasks.remove(id);
        let completion = CompletionType::new(task, ret);

        Ok((id, completion))
    }

    pub fn run_backlog(&mut self) {
        if self.backlog.is_empty() {
            return;
        }

        while !self.submissions.is_full() {
            let entry = match self.backlog.pop_front() {
                Some(entry) => entry,
                None => return,
            };

            unsafe {
                // won't fail
                self.submissions.push(entry).ok();
            }
        }
    }
}
