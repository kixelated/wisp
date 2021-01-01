use std::os::unix::io::AsRawFd;
use std::{mem, net, ops, ptr, time};

use io_uring::opcode::{self, types};
use io_uring::squeue::Entry;

use enum_dispatch::enum_dispatch;

use super::tcp;

// Accept a TCP connection.
pub struct Accept {
    pub socket: net::TcpListener,
}

pub type TaskId = usize;

impl Task for Accept {
    fn entry(&mut self) -> Entry {
        opcode::Accept::new(
            types::Fd(self.socket.as_raw_fd()),
            ptr::null_mut(),
            ptr::null_mut(),
        )
        .build()
    }
}

pub struct Cancel {
    pub id: usize,
}

impl Task for Cancel {
    fn entry(&mut self) -> Entry {
        opcode::AsyncCancel::new(self.id as _).build()
    }
}

// Dial a TCP connection to the given address.
pub struct Connect {
    pub socket: tcp::Reader,
    addr: Box<net::SocketAddr>,
}

impl Connect {
    pub fn new(socket: tcp::Reader, addr: net::SocketAddr) -> Self {
        Self {
            socket,
            addr: Box::new(addr),
        }
    }
}

impl Task for Connect {
    fn entry(&mut self) -> Entry {
        let (addr, size) = match *self.addr {
            net::SocketAddr::V4(ref a) => (
                a as *const _ as *const _,
                mem::size_of_val(a) as libc::socklen_t,
            ),
            net::SocketAddr::V6(ref a) => (
                a as *const _ as *const _,
                mem::size_of_val(a) as libc::socklen_t,
            ),
        };

        opcode::Connect::new(types::Fd(self.socket.as_raw_fd()), addr, size).build()
    }
}

// Read from a TCP socket.
pub struct Read {
    pub socket: tcp::Reader, // read data from this file descriptor
    pub buffer: Box<[u8]>,   // buffer that will contain the data
}

impl Task for Read {
    fn entry(&mut self) -> Entry {
        opcode::Read::new(
            types::Fd(self.socket.as_raw_fd()),
            self.buffer.as_mut_ptr(),
            self.buffer.len() as _,
        )
        .build()
    }
}

pub struct Timeout {
    duration: types::Timespec,
}

impl Timeout {
    pub fn new(duration: time::Duration) -> Self {
        let duration = types::Timespec {
            tv_sec: duration.as_secs() as _,
            tv_nsec: duration.subsec_nanos() as _,
        };

        Self { duration }
    }
}

impl Task for Timeout {
    fn entry(&mut self) -> Entry {
        opcode::LinkTimeout::new(&self.duration).build()
    }
}

// Write to a TCP socket.
pub struct Write {
    pub socket: tcp::Writer, // write data to this file descriptor
    pub buffer: Box<[u8]>,   // buffer that contains the data
    pub start: usize,
    pub end: usize,
}

impl Write {
    pub fn new<R>(socket: tcp::Writer, buffer: Box<[u8]>, range: R) -> Self
    where
        R: ops::RangeBounds<usize>,
    {
        let start = match range.start_bound() {
            ops::Bound::Included(n) => *n,
            ops::Bound::Excluded(n) => n + 1,
            ops::Bound::Unbounded => 0,
        };

        let end = match range.end_bound() {
            ops::Bound::Included(n) => n + 1,
            ops::Bound::Excluded(n) => *n,
            ops::Bound::Unbounded => buffer.len(),
        };

        Self {
            socket,
            buffer,
            start,
            end,
        }
    }
}

impl Task for Write {
    fn entry(&mut self) -> Entry {
        let buffer = &mut self.buffer[self.start..self.end];
        opcode::Write::new(
            types::Fd(self.socket.as_raw_fd()),
            buffer.as_mut_ptr(),
            buffer.len() as _,
        )
        .build()
    }
}

#[enum_dispatch]
pub trait Task {
    fn entry(&mut self) -> Entry;
}

#[enum_dispatch(Task)]
pub enum TaskType {
    Accept,
    Cancel,
    Connect,
    Read,
    Timeout,
    Write,
}
