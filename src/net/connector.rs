use std::{io};
use {PollOpt, Ready, Token, TokenType, TokenEntry};
use net::{EventLoop, EventLoopBuilder, TcpStream};
use std::sync::{Arc, Mutex};
use event_imp::ready_from_usize;
use log::{debug, info, warn};
use timer::{Timeout};
use std::time::Duration;

pub type ConnJob = Arc<Mutex<dyn FnMut(TcpStream)->bool + 'static + Send + Sync>>;

pub struct Connector {
    //addr: String,
    timer: Option<Timeout>,
    tcp_stream: Option<TcpStream>,
    event_loop: Arc<EventLoop>,
    on_conn_job: ConnJob,
}

macro_rules! enclose {
    ( ($( $x:ident ),*) $y:expr ) => {
        {
            $(let $x = $x.clone();)*
            $y
        }
    };
}

impl Connector {
    pub fn new(event_loop: Arc<EventLoop>) -> Connector {
        Connector {
            timer: None,
            tcp_stream: None,
            event_loop,
            on_conn_job: Arc::new(Mutex::new(move |_| { true })),
        }
    }

    pub fn set_conn_job(&mut self, job: ConnJob) {
        self.on_conn_job = job;
    }

    fn handle_on_connect(&mut self) -> io::Result<()> {
        let mut cb = self.on_conn_job.lock().unwrap();

        let stream = match self.tcp_stream.take() {
            Some(stream) => stream,
            None => {
                debug!("stream is None");
                assert_eq!(0, 1);
                return Ok(());
            },
        };

        let res = cb(stream);
        if res == false {
        }

        Ok(())
    }

    pub fn handle_event(&mut self, event: i64) -> io::Result<()> {
        let ready = ready_from_usize(event as usize);
        debug!("connect stream: {:?}, ready: {:?}",
                self.tcp_stream.as_mut().unwrap(), ready);

        match &self.timer {
            Some(timer) => {
                let event_loop = EventLoopBuilder::get_current_loop();
                event_loop.clear_timeout(&timer);
                debug!("timeout cancel")
            },
            _ => {},
        }

        if ready.is_writable() {
            self.handle_on_connect().unwrap();
        }

        if ready.is_error() ||
                ready.is_hup() {
        }
        Ok(())
    }

    pub fn handle_close(&mut self) -> io::Result<()> {
        match self.tcp_stream.as_mut() {
            Some(stream) => {
                self.event_loop.deregister(stream).unwrap();
            },
            None => {
            },
        }
        Ok(())
    }

    pub fn connect(this: Arc<Mutex<Self>>, addr: &String, timeout_ms: usize) {
        let stream = TcpStream::connect(&(addr.parse().unwrap())).unwrap();
        debug!("connect stream: {:?}", stream);

        this.lock().unwrap().tcp_stream = Some(stream);
        let job = Arc::new(Mutex::new(
            enclose! {
                (this)
                move |val: i64| {
                    this.lock().unwrap().handle_event(val).unwrap();
                }
            }
        ));

        let event_loop = this.lock().unwrap().event_loop.clone();

        if timeout_ms != usize::MAX {
            let this = this.clone();
            let timer = event_loop.timeout(
                Duration::from_millis(timeout_ms as u64),
                Box::new(
                    enclose! {
                        (this)
                        move || {
                            info!("connect timeout");
                            let event_loop = this.lock().unwrap().event_loop.clone();
                            event_loop.clear_timeout(this.lock().unwrap().timer.as_ref().unwrap());
                            event_loop.deregister(this.lock().unwrap().tcp_stream.as_ref().unwrap()).unwrap();
                        }
                    }
                )
            );
            match timer {
                Ok(timer) => this.lock().unwrap().timer = Some(timer),
                _ => {
                    warn!("connect set timeout failed");
                    return;
                },
            }
        }

        event_loop.register(this.lock().unwrap().tcp_stream.as_ref().unwrap(),
                TokenEntry {
                    ttype: TokenType::SocketEvent,
                    token: Token(9999999),
                },
                Ready::writable() | Ready::readable(),
                PollOpt::edge() | PollOpt::oneshot(),
                job
        ).unwrap();
    }
}

