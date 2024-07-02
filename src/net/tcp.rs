use std::fmt;
use std::io::{self, Read, Write};
use std::net::{self, SocketAddr, SocketAddrV4, SocketAddrV6, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use net2::TcpBuilder;
use iovec::IoVec;

use {sys, Ready, Poll, PollOpt, Job, TokenEntry};
use event::Evented;
use poll::SelectorId;

pub struct TcpStream {
    sys: sys::TcpStream,
    selector_id: SelectorId,
}

use std::net::Shutdown;

fn set_nonblocking(stream: &net::TcpStream) -> io::Result<()> {
    sys::set_nonblock(
        ::std::os::unix::io::AsRawFd::as_raw_fd(stream)
    )
}

impl TcpStream {
    pub fn connect(addr: &SocketAddr) -> io::Result<TcpStream> {
        let sock = match *addr {
            SocketAddr::V4(..) => TcpBuilder::new_v4(),
            SocketAddr::V6(..) => TcpBuilder::new_v6(),
        }?;
        TcpStream::connect_stream(sock.to_tcp_stream()?, addr)
    }
    pub fn connect_stream(stream: net::TcpStream, 
            addr: &SocketAddr) -> io::Result<TcpStream> {
        set_nonblocking(&stream)?;
        Ok(TcpStream {
            sys: sys::TcpStream::connect(stream, addr)?,
            selector_id: SelectorId::new(),
        })
    }
    pub fn from_stream(stream: net::TcpStream) -> io::Result<TcpStream> {
        set_nonblocking(&stream)?;
        Ok(TcpStream {
            sys: sys::TcpStream::from_stream(stream),
            selector_id: SelectorId::new(),
        })
    }
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.sys.peer_addr()
    }
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.sys.local_addr()
    }
    pub fn try_clone(&self) -> io::Result<TcpStream> {
        self.sys.try_clone().map(|s| {
            TcpStream {
                sys: s,
                selector_id: self.selector_id.clone(),
            }
        })
    }
    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        self.sys.shutdown(how)
    }
    pub fn set_nodelay(&self, nodelay: bool) -> io::Result<()> {
        self.sys.set_nodelay(nodelay)
    }
    pub fn nodelay(&self) -> io::Result<bool> {
        self.sys.nodelay()
    }
    pub fn set_recv_buffer_size(&self, size: usize) -> io::Result<()> {
        self.sys.set_recv_buffer_size(size)
    }
    pub fn recv_buffer_size(&self) -> io::Result<usize> {
        self.sys.recv_buffer_size()
    }
    pub fn set_send_buffer_size(&self, size: usize) -> io::Result<()> {
        self.sys.set_send_buffer_size(size)
    }
    pub fn send_buffer_size(&self) -> io::Result<usize> {
        self.sys.send_buffer_size()
    }
    pub fn set_keepalive(&self, keepalive: Option<Duration>) -> io::Result<()> {
        self.sys.set_keepalive(keepalive)
    }
    pub fn keepalive(&self) -> io::Result<Option<Duration>> {
        self.sys.keepalive()
    }
    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.sys.set_ttl(ttl)
    }
    pub fn ttl(&self) -> io::Result<u32> {
        self.sys.ttl()
    }
    /*
    pub fn set_only_v6(&self, only_v6: bool) -> io::Result<()> {
        self.sys.set_only_v6(only_v6)
    }
    pub fn only_v6(&self) -> io::Result<bool> {
        self.sys.only_v6()
    }
    pub fn set_linger(&self, dur: Option<Duration>) -> io::Result<()> {
        self.sys.set_linger(dur)
    }
    pub fn linger(&self) -> io::Result<Option<Duration>> {
        self.sys.linger()
    }
    */
    pub fn set_keepalive_ms(&self, keepalive: Option<u32>) -> io::Result<()> {
        self.set_keepalive(keepalive.map(|v| {
            Duration::from_millis(u64::from(v))
        }))
    }
    pub fn keepalive_ms(&self) -> io::Result<Option<u32>> {
        self.keepalive().map(|v| {
            v.map(|v| {
                ::convert::millis(v) as u32
            })
        })
    }
    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.sys.take_error()
    }
    pub fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.sys.peek(buf)
    }
    pub fn read_bufs(&self, bufs: &mut [&mut IoVec]) -> io::Result<usize> {
        self.sys.readv(bufs)
    }
    pub fn write_bufs(&self, bufs: &[&IoVec]) -> io::Result<usize> {
        self.sys.writev(bufs)
    }
}

impl fmt::Debug for TcpStream {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(&self.sys, f)
    }
}

#[allow(dead_code)]
fn inaddr_any(other: &SocketAddr) -> SocketAddr {
    match *other {
        SocketAddr::V4(..) => {
            let any = Ipv4Addr::new(0, 0, 0, 0);
            let addr = SocketAddrV4::new(any, 0);
            SocketAddr::V4(addr)
        }
        SocketAddr::V6(..) => {
            let any = Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0);
            let addr = SocketAddrV6::new(any, 0, 0, 0);
            SocketAddr::V6(addr)
        }
    }
}

