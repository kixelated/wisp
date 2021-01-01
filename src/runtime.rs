use super::task::{Task,TaskType};
use super::completion::CompletionType;

use std::collections::LinkedList;

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
}

impl<'a> Runtime<'a> {
    pub fn new(uring: &'a mut IoUring) -> Result<Self> {
        let (submitter, submissions, completions) = uring.split();

        Ok(Self {
            submitter,
            submissions: submissions.available(),
            completions: completions.available(),

            tasks: Slab::new(),
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

        if self.submissions.is_full() {
            self.submissions.sync();
            if self.submissions.is_full() {
                self.backlog.push_back(entry);
                return Ok(task_id)
            }
        }

        unsafe {
            if self.submissions.push(entry).is_err() {
                anyhow::bail!("failed to push task");
            }
        }

        Ok(task_id)
    }

    pub fn wait(&mut self) -> Result<(usize, CompletionType)> {
        let entry = match self.completions.next() {
            Some(e) => e,
            None => {
                self.completions.sync();
                match self.completions.next() {
                    Some(e) => e,
                    None => {
                        self.submitter.submit_and_wait(1)?;
                        self.completions.sync();
                        self.completions.next().unwrap()
                    },
                }
            },
        };

        let ret = entry.result();
        let task_id = entry.user_data() as usize;
        let task = self.tasks.remove(task_id);
        let completion = CompletionType::new(task, ret);

        self.run_backlog()?;

        Ok((task_id, completion))
    }

    pub fn run_backlog(&mut self) -> Result<()> {
        if self.backlog.is_empty() {
            return Ok(())
        }

        self.submissions.sync();

        while !self.submissions.is_full() {
            let entry = match self.backlog.pop_front() {
                Some(entry) => entry,
                None => return Ok(()),
            };

            unsafe {
                if self.submissions.push(entry).is_err() {
                    anyhow::bail!("failed to push backlog task");
                }
            }
        }

        return Ok(())
    }
}
