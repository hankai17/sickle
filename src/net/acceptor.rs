use net::{EventLoop, TcpStream, TcpListener};
use std::net::{SocketAddr};
use std::sync::{Arc};
use {PollOpt, Ready, Token, Job, TokenType, TokenEntry};

pub type AcceptorJob = Box<dyn FnMut(TcpStream, SocketAddr) + 'static + Send + Sync>;

unsafe impl Send for Acceptor {}
unsafe impl Sync for Acceptor {}

pub struct Acceptor {
    tcp_listener: TcpListener,
    event_loop: Arc<EventLoop>,
    is_listening: bool,
    acceptor_job: AcceptorJob
}

impl Acceptor {
    pub fn new(event_loop: Arc<EventLoop>, addr: &String) -> Acceptor {
        Acceptor {
            tcp_listener: TcpListener::new(&(addr.parse().unwrap())),
            event_loop: event_loop,
            is_listening: false,
            acceptor_job: Box::new(move |_, _| { println!("default acceptor job"); })
        }
    }

    pub fn set_accept_job(&mut self, job: AcceptorJob) {
        self.acceptor_job = job;
    }

    pub fn handle_read(&mut self, _val: i64) {
        use std::io::ErrorKind::WouldBlock;
        use std::io::ErrorKind::Interrupted;
        loop {
            match self.tcp_listener.accept() {
                Ok((stream, addr)) => {
                    (self.acceptor_job)(stream, addr);
                }
                Err(ref e) if e.kind() == Interrupted => {
                    continue;
                }
                Err(ref e) if e.kind() == WouldBlock => {
                    break;
                }
                Err(_) => {
                    break;
                }
            }
        }
    }

    pub fn bind(&mut self, job: Job) {
        self.event_loop.register(
                &self.tcp_listener, 
                TokenEntry {
                    ttype: TokenType::AcceptEvent,
                    token: Token(0)
                }, 
                Ready::readable(), 
                PollOpt::edge(),
                job
        ).unwrap();
        self.is_listening = true;
    }
}

