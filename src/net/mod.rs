//#![feature(mutex_unpoison)]
//#[warn(unused_attributes)]

mod tcp;
pub use self::tcp::{TcpListener, TcpStream};

mod event_loop;
pub use self::event_loop::{
    EventLoop,
    EventLoopBuilder,
    EventLoopPool,
    enqueue_job,
};

mod acceptor;
pub use self::acceptor::{
    Acceptor
};

mod connector;
pub use self::connector::{
    Connector
};

mod connection;
pub use self::connection::{
    TcpConnection
};

mod tcp_server;
pub use self::tcp_server::{
    Handler,
    TcpServer,
    ClientHandler,
    TcpClient,
};

use std::{io};
use bytes::{Buf, BufMut};
use std::io::{Read, Write};

trait MapNonBlock<T> {
    fn map_non_block(self) -> io::Result<Option<T>>;
}

impl<T> MapNonBlock<T> for io::Result<T> {
    fn map_non_block(self) -> io::Result<Option<T>> {
        use std::io::ErrorKind::WouldBlock;
        match self {
            Ok(value) => Ok(Some(value)),
            Err(err) => {
                if let WouldBlock = err.kind() {
                    Ok(None) 
                } else {
                    Err(err)
                }
            }
        }
    }
}

pub trait TryRead {
    fn try_read_buf<B: BufMut>(&mut self, buf: &mut B) -> io::Result<Option<usize>> 
            where Self : Sized {
        let bytes = buf.chunk_mut();
        let res = self.try_read(unsafe { 
            std::slice::from_raw_parts_mut(bytes.as_mut_ptr(), bytes.len())
        });

        if let Ok(Some(cnt)) = res {
            unsafe { buf.advance_mut(cnt); }
        }
        res 
    }
    fn try_read(&mut self, buf: &mut [u8]) -> io::Result<Option<usize>>;
}

pub trait TryWrite {
    fn try_write_buf<B: Buf>(&mut self, buf: &mut B) -> io::Result<Option<usize>> 
        where Self : Sized {
        let res = self.try_write(buf.chunk());
        if let Ok(Some(cnt)) = res {
            buf.advance(cnt);
        }
        res
    }
    fn try_write(&mut self, buf: &[u8]) -> io::Result<Option<usize>>;
}

impl<T: Read> TryRead for T {
    fn try_read(&mut self, dst: &mut [u8]) -> io::Result<Option<usize>> {
        self.read(dst).map_non_block()
    }
}

impl<T: Write> TryWrite for T {
    fn try_write(&mut self, src: &[u8]) -> io::Result<Option<usize>> {
        self.write(src).map_non_block()
    }
}

