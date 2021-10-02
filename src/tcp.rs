use std::{
    cell::RefCell,
    io::{Read, Write},
    net::ToSocketAddrs,
    os::unix::prelude::AsRawFd,
    rc::{Rc, Weak},
    task::Poll,
};

use futures::Stream;
use socket2::{Domain, Protocol, Socket, Type};
use tokio::io::{AsyncRead, AsyncWrite};

use crate::{executor::get_reactor, reactor::Reactor};

pub struct TcpListener {
    reactor: Weak<RefCell<Reactor>>,
    listener: std::net::TcpListener,
}

impl TcpListener {
    pub fn bind<A: ToSocketAddrs>(addr: A) -> std::io::Result<Self> {
        let addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "empty address"))?;

        let domain = if addr.is_ipv6() {
            Domain::IPV6
        } else {
            Domain::IPV4
        };
        let sk = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
        let addr = socket2::SockAddr::from(addr);
        sk.set_reuse_address(true)?;
        sk.bind(&addr)?;
        sk.listen(1024)?;

        // add fd to reactor
        let reactor = get_reactor();
        reactor.borrow_mut().add(sk.as_raw_fd());
        println!("tcp bind with fd {}", sk.as_raw_fd());
        Ok(Self {
            reactor: Rc::downgrade(&reactor),
            listener: sk.into(),
        })
    }
}

impl Stream for TcpListener {
    type Item = std::io::Result<TcpStream>;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        match self.listener.accept() {
            Ok((stream, _)) => {
                println!("[TcpStream] poll_next ok");
                Poll::Ready(Some(Ok(stream.into())))
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                println!("[TcpStream] poll_next get WouldBlock");
                let reactor = self.reactor.upgrade().unwrap();
                reactor
                    .borrow_mut()
                    .modify_readable(self.listener.as_raw_fd(), cx);
                println!("[TcpStream] poll_next modify_readable");
                Poll::Pending
            }
            Err(e) => {
                println!("[TcpStream] poll_next get error {}", e);
                Poll::Ready(Some(Err(e)))
            }
        }
    }
}

#[derive(Debug)]
pub struct TcpStream {
    inner: std::net::TcpStream,
}

impl From<std::net::TcpStream> for TcpStream {
    fn from(stream: std::net::TcpStream) -> Self {
        let reactor = get_reactor();
        reactor.borrow_mut().add(stream.as_raw_fd());
        Self { inner: stream }
    }
}

impl Drop for TcpStream {
    fn drop(&mut self) {
        let reactor = get_reactor();
        reactor.borrow_mut().delete(self.inner.as_raw_fd());
        println!("[TcpStream drop] remove fd {}", self.inner.as_raw_fd());
    }
}

impl AsyncRead for TcpStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let fd = self.inner.as_raw_fd();
        println!("[TcpStream poll_read] from fd {}", fd);
        unsafe {
            let b = &mut *(buf.unfilled_mut() as *mut [std::mem::MaybeUninit<u8>] as *mut [u8]);
            match self.inner.read(b) {
                Ok(n) => {
                    println!("read for fd {} done, {}", fd, n);
                    buf.assume_init(n);
                    buf.advance(n);
                    Poll::Ready(Ok(()))
                },
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    println!("[TcpStream] AsyncRead meet error WouldBlock");
                    let reactor = get_reactor();
                    reactor
                        .borrow_mut()
                        .modify_readable(self.inner.as_raw_fd(), cx);
                    Poll::Pending
                }
                Err(e) => Poll::Ready(Err(e)),
            }
        }
    }
}

impl AsyncWrite for TcpStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, std::io::Error>> {
        match self.inner.write(buf) {
            Ok(size) => Poll::Ready(Ok(size)),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                println!("[TcpStream] poll_write meet error WouldBlock");
                let reactor = get_reactor();
                reactor
                    .borrow_mut()
                    .modify_writable(self.inner.as_raw_fd(), cx);
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), std::io::Error>> {
        Poll::Ready(Ok(()))
    }
}
