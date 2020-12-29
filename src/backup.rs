use std::net::TcpListener;
use std::mem::size_of;
use std::os::unix::io::{AsRawFd, RawFd};
use std::{io, ptr};
use std::io::Write;

use io_uring::opcode::{self, types};
use io_uring::squeue::{Flags};
use io_uring::IoUring;
use slab::Slab;

#[derive(Copy, Clone, Debug)]
enum Task {
    // Accept TCP connections from end users.
    Accept {
        fd: RawFd,
    },

    // Accept a HTTP request from an end user.
    Read {
        fd: RawFd,
        buffer: usize,
    },

    // Create a TCP connection to the backend.
    ConnectBackend,

    // Write a HTTP request to the backend.
    WriteBackend {
        fd_backend: RawFd,
        fd_user: RawFd,
        buf_index: usize,
        offset: usize,
        len: usize,
    },

    // Read the HTTP response from the backend.
    ReadBackend {
        fd_backend: RawFd,
        fd_user: RawFd,
        buf_index: usize,
    },

    // Write the HTTP response to the end user.
    WriteUser {
        fd: RawFd,
        buf_index: usize,
        offset: usize,
        len: usize,
    },
}

fn main() -> anyhow::Result<()> {
    let mut ring = IoUring::new(256)?;
    let listener = TcpListener::bind(("127.0.0.1", 8080))?;

    let mut buf_pool = Vec::with_capacity(64);
    let mut conn_pool = Vec::with_capacity(64);
    let mut buf_alloc = Slab::with_capacity(64);
    let mut token_alloc = Slab::with_capacity(64);

    println!("listen {}", listener.local_addr()?);

    let (submitter, sq, cq) = ring.split();

    let accept_token = token_alloc.insert(Token::AcceptUser);
    let accept_op = opcode::Accept::new(types::Fd(listener.as_raw_fd()), ptr::null_mut(), ptr::null_mut())
        .build()
        .user_data(accept_token as _);

    unsafe { 
        match sq.available().push(accept_op) {
            Ok(_) => (),
            Err(_) => anyhow::bail!("failed to push initial accept"),
        };
    }

    loop {
        submitter.submit_and_wait(1)?;

        for cqe in cq.available() {
            let ret = cqe.result();
            let token_index = cqe.user_data() as usize;
            let token = token_alloc[token_index];

            if ret < 0 {
                anyhow::bail!("token {:?} error: {:?}", token, io::Error::from_raw_os_error(-ret));
            }

            let mut sq = sq.available();

            match token {
                Token::AcceptUser => {
                    let accept_op = opcode::Accept::new(types::Fd(listener.as_raw_fd()), ptr::null_mut(), ptr::null_mut())
                        .build()
                        .user_data(accept_token as _);

                    unsafe { 
                        match sq.push(accept_op) {
                            Ok(_) => (),
                            Err(_) => anyhow::bail!("failed to push next accept"),
                        };
                    }

                    let (buf_index, buf) = match buf_pool.pop() {
                        Some(buf_index) => (buf_index, &mut buf_alloc[buf_index]),
                        None => {
                            let buf = vec![0u8; 2048].into_boxed_slice();
                            let buf_entry = buf_alloc.vacant_entry();
                            let buf_index = buf_entry.key();
                            (buf_index, buf_entry.insert(buf))
                        }
                    };

                    let fd = ret;
                    let read_token = token_alloc.insert(Token::ReadUser { fd, buf_index });
                    let read_op = opcode::Read::new(types::Fd(fd), buf.as_mut_ptr(), buf.len() as _)
                        .build()
                        .user_data(read_token as _);

                    unsafe { 
                        match sq.push(read_op) {
                            Ok(_) => (),
                            Err(_) => anyhow::bail!("failed to push initial read"),
                        };
                    }
                },
                Token::ReadUser { fd_user, buf_index } => {
                    if ret == 0 {
                        buf_pool.push(buf_index);

                        println!("connection closed by peer");

                        unsafe {
                            libc::close(fd_user);
                        }
                    } else {
                        let len = ret as usize;
                        let buf = &mut buf_alloc[buf_index];

                        let fd_backend = unsafe {
                            libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, libc::IPPROTO_TCP);
                        };

                        if fd_backend == -1 {
                            anyhow::bail!("failed to create socket");
                        }

                        let sock_addr = libc::sockaddr_in{
                           sin_family: libc::AF_INET,
                           sin_port: 9001,
                           sin_addr: libc::INADDR_LOOPBACK,
                        };

                        let connect_token = token_alloc.insert(Token::ConnectBackend);

                        let write_token = token_index;

                        token_alloc[token_index] = Token::WriteBackend {
                            fd_user,
                            fd_backend,
                            buf_index,
                            len,
                            offset: 0,
                        };

                        let connect_op = opcode::Connect::new(types::Fd(fd_backend), sock_addr.as_ptr(), size_of(sock_addr) as _)
                            .build()
                            .flags(Flags::IO_LINK) // must succeed before next op
                            .user_data(connect_token as _);


                        let write_op = opcode::Write::new(types::Fd(upstream_fd), buf.as_ptr(), len as _)
                            .build()
                            .user_data(write_token as _);

                        unsafe { 
                            match sq.push(connect_op) {
                                Ok(_) => (),
                                Err(_) => anyhow::bail!("failed to push connect"),
                            };

                            match sq.push(write_op) {
                                Ok(_) => (),
                                Err(_) => anyhow::bail!("failed to push first write"),
                            };
                        }
                    }
                }
                Token::ConnectBackend => {
                    println!("connected to backend!");
                },
                Token::WriteBackend {
                    fd_user,
                    fd_backend,
                    buf_index,
                    offset,
                    len,
                } => {
                    let write_len = ret as usize;

                    if offset + write_len >= len {
                        let read_token = Token::ReadBackend {
                            fd_user,
                            fd_backend,
                            buf_index,
                        };

                        // HERE
                        let read_op = opcode::Read::new(types::Fd(fd), buf.as_mut_ptr(), buf.len() as _)
                            .build()
                            .user_data(read_token as _);

                        unsafe { 
                            match sq.push(read_op) {
                                Ok(_) => (),
                                Err(_) => anyhow::bail!("failed to push initial read"),
                            };
                        }
                    } else {
                        let offset = offset + write_len;
                        let len = len - offset;

                        let buf = &buf_alloc[buf_index][offset..];

                        token_alloc[token_index] = Token::WriteBackend {
                            fd_backend,
                            fd_user,
                            buf_index,
                            offset,
                            len,
                        };

                        let write_op = opcode::Write::new(types::Fd(fd_backend), buf.as_ptr(), len as _)
                            .build()
                            .user_data(token_index as _);

                        unsafe { 
                            match sq.push(write_op) {
                                Ok(_) => (),
                                Err(_) => anyhow::bail!("failed to push additional backend write"),
                            };
                        }
                    }
                },
                Token::WriteUser {
                    fd,
                    buf_index,
                    offset,
                    len,
                } => {
                    let write_len = ret as usize;

                    if offset + write_len >= len {
                        buf_pool.push(buf_index);
                        token_alloc.remove(token_index);

                        println!("connection closed after write");

                        unsafe {
                            libc::close(fd);
                        }
                    } else {
                        let offset = offset + write_len;
                        let len = len - offset;

                        let buf = &buf_alloc[buf_index][offset..];

                        token_alloc[token_index] = Token::WriteUser {
                            fd,
                            buf_index,
                            offset,
                            len,
                        };

                        let write_op = opcode::Write::new(types::Fd(fd), buf.as_ptr(), len as _)
                            .build()
                            .user_data(token_index as _);

                        unsafe { 
                            match sq.push(write_op) {
                                Ok(_) => (),
                                Err(_) => anyhow::bail!("failed to push additional write"),
                            };
                        }
                    }
                }
            }
        }
    }
}
