extern crate libc;
extern crate log;
extern crate slab;
extern crate net2;
extern crate iovec;
extern crate bytes;

mod event_imp;
pub use event_imp:: {
    PollOpt,
    Ready,
    Job,
    TimerJob,
    JobEntry,
    TokenType,
    TokenEntry,
};

mod token;
pub use token::Token;

mod poll;
pub use poll::{Poll, Registration, SetReadiness};

pub mod event {
    pub use super::poll::{Events}; 
    pub use super::event_imp::{Event, Evented};
}
pub use event::{Events, Event, Evented};

mod sys;
pub mod unix {
    pub use sys::{EventedFd,};
    pub use sys::unix::UnixReady;
}

mod lazycell;

pub mod timer;

pub mod net;
pub use iovec::IoVec;
pub mod tcp {
    pub use net::{TcpListener, TcpStream};
    pub use std::net::Shutdown;
}

mod convert {
    use std::time::Duration;
    const NANOS_PER_MILLI: u32 = 1_000_000;
    const MILLIS_PER_SEC: u64 = 1_000;
    pub fn millis(duration: Duration) -> u64 {
        let millis = (duration.subsec_nanos() + NANOS_PER_MILLI - 1) / NANOS_PER_MILLI;
        duration.as_secs().saturating_mul(MILLIS_PER_SEC).saturating_add(u64::from(millis))
    }
}

