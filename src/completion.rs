use std::{io, net};
use std::os::unix::io::FromRawFd;

use enum_dispatch::enum_dispatch;

use super::task;

pub struct Accept {
    pub task: task::Accept,
    pub socket: Result<net::TcpStream, io::Error>, // the new connection
}

impl Accept {
    pub fn new(task: task::Accept, ret: i32) -> Self {
        let socket = if ret >= 0 {
            Ok(unsafe{net::TcpStream::from_raw_fd(ret)})
        } else {
            Err(io::Error::from_raw_os_error(-ret))
        };

        Self{task, socket}
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

        Self{task, result}
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

        Self{task, size}
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

        Self{task, size}
    }
}

#[enum_dispatch]
pub trait Completion {}

#[enum_dispatch(Completion)]
pub enum CompletionType {
    Accept(Accept),
    Connect(Connect),
    Read(Read),
    Write(Write),
}

impl CompletionType {
    pub fn new(task: task::TaskType, ret: i32) -> Self {
        match task {
            task::TaskType::Accept(task) => CompletionType::Accept(Accept::new(task, ret)),
            task::TaskType::Connect(task) => CompletionType::Connect(Connect::new(task, ret)),
            task::TaskType::Read(task) => CompletionType::Read(Read::new(task, ret)),
            task::TaskType::Write(task) => CompletionType::Write(Write::new(task, ret)),
        }
    }
}
