use std::net;
use std::rc::Rc;

pub struct Reader {
    inner: Rc<net::TcpStream>,
}

impl std::ops::Deref for Reader {
    type Target = net::TcpStream;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::ops::Drop for Reader {
    fn drop(&mut self) {
        let _ = self.shutdown(net::Shutdown::Read);
    }
}

pub struct Writer {
    inner: Rc<net::TcpStream>,
}

impl std::ops::Deref for Writer {
    type Target = net::TcpStream;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::ops::Drop for Writer {
    fn drop(&mut self) {
        let _ = self.shutdown(net::Shutdown::Write);
    }
}

pub fn split(stream: net::TcpStream) -> (Reader, Writer) {
    let inner = Rc::new(stream);
    let reader = Reader {
        inner: Rc::clone(&inner),
    };
    let writer = Writer { inner };
    (reader, writer)
}
