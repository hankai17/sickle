extern crate sickle;
extern crate bytes;
extern crate log;
extern crate env_logger;
extern crate chrono;

use bytes::{Buf, BytesMut};
use std::sync::{Arc, Mutex};
use std::thread;
use std::io::Write;
use std::fs::File;
use chrono::Local;
use sickle::net::{EventLoopBuilder, EventLoopPool, TcpConnection, TcpClient, ClientHandler, enqueue_job};

use log::LevelFilter;
use env_logger::{Builder};
use log::{debug, info};

struct TestClient {
}

impl TestClient {
    fn new() -> TestClient {
        TestClient {
        }
    } 
}

impl Drop for TestClient {
    fn drop(&mut self) {
        debug!("dropping for TestClient")
    }
}

impl ClientHandler for TestClient {
    fn shutdown(&mut self) {
    }

    fn attach_connection(&mut self, _conn: Arc<Mutex<TcpConnection>>) {
        //self.conn = Some(Arc::downgrade(&conn));
    }

    fn on_connect(&mut self, conn: Arc<Mutex<TcpConnection>>) {
        let poller = EventLoopBuilder::get_current_loop();
        let job = Arc::new(Mutex::new(move |_| {
            let mut conn = conn.lock().unwrap();
            conn.send(
                BytesMut::from(&b"GET /klsdjf HTTP/1.1\r\nHost: 0.0.0.0:90\r\nUser-Agent: curl/7.61.1\r\nAccept: */*\r\n"[..])
            ).unwrap();
        }));
        enqueue_job(poller.clone(), job);
    }

    fn on_recv(&mut self, bytes: &mut BytesMut) {
        info!("on_recv: {:?}", bytes);
        bytes.advance(bytes.len());
    }

    fn on_written(&mut self) -> bool {
        debug!("write done");
        true
    }

    fn on_error(&mut self) {
        info!("connect error");
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
        .filter(None, LevelFilter::Debug)
        .init();

    debug!("Starting main");
    let pool = EventLoopPool::new(4);
    for poller in pool.get_all_poller().iter() {
        let poller_clone = poller.clone();
        let job = Arc::new(Mutex::new(move |_| {
            let mut cli = TcpClient::new(poller_clone.clone());
            cli.start_connect(
                &"127.0.0.1:90".to_string(),
                Arc::new(Mutex::new(TestClient::new()))
            );
        }));
        enqueue_job(poller.clone(), job);
    }
    pool.wait();
}