impl Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&self.sys).read(buf)
    }
}

impl<'a> Read for &'a TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&self.sys).read(buf)
    }
}

impl Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&self.sys).write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        (&self.sys).flush()
    }
}

impl<'a> Write for &'a TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&self.sys).write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        (&self.sys).flush()
    }
}

impl Evented for TcpStream {
    fn register(&self, poll: &Poll, token: TokenEntry, interest: Ready, opts: PollOpt, job: Job) -> io::Result<()> {
        self.selector_id.associate_selector(poll)?;
        self.sys.register(poll, token, interest, opts, job)
    }
    fn reregister(&self, poll: &Poll, token: TokenEntry, interest: Ready, opts: PollOpt) -> io::Result<()> {
        self.sys.reregister(poll, token, interest, opts)
    }
    fn deregister(&self, poll: &Poll) -> io::Result<()> {
        self.sys.deregister(poll)
    }
}

pub struct TcpListener {
    sys: sys::TcpListener,
    selector_id: SelectorId,
}

impl TcpListener {
    pub fn new(addr: &SocketAddr) -> TcpListener {
        Self::bind(addr).unwrap()
    }
    pub fn bind(addr: &SocketAddr) -> io::Result<TcpListener> {
        use net2::unix::UnixTcpBuilderExt;
        let sock = match *addr {
            SocketAddr::V4(..) => TcpBuilder::new_v4(),
            SocketAddr::V6(..) => TcpBuilder::new_v6(),
        }?;
        sock.reuse_address(true)?;
        sock.reuse_port(true)?;
        sock.bind(addr)?;
        let listener = sock.listen(1024)?;
        Ok(TcpListener {
            sys: sys::TcpListener::new(listener)?,
            selector_id: SelectorId::new(),
        })
    }
    pub fn from_listener(listener: net::TcpListener, _: &SocketAddr)
            -> io::Result<TcpListener> {
        TcpListener::from_std(listener)
    }
    pub fn from_std(listener: net::TcpListener) -> io::Result<TcpListener> {
        sys::TcpListener::new(listener).map(|s| {
            TcpListener {
                sys: s,
                selector_id: SelectorId::new(),
            }
        })
    }
    pub fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        let (s, a) = self.accept_std()?;
        Ok((TcpStream::from_stream(s)?, a))
    }
    pub fn accept_std(&self) -> io::Result<(net::TcpStream, SocketAddr)> {
        self.sys.accept()
    }
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.sys.local_addr()
    }
    pub fn try_clone(&self) -> io::Result<TcpListener> {
        self.sys.try_clone().map(|s| {
            TcpListener {
                sys: s,
                selector_id: self.selector_id.clone(),
            }
        })
    }
    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.sys.set_ttl(ttl)
    }
    pub fn ttl(&self) -> io::Result<u32> {
        self.sys.ttl()
    }
    /*
    pub fn set_only_v6(&self, only_v6: bool) -> io::Result<()> {
        self.sys.set_only_v6(only_v6)
    }
    pub fn only_v6(&self) -> io::Result<bool> {
        self.sys.only_v6()
    }
    */
    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.sys.take_error()
    }
}

impl Evented for TcpListener {
    fn register(&self, poll: &Poll, token: TokenEntry, interest: Ready, opts: PollOpt, job: Job) -> io::Result<()> {
        self.selector_id.associate_selector(poll)?;
        self.sys.register(poll, token, interest, opts, job)
    }
    fn reregister(&self, poll: &Poll, token: TokenEntry, interest: Ready, opts: PollOpt) -> io::Result<()> {
        self.sys.reregister(poll, token, interest, opts)
    }
    fn deregister(&self, poll: &Poll) -> io::Result<()> {
        self.sys.deregister(poll)
    }
}

use std::os::unix::io::{IntoRawFd, AsRawFd, FromRawFd, RawFd};

impl IntoRawFd for TcpStream {
    fn into_raw_fd(self) -> RawFd {
        self.sys.into_raw_fd()
    }
}

impl AsRawFd for TcpStream {
    fn as_raw_fd(&self) -> RawFd {
        self.sys.as_raw_fd()
    }
}

impl FromRawFd for TcpStream {
    unsafe fn from_raw_fd(fd: RawFd) -> TcpStream {
        TcpStream {
            sys: FromRawFd::from_raw_fd(fd),
            selector_id: SelectorId::new(),
        }
    }
}

impl IntoRawFd for TcpListener {
    fn into_raw_fd(self) -> RawFd {
        self.sys.into_raw_fd()
    }
}

impl AsRawFd for TcpListener {
    fn as_raw_fd(&self) -> RawFd {
        self.sys.as_raw_fd()
    }
}

impl FromRawFd for TcpListener {
    unsafe fn from_raw_fd(fd: RawFd) -> TcpListener {
        TcpListener {
            sys: FromRawFd::from_raw_fd(fd),
            selector_id: SelectorId::new(),
        }
    }
}

