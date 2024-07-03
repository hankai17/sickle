extern crate sickle;
extern crate bytes;
extern crate log;
extern crate env_logger;
extern crate chrono;

use bytes::{Buf, BytesMut};
use sickle::net::{EventLoop, EventLoopBuilder,  EventLoopPool, TcpConnection, TcpServer, Handler, enqueue_job};
use std::sync::{Arc, Mutex, Weak};
use std::thread;

use log::{debug, info, LevelFilter};
use std::io::Write;
use std::fs::File;
use chrono::Local;
use env_logger::{Builder};

use std::net::Shutdown;
use sickle::net::{TcpClient, ClientHandler};

struct ServerSession {
    client_conn: Option<Weak<Mutex<TcpConnection>>>,
    server_conn: Option<Weak<Mutex<TcpConnection>>>,
}

impl ServerSession {
    fn new(conn: Weak<Mutex<TcpConnection>>) -> ServerSession {
        ServerSession {
            client_conn: Some(conn),
            server_conn: None,
        }
    } 

    pub fn get_client_conn(&mut self) -> Option<Arc<Mutex<TcpConnection>>> {
        match self.client_conn.as_mut() {
            Some(conn) => conn.upgrade(),
            None => None,
        }
    }

    pub fn get_server_conn(&mut self) -> Option<Arc<Mutex<TcpConnection>>> {
        match self.server_conn.as_mut() {
            Some(conn) => conn.upgrade(),
            None => None,
        }
    }
}

impl Drop for ServerSession {
    fn drop(&mut self) {
        debug!("dropping for ServerSession")
    }
}

impl ClientHandler for ServerSession {
    fn shutdown(&mut self) {
    }

    fn attach_connection(&mut self, conn: Arc<Mutex<TcpConnection>>) {
        self.server_conn = Some(Arc::downgrade(&conn));
    }

    fn on_connect(&mut self, new_conn: Arc<Mutex<TcpConnection>>) {
		match new_conn.lock().unwrap().tcp_stream.take_error() {
            Ok(res) => {
                if let Some(res) = res {
                    debug!("on_connect failed: {:?}", res);
                    return;
                }
            },
            Err(_) => {},
        }
        let cs = match self.get_client_conn() {
            Some(conn) => conn.clone(),
            None => return,
        };

        let poller = EventLoopBuilder::get_current_loop();
        let job = Arc::new(Mutex::new(move|_| {
            let mut ss = new_conn.lock().unwrap();
            let mut cs = cs.lock().unwrap();
            let bytes = cs.read_buffer.as_mut().unwrap();
            let len = bytes.len();
            if len > 0 {
                ss.send(bytes.clone()).unwrap();
                bytes.advance(len);
            }
        }));
        enqueue_job(poller.clone(), job);
    }

    fn on_recv(&mut self, bytes: &mut BytesMut) {
        info!("on_recv: {:?}", bytes);
        let cs = match self.get_client_conn() {
            Some(conn) => conn.clone(),
            None => return,
        };
        if bytes.len() > 0 {
            cs.lock().unwrap().send(bytes.clone()).unwrap();
            bytes.advance(bytes.len());
        }
        // job?
    }

    fn on_written(&mut self) -> bool {
        debug!("write done");
        true
    }

    fn on_error(&mut self) {
        info!("connect error");
        let cs = match self.get_client_conn() {
            Some(conn) => conn.clone(),
            None => return,
        };
        debug!("close client session");

        let poller = EventLoopBuilder::get_current_loop();
        let job = Arc::new(Mutex::new(move|_| {
            cs.lock().unwrap().shutdown(Shutdown::Write).unwrap();
        }));
        enqueue_job(poller.clone(), job);
    }
}

struct Tunnel {
    addr: String,
    client: TcpClient,
    pub server_session: Option<Weak<Mutex<ServerSession>>>,
    pub client_conn: Option<Weak<Mutex<TcpConnection>>>,
}

impl Tunnel {
    pub fn new(event_loop: Arc<EventLoop>, addr: &String,
            client_conn: Weak<Mutex<TcpConnection>>) -> Tunnel {
        Tunnel {
            addr: String::from(addr),
            client: TcpClient::new(event_loop),
            server_session: None,
            client_conn: Some(client_conn),
        }
    }

    pub fn connect(&mut self) {
        let cs = match self.get_client_conn() {
            Some(conn) => conn.clone(),
            None => return,
        };
        let session = Arc::new(Mutex::new(ServerSession::new(
            Arc::downgrade(&cs)
        )));
        self.client.start_connect(&self.addr.to_string(), session.clone());
        self.server_session = Some(Arc::downgrade(&session));
    }

