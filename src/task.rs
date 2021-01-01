use std::{net, ptr, mem};
use std::os::unix::io::{AsRawFd, FromRawFd};

use nix::sys::socket;

use io_uring::squeue::Entry;
use io_uring::opcode::{self, types};

use enum_dispatch::enum_dispatch;

// Accept a TCP connection.
pub struct Accept {
    pub socket: net::TcpListener,
}

impl Accept {
    pub fn new(socket: net::TcpListener) -> Self {
        Self { socket }
    }
}

impl Task for Accept {
    fn entry(&mut self) -> Entry {
        opcode::Accept::new(
            types::Fd(self.socket.as_raw_fd()),
            ptr::null_mut(),
            ptr::null_mut(),
        ).build()
    }
}

// Dial a TCP connection to the given address.
pub struct Connect {
    pub socket: net::TcpStream,
    pub addr: net::SocketAddr,
}

impl Connect {
    pub fn new(addr: net::SocketAddr) -> Result<Self, nix::Error> {
        // Create a new IPv4 TCP socket.
        // We need to use the nix package because there's no way to do this in the stdlib.
        let fd = socket::socket(
            socket::AddressFamily::Inet, // TODO support ipv6
            socket::SockType::Stream,
            socket::SockFlag::empty(),
            socket::SockProtocol::Tcp,
        )?;

        let socket = unsafe { net::TcpStream::from_raw_fd(fd) };
        Ok(Self { socket, addr })
    }
}

impl Task for Connect {
    fn entry(&mut self) -> Entry {
        let (addr, size) = match self.addr {
            net::SocketAddr::V4(ref a) => {
                (a as *const _ as *const _, mem::size_of_val(a) as libc::socklen_t)
            }
            net::SocketAddr::V6(ref a) => {
                (a as *const _ as *const _, mem::size_of_val(a) as libc::socklen_t)
            }
        };

        opcode::Connect::new(
            types::Fd(self.socket.as_raw_fd()),
            addr,
            size,
        ).build()
    }
}

// Read from a TCP socket.
pub struct Read {
    pub socket: net::TcpStream,         // read data from this file descriptor
    pub buffer: Box<[u8]>, // buffer that will contain the data
    pub range: std::ops::Range<usize>,
}

impl Read {
    // TODO take start/end as range
    pub fn new(socket: net::TcpStream, buffer: Box<[u8]>) -> Self {
        let range = 0..buffer.len();
        Self{
            socket,
            buffer,
            range,
        }
    }
}

impl Task for Read {
    fn entry(&mut self) -> Entry {
        let buffer = &mut self.buffer[self.range.start..self.range.end];

        opcode::Read::new(
            types::Fd(self.socket.as_raw_fd()),
            buffer.as_mut_ptr(),
            buffer.len() as _,
        ).build()
    }
}

// Write to a TCP socket.
pub struct Write {
    pub socket: net::TcpStream,         // write data to this file descriptor
    pub buffer: Box<[u8]>, // buffer that contains the data
    pub range: std::ops::Range<usize>,
}

impl Write {
    pub fn new(socket: net::TcpStream, buffer: Box<[u8]>, range: std::ops::Range<usize>) -> Self {
        Self{
            socket,
            buffer,
            range,
        }
    }
}

impl Task for Write {
    fn entry(&mut self) -> Entry {
        let buffer = &mut self.buffer[self.range.start..self.range.end];
        opcode::Write::new(types::Fd(self.socket.as_raw_fd()), buffer.as_mut_ptr(), buffer.len() as _).build()
    }
}

#[enum_dispatch]
pub trait Task {
    fn entry(&mut self) -> Entry;
}

#[enum_dispatch(Task)]
pub enum TaskType {
    Accept,
    Connect,
    Read,
    Write,
}
