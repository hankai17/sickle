use {Poll, Events, Token, TokenType, TokenEntry, Registration};
use event::Evented;
use event_imp::{Ready, PollOpt, Job, ready_as_usize, TimerJob};
use std::sync::atomic::{AtomicBool};
use std::sync::atomic::Ordering::{self, Acquire};
use timer::{self, Timer, Timeout};
use std::{io, usize};
use std::default::Default;
use std::time::Duration;
use std::sync::{Arc, Mutex};
use std::thread_local;
use std::thread::{self, JoinHandle};
use std::cell::UnsafeCell;
use std::sync::mpsc::channel;
use log::{debug, warn};

#[derive(Copy, Clone, Debug)]
struct Config {
    notify_capacity: usize,
    messages_per_tick: usize,
    timer_tick: Duration,
    timer_wheel_size: usize,
    timer_capacity: usize,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            notify_capacity: 4_096,
            messages_per_tick: 256,
            timer_tick: Duration::from_millis(100),
            timer_wheel_size: 1_024,
            timer_capacity: 65_536,
        }
    }
}

unsafe impl Send for EventLoop {}
unsafe impl Sync for EventLoop {}

pub struct EventLoop {
    run: AtomicBool,
    pub poll: Poll,
    timer: UnsafeCell<Timer<TimerJob>>,
}

impl EventLoop {
    fn configured(config: Config) -> io::Result<EventLoop> {
        let poll = Poll::new()?;
        let timer = timer::Builder::default()
            .tick_duration(config.timer_tick)
            .num_slots(config.timer_wheel_size)
            .capacity(config.timer_capacity)
            .build();
        poll.register(&timer,
            TokenEntry {
                ttype: TokenType::TimersEvent,
                token: Token(0)
            },
            Ready::readable(),
            PollOpt::edge(),
            Arc::new(Mutex::new(move |_| {}))
        )?;
        Ok(EventLoop {
            run: AtomicBool::new(false),
            poll,
            timer: UnsafeCell::new(timer),
        })
    }

    pub fn new() -> io::Result<EventLoop> {
        EventLoop::configured(Config::default())
    }

    pub fn timeout(&self, delay: Duration, token: TimerJob)
            -> timer::Result<Timeout> {
        let timer = unsafe { &mut *self.timer.get() };
        timer.set_timeout(delay, token)
    }

    pub fn clear_timeout(&self, timeout: &Timeout) -> bool {
        let timer = unsafe { &mut *self.timer.get() };
        timer.cancel_timeout(&timeout).is_some()
    }

    pub fn shutdown(&self) { 
        let res = self.run.compare_exchange(true, false, Acquire, Acquire);
        match res {
            Ok(_) => return,
            Err(_) => return,
        }
    }

    pub fn is_running(&self) -> bool { self.run.load(Ordering::Relaxed) }

    pub fn register<E>(&self, io: &E, token: TokenEntry, interest: Ready,
            opt: PollOpt, job: Job) -> io::Result<()>
            where E: Evented {
        self.poll.register(io, token, interest, opt, job)
    }

    pub fn reregister<E: ?Sized>(&self, io: &E, token: TokenEntry,
            interest: Ready, opt: PollOpt) -> io::Result<()>
            where E: Evented {
        self.poll.reregister(io, token, interest, opt)
    }

    pub fn deregister<E: ?Sized>(&self, io: &E) -> io::Result<()>
            where E: Evented {
        self.poll.deregister(io)
    }

    fn io_poll(&self, events: &mut Events, timeout: Option<Duration>)
            -> io::Result<usize> {
        self.poll.poll(events, timeout)
    }

    fn io_process(&self, events: &mut Events, cnt: usize) {
        let mut i = 0;
        let timer = unsafe { &mut *self.timer.get() };
        debug!("io_process cnt: {}; len: {}", cnt, events.len());
        while i < cnt {
            match events.get_mut(i) {
                Some(job_entry) => {
                    let token = job_entry.token_entry;
                    debug!("token: {:?}", token);
                    match token.ttype {
                        TokenType::TimersEvent => {
                            while let Some(mut t) = timer.poll() {
                                t();
                            }
                        },
                        _ => {
                            let job = &mut job_entry.job;
                            let ready = job_entry.ready;
                            job
                            .lock()
                            .unwrap()(ready_as_usize(ready) as i64);
                        }
                    }
                },
                None => {
                    warn!("get_mut none");
                }
            }
            i += 1;
        }
        events.clear();
        debug!("io_process done");
    }

