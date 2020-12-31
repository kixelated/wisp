use std::net;

#[derive(Debug)]
pub enum Task {
    // Accept a TCP connection.
    Accept {
        socket: net::TcpListener,
    },

    // Close a socket.
    Close {
        socket: net::TcpStream,
    },

    // Dial a TCP connection to the given address.
    Connect {
        socket: net::TcpStream,
        //addr: SockAddr,
        addr: net::SocketAddr,
    },

    // Read from a TCP socket.
    Read {
        socket: net::TcpStream,         // read data from this file descriptor
        buffer: Box<[u8]>, // buffer that will contain the data
    },

    // Write to a TCP socket.
    Write {
        socket: net::TcpStream,         // write data to this file descriptor
        buffer: Box<[u8]>, // buffer that contains the data
        offset: usize,
        size: usize,
    },
}