    pub fn get_client_conn(&mut self) -> Option<Arc<Mutex<TcpConnection>>> {
        match self.client_conn.as_mut() {
            Some(conn) => conn.upgrade(),
            None => None,
        }
    }

    pub fn get_server_session(&mut self) -> Option<Arc<Mutex<ServerSession>>> {
        match self.server_session.as_mut() {
            Some(session) => session.upgrade(),
            None => None,
        }
    }

    pub fn get_server_conn(&mut self) -> Option<Arc<Mutex<TcpConnection>>> {
        match self.get_server_session() {
            Some(session) => {
                session.lock().unwrap().get_server_conn()
            },
            None => None,
        }
    }
}

struct TunnelServer {
    client_conn: Option<Weak<Mutex<TcpConnection>>>,
    tunnel: Option<Arc<Mutex<Tunnel>>>
}

impl TunnelServer {
    pub fn get_client_conn(&mut self) -> Option<Arc<Mutex<TcpConnection>>> {
        match &self.client_conn {
            Some(conn) => conn.upgrade(),
            None => None,
        }
    }

    pub fn get_server_conn(&mut self) -> Option<Arc<Mutex<TcpConnection>>> {
        match &self.tunnel {
            Some(tunnel) => {
                tunnel.lock().unwrap().get_server_conn()
            },
            None => None,
        }
    }
}

impl Drop for TunnelServer {
    fn drop(&mut self) {
    }
}

impl Handler for TunnelServer {
    fn new() -> TunnelServer {
        TunnelServer {
            client_conn: None,
            tunnel: None,
        }
    }

    fn attach_connection(&mut self, conn: Arc<Mutex<TcpConnection>>) {
        self.client_conn = Some(Arc::downgrade(&conn));
    }

    fn free_connection(&mut self) {
        self.client_conn = None;
    }

    fn on_accept(&mut self) {
        let cc = match self.get_client_conn() {
            Some(conn) => conn.clone(),
            None => return,
        };
        let event_loop = EventLoopBuilder::get_current_loop();
        let tunnel = Arc::new(Mutex::new(
            Tunnel::new(
                event_loop.clone(),
                &"127.0.0.1:80".to_string(),
                Arc::downgrade(&cc))
        ));
        tunnel.lock().unwrap().connect();
        self.tunnel = Some(tunnel.clone());
    }

    fn on_recv(&mut self, bytes: &mut BytesMut) {
        if bytes.len() <= 0 {
            return;
        }
        let ss = match self.get_server_conn() {
            Some(conn) => conn.clone(),
            None => return,
        };
        let poller = EventLoopBuilder::get_current_loop();
        let bytes_clone = bytes.clone();
        let job = Arc::new(Mutex::new(move|_| {
            ss.lock().unwrap().send(bytes_clone.clone()).unwrap();
        }));
        bytes.advance(bytes.len());
        enqueue_job(poller.clone(), job);
    }

    fn on_written(&mut self) -> bool {
        true
    }

    fn on_error(&mut self) {
        self.free_connection();
        debug!("TunnelServer on_error");
        let ss = match self.get_server_conn() {
            Some(conn) => conn.clone(),
            None => return,
        };

        let poller = EventLoopBuilder::get_current_loop();
        let job = Arc::new(Mutex::new(move|_| {
            let _ = ss.lock().unwrap().shutdown(Shutdown::Write);
        }));
        enqueue_job(poller.clone(), job);
    }
}

fn main() {
    let target = Box::new(File::create("/tmp/test.txt").expect("Can't create file"));
	Builder::new()
        .format(|buf, record| {
            writeln!(
                buf,
                "{} {:?} {}:{} [{}] - {}",
                Local::now().format("%Y-%m-%dT%H:%M:%S%.3f"),
                thread::current().id(),
                record.file().unwrap_or("unknown"),
                record.line().unwrap_or(0),
                record.level(),
                record.args()
            )
        })
        .target(env_logger::Target::Pipe(target))
        .filter(None, LevelFilter::Warn)
        .init();

    debug!("Starting main");
    let pool = EventLoopPool::new(4);
    for poller in pool.get_all_poller().iter() {
        let tcp_server = Arc::new(Mutex::new(
            TcpServer::new(
                poller.clone(),
                &"0.0.0.0:9528".to_string()
            )
        ));
        tcp_server.lock().unwrap().start::<TunnelServer>();
        TcpServer::start_internal(tcp_server);
    }
    pool.wait();
}