    pub fn run_once(&self, timeout: Option<Duration>) -> io::Result<()> {
        let mut events = Events::with_capacity(1024 * 4);
        let cnt = match self.io_poll(&mut events, timeout) {
            Ok(e) => e,
            Err(err) => {
                if err.kind() == io::ErrorKind::Interrupted {
                    0
                } else {
                    return Err(err);
                }
            }
        };
        self.io_process(&mut events, cnt);
        Ok(())
    }

    pub fn run(&self) -> io::Result<()> {
        let res = self.run.compare_exchange(false, true, Acquire, Acquire);
        match res {
            Ok(_) => {},
            //Err(_) => panic!("unable run EventLoop"),
            Err(_) => {},
        }
        while self.run.load(Ordering::Relaxed) {
            self.run_once(None)?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct EventLoopBuilder {
    config: Config,
    event_loop: Option<Arc<EventLoop>>,
}

use std::cell::RefCell;
thread_local! {
    pub static CURRENT_LOOP: RefCell<Arc<EventLoop>> = panic!("!");
}

impl EventLoopBuilder {
    pub fn new() -> EventLoopBuilder {
        EventLoopBuilder {
            config: Config::default(),
            event_loop: None,
        }
    }

    pub fn notify_capacity(&mut self, capacity: usize) -> &mut Self {
        self.config.notify_capacity = capacity;
        self
    }

    pub fn messages_per_tick(&mut self, messages: usize) -> &mut Self {
        self.config.messages_per_tick = messages;
        self
    }

    pub fn timer_tick(&mut self, val: Duration) -> &mut Self {
        self.config.timer_tick = val;
        self
    }

    pub fn timer_wheel_size(&mut self, size: usize) -> &mut Self {
        self.config.timer_wheel_size = size;
        self
    }

    pub fn timer_capacity(&mut self, cap: usize) -> &mut Self {
        self.config.timer_capacity = cap;
        self
    }

	pub fn build(&mut self) -> io::Result<EventLoop> {
        EventLoop::configured(self.config)
    }

    pub fn get_build(&mut self) -> io::Result<Arc<EventLoop>> {
        let event_loop = Arc::new(self.build().unwrap());
        self.event_loop = Some(event_loop.clone());
        CURRENT_LOOP.set(event_loop.clone());
        Ok(event_loop)
    }

    pub fn set_current_loop(event_loop: Arc<EventLoop>) {
        CURRENT_LOOP.set(event_loop);
    }

    pub fn get_current_loop() -> Arc<EventLoop> {
        let ptr = CURRENT_LOOP.with(|poll|-> *mut Arc<EventLoop> {
            return poll.as_ptr()
        });
        unsafe {
            let clone = (*ptr).clone();
            clone
        }
    }
}

pub struct EventLoopPool {
    loops: Vec<Arc<EventLoop>>,
    threads: Vec<JoinHandle<()>>,
}

impl EventLoopPool {
    pub fn new(size: usize) -> EventLoopPool {
        let mut threads = Vec::with_capacity(size);
        let mut loops = Vec::with_capacity(size);
        for i in 0..size {
            let (tx, rx) = channel();
            threads.push(
                thread::Builder::new()
                .name(format!("{}{}", "thread_poller_", i))
                .spawn(move || {
                    let mut b = EventLoopBuilder::new();
                    b.notify_capacity(1_048_576)
                        .messages_per_tick(64)
                        .timer_tick(Duration::from_millis(100))
                        .timer_wheel_size(1024)
                        .timer_capacity(65536);
                    let event_loop = b.get_build().unwrap();
                    tx.send(event_loop.clone()).unwrap();
                    let _ = event_loop.run();
                }).unwrap());
            loops.push(rx.recv().unwrap());
        }
        EventLoopPool {
            loops: loops,
            threads: threads,
        }
    }

    pub fn get_first_poller(&self) -> Arc<EventLoop> {
        self.loops[0].clone()
    }

    pub fn get_all_poller(&self) -> Vec<Arc<EventLoop>> {
        self.loops.clone()
    }

    pub fn wait(self) {
        for handle in self.threads {
            handle.join().unwrap();
        }
    }
}

pub fn enqueue_job(poller: Arc<EventLoop>, cb: Job) {
    let (r, s) = Registration::new2();
    let r = Arc::new(r);
    let s = Arc::new(s);

    let r_clone = r.clone();
    let s_clone = s.clone();

    let job = Arc::new(Mutex::new(move |_| {
        let _ = r_clone.clone(); 
        let _ = s_clone.clone();
        cb.lock().unwrap()(0);
    }));
    s.set_readiness(Ready::readable()).unwrap();
    poller.register(
            &r,
            TokenEntry {
                ttype: TokenType::OtherEvent,
                token: Token(0)
            },
            Ready::readable(),
            PollOpt::edge(),
            job
    ).unwrap();
}

