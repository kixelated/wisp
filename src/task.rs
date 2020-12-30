use std::os::unix::io::RawFd;
//use std::net::SocketAddr;

#[derive(Debug)]
pub enum Task {
    // Accept a TCP connection.
    Accept {
        fd: RawFd,
    },

    // Close a socket.
    Close {
        fd: RawFd,
    },

    // Dial a TCP connection to the given address.
    /*
    Connect {
        fd: RawFd, 
        addr: SocketAddr,
    }
    */

    // Read from a TCP stream.
    Read {
        fd: RawFd, // read data from this file descriptor
        buffer: Box<[u8]>, // buffer that will contain the data
    },

    // Write to a TCP stream.
    Write {
        fd: RawFd, // write data to this file descriptor
        buffer: Box<[u8]>, // buffer that contains the data
    },
}
