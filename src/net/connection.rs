use std::{io};
use net::{TryRead, TryWrite};
use bytes::{BufMut, BytesMut};
use event_imp::{ready_from_usize};
use net::{EventLoop, TcpStream};
use std::sync::{Arc};
use std::net::Shutdown;
use log::{debug, warn};

unsafe impl Send for TcpConnection {}
unsafe impl Sync for TcpConnection {}

pub type ReadJob = Box<dyn FnMut(&mut BytesMut) + 'static + Send + Sync>;
pub type WritJob = Box<dyn FnMut()->bool + 'static + Send + Sync>;
pub type CloseJob = Box<dyn FnMut() + 'static + Send + Sync>;

pub struct TcpConnection {
    event_loop: Arc<EventLoop>,
    pub tcp_stream: TcpStream,
    pub read_buffer: Option<BytesMut>,
    write_buffer_sending: Option<BytesMut>,
    write_buffer_waiting: Option<BytesMut>,

    read_job: ReadJob,
    writ_job: WritJob,
    close_job: CloseJob,

    read_enabled: bool,
    write_enabled: bool,
    read_triggered: bool,
    write_triggered: bool,
    closed: bool,
}

impl TcpConnection {
    pub fn new(event_loop: Arc<EventLoop>, tcp_stream: TcpStream)
            -> TcpConnection {
        TcpConnection {
            event_loop: event_loop,
            tcp_stream: tcp_stream,

            read_buffer: Some(BytesMut::with_capacity(1024)),
            write_buffer_sending: Some(BytesMut::with_capacity(1024)),
            write_buffer_waiting: Some(BytesMut::with_capacity(1024)),

            read_job: Box::new(move |_| { debug!("default read job"); }),
            writ_job: Box::new(move || { debug!("default write job"); true }),
            close_job: Box::new(move || { debug!("default close job"); }),

            read_enabled: true,
            write_enabled: true,
            read_triggered: false,
            write_triggered: false,
            closed: false,
        }
    }

    pub fn connected(&self) -> bool {
        self.read_triggered ||
                self.write_triggered
    }

    pub fn set_read_job(&mut self, job: ReadJob) {
        self.read_job = job;
    }

    pub fn set_writ_job(&mut self, job: WritJob) {
        self.writ_job = job;
    }

    pub fn set_close_job(&mut self, job: CloseJob) {
        self.close_job = job;
    }

    pub fn set_enabled(&mut self, enable: bool) {
        self.read_enabled = enable;
        self.write_enabled = enable;
    }

    fn handle_read(&mut self) -> io::Result<()> {
        loop {
            let mut buf = self.read_buffer.as_mut().unwrap();
            match self.tcp_stream.try_read_buf(&mut buf) {
                Ok(None) => {
                    debug!("Conn: spurious read wakeup");
                    self.read_triggered = false; 
                    break;
                }
                Ok(Some(r)) => {
                    debug!("Conn: read {} bytes, {:?}", r, buf);
                    if r > 0 {
                        (self.read_job)(&mut buf);
                    } else {
                        (self.read_job)(&mut buf);
                        self.read_triggered = false; 
                        self.handle_close().unwrap();
                        break;
                    }
                }
                Err(e) => {
                    warn!("not implemented client err: {:?}", e);
                    self.read_triggered = false; 
                    self.handle_close().unwrap();
                    break;
                }
            };
        }
        Ok(())
    }

    fn write_data(&mut self) -> io::Result<()> {
        let mut buf_tmp = Some(BytesMut::with_capacity(1024)).unwrap();
        let buf_snd = self.write_buffer_sending.as_mut().unwrap();
        if buf_snd.len() > 0 {
            buf_tmp = buf_snd.split();
        }
        if buf_tmp.len() == 0 {
            loop {
                let buf = self.write_buffer_waiting.as_mut().unwrap();
                if buf.len() > 0 {
                    buf_tmp = buf.split();
                    break;
                }
                return Ok(())
            }
        }

        let mut buf = buf_tmp;
        debug!("Conn {:?}: write1 ", self.tcp_stream);
        match self.tcp_stream.try_write_buf(&mut buf) {
            Ok(None) => {
                debug!("client flushing buf; WouldBlock");
                self.write_triggered = false;
            }
            Ok(Some(_r)) => {
                debug!("Conn: write2 {} bytes", _r);
                if buf.len() > 0 {
                    return Ok(());
                }
                self.write_triggered = false;
                let ret = (self.writ_job)();
                if ret == false {
                    debug!("close stream1");
                    self.handle_close().unwrap();
                }
            }
            Err(e) => {
                debug!("not implemented; client err: {:?}", e);
                self.write_triggered = false;
                self.handle_close().unwrap();
            }
        }

        Ok(())
    }

    pub fn send(&mut self, bytes: BytesMut) -> io::Result<usize> {
        if self.write_enabled == false {
            return Ok(0);
        }
        let len = bytes.len();
        if len == 0 {
            return Ok(0);
        }
        let buffer = self.write_buffer_waiting.as_mut().unwrap();
        buffer.put(bytes);
        self.write_data().unwrap();
        return Ok(len);
    }

    fn handle_write(&mut self) -> io::Result<()> {
        let mut empty_waiting: bool = false;
        let mut empty_sending: bool = false;
        if self.write_buffer_waiting.as_ref().unwrap().len() == 0 {
            empty_waiting = true;
        }
        if self.write_buffer_sending.as_ref().unwrap().len() == 0 {
            empty_sending = true;
        }
        if empty_waiting && empty_sending {
            debug!("handle_write disable write TODO");
        } else {
            self.write_data().unwrap();
        }
        return Ok(())
    }

    fn handle_close(&mut self) -> io::Result<()> {
        if self.closed {
            return Ok(())
        }
        self.set_enabled(false);
        self.closed = true;
        
        (self.close_job)();
        self.close_stream();
        Ok(())
    }

    pub fn handle_event(&mut self, event: i64) -> io::Result<()> {
        let ready = ready_from_usize(event as usize);
        debug!("stream: {:?}, handle_event ready: {:?}", self.tcp_stream, ready);
        if ready.is_readable() {
            self.read_triggered = true; 
            self.handle_read().unwrap();
        }
        if ready.is_writable() {
            self.write_triggered = true; 
           self.handle_write().unwrap();
        }
        if ready.is_error() ||
                ready.is_hup() {
            self.read_triggered = true; 
            self.write_triggered = true; 
            self.handle_close().unwrap();
        }
        Ok(())
    }

    pub fn shutdown(&mut self, how: Shutdown) -> io::Result<()> {
        if how == Shutdown::Read {
            self.read_enabled = false;
        } else if how == Shutdown::Write {
            self.write_enabled = false;
        } else {
            self.set_enabled(false);
        }
        self.tcp_stream.shutdown(how)
    }

    pub fn close_stream(&mut self) {
        self.event_loop.deregister(&self.tcp_stream).unwrap();
        debug!("close_stream {:?} deregister done", self.tcp_stream);
    }
}

impl Drop for TcpConnection {
    fn drop(&mut self) {
        debug!("drop for tcpconnection {:?}", self.tcp_stream)
    }
}

