extern crate sickle;
extern crate bytes;
extern crate log;
extern crate env_logger;
extern crate chrono;

use bytes::{Buf, BytesMut};
use sickle::net::{EventLoopBuilder,  EventLoopPool, TcpConnection, TcpServer, Handler, enqueue_job};
use std::sync::{Arc, Mutex, Weak};
use std::thread;

use std::io::Write;
use std::fs::File;
use chrono::Local;
use log::LevelFilter;
use env_logger::{Builder};
use log::{debug};

struct Test {
    conn: Option<Weak<Mutex<TcpConnection>>>
}

impl Test {
}

impl Drop for Test {
    fn drop(&mut self) {
    }
}

impl Handler for Test {
    fn new() -> Test {
        Test {
            conn: None,
        }
    }

    fn attach_connection(&mut self, conn: Arc<Mutex<TcpConnection>>) {
        self.conn = Some(Arc::downgrade(&conn));
    }

    fn free_connection(&mut self) {
        self.conn = None;
    }

    fn on_accept(&mut self) {
    }

    fn on_recv(&mut self, bytes: &mut BytesMut) {
        if bytes.len() <= 0 {
            return;
        }
        bytes.advance(bytes.len());
        let conn = match self.conn.as_mut() {
            Some(conn) => {
                match conn.upgrade() {
                    Some(conn) => conn.clone(),
                    None => return,
                }
            },
            None => return,
        };
        let poller = EventLoopBuilder::get_current_loop();
        let job = Arc::new(Mutex::new(move |_| {
            let mut conn = conn.lock().unwrap();
            conn.send(
                BytesMut::from(&b"HTTP/1.1 200 OK\r\nSet-Cookie:k1=v1\r\nContent-Length: 15\r\nConnection: Keep-Alive\r\n\r\nabcdefghijkldef"[..])
            ).unwrap();
        }));
        enqueue_job(poller.clone(), job);
    }

    fn on_written(&mut self) -> bool {
        self.free_connection();
        false
    }

    fn on_error(&mut self) {
        debug!("Test onError");
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
    let pool = EventLoopPool::new(1);
    for poller in pool.get_all_poller().iter() {
        let tcp_server = Arc::new(Mutex::new(
            TcpServer::new(
                poller.clone(),
                &"0.0.0.0:9528".to_string()
            )
        ));
        tcp_server.lock().unwrap().start::<Test>();
        TcpServer::start_internal(tcp_server);
    }
    pool.wait();
}

