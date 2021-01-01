use super::task::{Task,TaskType};
use super::completion::CompletionType;

use std::collections::LinkedList;

use io_uring::squeue::{Entry, Flags};
use io_uring::IoUring;

use anyhow::Result;
use slab::Slab;

pub struct Runtime {
    uring: IoUring,
    tasks: Slab<TaskType>,
    backlog: LinkedList<Entry>,
}

impl Runtime {
    pub fn new(size: u32) -> Result<Self> {
        Ok(Self {
            uring: IoUring::new(size)?,
            tasks: Slab::with_capacity(size as _),
            backlog: LinkedList::new(),
        })
    }

    /// Run the given task asynchronously.
    pub fn run(&mut self, task: TaskType) -> Result<usize> {
        self.run_flags(task, Flags::empty())
    }

    /// Run the task and block the next task until this one has finished successfully.
    /// NOTE: The task will be considered a failure on a short read/write.
    pub fn run_then(&mut self, task: TaskType) -> Result<usize> {
        self.run_flags(task, Flags::IO_LINK)
    }

    /// Run the task and block the next task until this one has finished.
    pub fn run_before(&mut self, task: TaskType) -> Result<usize> {
        self.run_flags(task, Flags::IO_HARDLINK)
    }

    /// Run the given task after all current tasks have finished and before any future tasks.
    pub fn run_drain(&mut self, task: TaskType) -> Result<usize> {
        self.run_flags(task, Flags::IO_DRAIN)
    }

    fn run_flags(&mut self, mut task: TaskType, flags: Flags) -> Result<usize> {
        let entry = task.entry();
        let task_id = self.tasks.insert(task);
        let entry = entry.user_data(task_id as _).flags(flags);

        let mut available = self.uring.submission().available();
        if available.is_full() {
            self.backlog.push_back(entry);
            //println!("backlog size: {}", self.backlog.len());
        } else {
            unsafe {
                if available.push(entry).is_err() {
                    anyhow::bail!("failed to push task");
                }
            }
        }

        Ok(task_id)
    }

    pub fn wait(&mut self) -> Result<(usize, CompletionType)> {
        while self.uring.completion().is_empty() {
            self.uring.submitter().submit_and_wait(1)?;
        }

        let available = self.uring.completion().available();

        let entry = available.into_iter().next().unwrap();

        let ret = entry.result();
        let task_id = entry.user_data() as usize;
        let task = self.tasks.remove(task_id);
        let completion = CompletionType::new(task, ret);

        if !self.backlog.is_empty() {
            let mut available = self.uring.submission().available();
            let space = available.capacity() - available.len();

            for _ in 0..space {
                let entry = match self.backlog.pop_front() {
                    Some(entry) => entry,
                    None => break,
                };

                unsafe {
                    if available.push(entry).is_err() {
                        anyhow::bail!("failed to push backlog task");
                    }
                }
            }

            //println!("backlog size: {}", self.backlog.len());
        }

        Ok((task_id, completion))
    }
}
