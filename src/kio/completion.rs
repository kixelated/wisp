use std::os::unix::io::FromRawFd;
use std::{io, net};

use enum_dispatch::enum_dispatch;

use super::task;

pub struct Accept {
    pub task: task::Accept,
    pub socket: Result<net::TcpStream, io::Error>, // the new connection
}

impl Accept {
    pub fn new(task: task::Accept, ret: i32) -> Self {
        let socket = if ret >= 0 {
            Ok(unsafe { net::TcpStream::from_raw_fd(ret) })
        } else {
            Err(io::Error::from_raw_os_error(-ret))
        };

        Self { task, socket }
    }
}

pub struct Cancel {
    pub task: task::Cancel,
    pub result: Result<(), io::Error>,
}

impl Cancel {
    pub fn new(task: task::Cancel, ret: i32) -> Self {
        let result = if ret >= 0 || ret == -libc::EALREADY {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(-ret))
        };

        Self { task, result }
    }
}

pub struct Connect {
    pub task: task::Connect,
    pub result: Result<(), io::Error>, // result of the connection
}

impl Connect {
    pub fn new(task: task::Connect, ret: i32) -> Self {
        let result = if ret >= 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(-ret))
        };

        Self { task, result }
    }
}

pub struct Read {
    pub task: task::Read,
    pub size: Result<usize, io::Error>, // number of bytes that were read
}

impl Read {
    pub fn new(task: task::Read, ret: i32) -> Self {
        let size = if ret >= 0 {
            Ok(ret as usize)
        } else {
            Err(io::Error::from_raw_os_error(-ret))
        };

        Self { task, size }
    }
}

pub struct ReadFixed {
    pub task: task::ReadFixed,
    pub size: Result<usize, io::Error>, // number of bytes that were read
}

impl ReadFixed {
    pub fn new(task: task::ReadFixed, ret: i32) -> Self {
        let size = if ret >= 0 {
            Ok(ret as usize)
        } else {
            Err(io::Error::from_raw_os_error(-ret))
        };

        Self { task, size }
    }
}

pub struct Timeout {
    pub task: task::Timeout,
    pub result: Result<(), io::Error>,
}

impl Timeout {
    pub fn new(task: task::Timeout, ret: i32) -> Self {
        let result = if ret >= 0 {
            Ok(())
        } else {
            Err(io::Error::from_raw_os_error(-ret))
        };

        Self { task, result }
    }
}

pub struct Write {
    pub task: task::Write,
    pub size: Result<usize, io::Error>, // number of bytes that were written
}

impl Write {
    pub fn new(task: task::Write, ret: i32) -> Self {
        let size = if ret >= 0 {
            Ok(ret as usize)
        } else {
            Err(io::Error::from_raw_os_error(-ret))
        };

        Self { task, size }
    }
}

pub struct WriteFixed {
    pub task: task::WriteFixed,
    pub size: Result<usize, io::Error>, // number of bytes that were written
}

impl WriteFixed {
    pub fn new(task: task::WriteFixed, ret: i32) -> Self {
        let size = if ret >= 0 {
            Ok(ret as usize)
        } else {
            Err(io::Error::from_raw_os_error(-ret))
        };

        Self { task, size }
    }
}

#[enum_dispatch]
pub enum CompletionType {
    Accept,
    Cancel,
    Connect,
    Read,
    ReadFixed,
    Timeout,
    Write,
    WriteFixed,
}

impl CompletionType {
    pub fn new(task: task::TaskType, ret: i32) -> Self {
        match task {
            task::TaskType::Accept(task) => CompletionType::Accept(Accept::new(task, ret)),
            task::TaskType::Cancel(task) => CompletionType::Cancel(Cancel::new(task, ret)),
            task::TaskType::Connect(task) => CompletionType::Connect(Connect::new(task, ret)),
            task::TaskType::Read(task) => CompletionType::Read(Read::new(task, ret)),
            task::TaskType::ReadFixed(task) => CompletionType::ReadFixed(ReadFixed::new(task, ret)),
            task::TaskType::Timeout(task) => CompletionType::Timeout(Timeout::new(task, ret)),
            task::TaskType::Write(task) => CompletionType::Write(Write::new(task, ret)),
            task::TaskType::WriteFixed(task) => CompletionType::WriteFixed(WriteFixed::new(task, ret)),
        }
    }
}
