use std::io::Result as IoResult;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};

pub struct RefinedTcpStream {
    stream: Stream,
    close_read: bool,
    close_write: bool,
}

pub enum Stream {
    Http(TcpStream),
}

impl From<TcpStream> for Stream {
    #[inline]
    fn from(stream: TcpStream) -> Stream {
        Stream::Http(stream)
    }
}

impl RefinedTcpStream {
    pub fn new<S>(stream: S) -> (RefinedTcpStream, RefinedTcpStream)
    where
        S: Into<Stream>,
    {
        let stream = stream.into();
        let read = match stream {
            Stream::Http(ref stream) => Stream::Http(stream.try_clone().unwrap()),
        };

        let read = RefinedTcpStream {
            stream: read,
            close_read: true,
            close_write: false,
        };

        let write = RefinedTcpStream {
            stream: stream,
            close_read: false,
            close_write: true,
        };

        (read, write)
    }

    #[inline]
    pub fn secure(&self) -> bool {
        match self.stream {
            Stream::Http(_) => false,
        }
    }

    pub fn peer_addr(&mut self) -> IoResult<SocketAddr> {
        match self.stream {
            Stream::Http(ref mut stream) => stream.peer_addr(),
        }
    }
}

impl Read for RefinedTcpStream {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        match self.stream {
            Stream::Http(ref mut stream) => stream.read(buf),
        }
    }
}

impl Write for RefinedTcpStream {
    fn write(&mut self, buf: &[u8]) -> IoResult<usize> {
        match self.stream {
            Stream::Http(ref mut stream) => stream.write(buf),
        }
    }
    fn flush(&mut self) -> IoResult<()> {
        match self.stream {
            Stream::Http(ref mut stream) => stream.flush(),
        }
    }
}

impl Drop for RefinedTcpStream {
    fn drop(&mut self) {
        if self.close_read {
            // self.stream.shutdown(Shutdown::Read).ok();
            match self.stream {
                Stream::Http(ref mut stream) => {
                    stream.shutdown(Shutdown::Read).ok();
                }
            }
        }
        if self.close_write {
            match self.stream {
                Stream::Http(ref mut stream) => {
                    stream.shutdown(Shutdown::Read).ok();
                }
            }
        }
    }
}
