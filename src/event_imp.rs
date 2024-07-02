use {Poll, Token};
use std::{fmt, io, ops};
use std::sync::{Arc, Mutex};
use log::debug;

const READABLE: usize = 0b00001;
const WRITABLE: usize = 0b00010;
const ERROR:    usize = 0b00100;
const HUP:      usize = 0b01000;

#[derive(Copy, PartialEq, Eq, Clone, PartialOrd, Ord)]
pub struct Ready(usize);

impl Ready {
    pub fn empty() -> Ready { Ready(0) }
    pub fn none() -> Ready { Ready::empty() }
    pub fn readable() -> Ready { Ready(READABLE) }
    pub fn writable() -> Ready { Ready(WRITABLE) }
    pub fn error() -> Ready { Ready(ERROR) }
    pub fn hup() -> Ready { Ready(HUP) }
    pub fn all() -> Ready { Ready(READABLE | WRITABLE | ::sys::READY_ALL) }
    pub fn is_empty(&self) -> bool { *self == Ready::empty() }
    pub fn is_none(&self) -> bool { self.is_empty() }
    pub fn is_readable(&self) -> bool { self.contains(Ready::readable()) }
    pub fn is_writable(&self) -> bool { self.contains(Ready::writable()) }
    pub fn is_error(&self) -> bool { self.contains(Ready(ERROR)) }
    pub fn is_hup(&self) -> bool { self.contains(Ready(HUP)) }
    pub fn insert<T: Into<Self>>(&mut self, other: T) {
        let other = other.into();
        self.0 |= other.0;
    }
    pub fn remove<T: Into<Self>>(&mut self, other: T) {
        let other = other.into();
        self.0 &= !other.0;
    }
    pub fn bits(&self) -> usize { self.0 }
    pub fn contains<T: Into<Self>>(&self, other: T) -> bool {
        let other = other.into();
        (*self & other) == other
    }
    pub fn from_usize(val: usize) -> Ready { Ready(val) }
    pub fn as_usize(&self) -> usize { self.0 }
}

pub fn ready_as_usize(events: Ready) -> usize {
    events.0
}

pub fn ready_from_usize(events: usize) -> Ready {
    Ready(events)
}

impl <T: Into<Ready>> ops::BitOr<T> for Ready {
    type Output = Ready;
    fn bitor(self, other: T) -> Ready {
        Ready(self.0 | other.into().0)
    }
}

impl <T: Into<Ready>> ops::BitOrAssign<T> for Ready {
    fn bitor_assign(&mut self, other: T) {
        self.0 |= other.into().0
    }
}

impl <T: Into<Ready>> ops::BitXor<T> for Ready {
    type Output = Ready;
    fn bitxor(self, other: T) -> Ready {
        Ready(self.0 ^ other.into().0)
    }
}

impl <T: Into<Ready>> ops::BitXorAssign<T> for Ready {
    fn bitxor_assign(&mut self, other: T) {
        self.0 ^= other.into().0
    }
}

impl <T: Into<Ready>> ops::BitAnd<T> for Ready {
    type Output = Ready;
    fn bitand(self, other: T) -> Ready {
        Ready(self.0 & other.into().0)
    }
}

impl <T: Into<Ready>> ops::BitAndAssign<T> for Ready {
    fn bitand_assign(&mut self, other: T) {
        self.0 &= other.into().0
    }
}

impl <T: Into<Ready>> ops::Sub<T> for Ready {
    type Output = Ready;
    fn sub(self, other: T) -> Ready {
        Ready(self.0 & !other.into().0)
    }
}

impl <T: Into<Ready>> ops::SubAssign<T> for Ready {
    fn sub_assign(&mut self, other: T) {
        self.0 &= !other.into().0
    }
}

impl ops::Not for Ready {
    type Output = Ready;
    fn not(self) -> Ready {
        Ready(!self.0)
    }
}

impl fmt::Debug for Ready {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let mut one = false;
        let flags = [
            (Ready::readable(), "Readable"),
            (Ready::writable(), "Writable"),
            (Ready(ERROR), "Error"),
            (Ready(HUP), "HUP")
        ];
        for &(flag, msg) in &flags {
            if self.contains(flag) {
                if one { write!(fmt, " | ")? }
                write!(fmt, "{}", msg)?;
                one = true
            }
        }
        if !one {
            fmt.write_str("(empty)")?;
        }
        Ok(())
    }
}

#[derive(Copy, PartialEq, Eq, Clone, PartialOrd, Ord)]
pub struct PollOpt(usize);

impl PollOpt {
    pub fn empty() -> PollOpt { PollOpt(0) }
    pub fn edge() -> PollOpt { PollOpt(0b0001) }
    pub fn level() -> PollOpt { PollOpt(0b0010) }
    pub fn oneshot() -> PollOpt { PollOpt(0b0100) }
    pub fn urgent() -> PollOpt { PollOpt(0b1000) }
    pub fn all() -> PollOpt { PollOpt::edge() | PollOpt::level() | PollOpt::oneshot() }
    pub fn is_edge(&self) -> bool { self.contains(PollOpt::edge()) }
    pub fn is_level(&self) -> bool { self.contains(PollOpt::level()) }
    pub fn is_oneshot(&self) -> bool { self.contains(PollOpt::oneshot()) }
    pub fn is_urgent(&self) -> bool { self.contains(PollOpt::urgent()) }
    pub fn bits(&self) -> usize { self.0 }
    pub fn contains(&self, other: PollOpt) -> bool { *self & other == other }
    pub fn insert(&mut self, other: PollOpt) { self.0 |= other.0; }
    pub fn remove(&mut self, other: PollOpt) { self.0 &= !other.0; }
}

pub fn opt_as_usize(opt: PollOpt) -> usize {
    opt.0
}

pub fn opt_from_usize(opt: usize) -> PollOpt {
    PollOpt(opt)
}

impl ops::BitOr for PollOpt {
    type Output = PollOpt;
    fn bitor(self, other: PollOpt) -> PollOpt {
        PollOpt(self.0 | other.0)
    }
}

impl ops::BitXor for PollOpt {
    type Output = PollOpt;
    fn bitxor(self, other: PollOpt) -> PollOpt {
        PollOpt(self.0 ^ other.0)
    }
}

impl ops::BitAnd for PollOpt {
    type Output = PollOpt;
    fn bitand(self, other: PollOpt) -> PollOpt {
        PollOpt(self.0 & other.0)
    }
}

impl ops::Sub for PollOpt {
    type Output = PollOpt;
    fn sub(self, other: PollOpt) -> PollOpt {
        PollOpt(self.0 & !other.0)
    }
}

impl ops::Not for PollOpt {
    type Output = PollOpt;
    fn not(self) -> PollOpt {
        PollOpt(!self.0)
    }
}

impl fmt::Debug for PollOpt {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let mut one = false;
        let flags = [
            (PollOpt::edge(), "Edge-Triggered"),
            (PollOpt::level(), "Level-Triggered"),
            (PollOpt::oneshot(), "OneShot")
        ];
        for &(flag, msg) in &flags {
            if self.contains(flag) {
                if one { write!(fmt, " | ")? }
                write!(fmt, "{}", msg)?;
                one = true
            }
        }
        if !one {
            fmt.write_str("(empty)")?;
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum TokenType {
#[warn(non_camel_case_types)]
    SocketEvent,
    NotifyEvent,
    TimersEvent,
    JobsEvent,
    TokenEvent,
    AcceptEvent,
    OtherEvent,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct TokenEntry {
    pub ttype: TokenType, 
    pub token: Token,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct Event {
    kind: Ready,
    token: TokenEntry
}

impl Event {
    pub fn new(readiness: Ready, token: TokenEntry) -> Event {
        Event {
            kind: readiness,
            token,
        }
    }
    pub fn readiness(&self) -> Ready {
        self.kind
    }
    pub fn kind(&self) -> Ready { self.kind }
    pub fn token(&self) -> TokenEntry { self.token }
}

pub type Job = Arc<Mutex<dyn FnMut(i64) + 'static + Send + Sync>>;
pub type TimerJob = Box<dyn FnMut() + 'static + Send + Sync>;

#[derive(Clone)]
pub struct JobEntry {
    pub token_entry: TokenEntry,
    pub job: Job,
    pub ready: Ready,
}

impl Drop for JobEntry {
    fn drop(&mut self) {
        debug!("Droping JobEntry token: {:?}", self.token_entry.token);
    }
}

pub trait Evented {
    fn register(&self, poll: &Poll, token: TokenEntry, interest: Ready,
            opts: PollOpt, job: Job) -> io::Result<()>;
    fn reregister(&self, poll: &Poll, token: TokenEntry, interest: Ready,
            opts: PollOpt) -> io::Result<()>;
    fn deregister(&self, poll: &Poll) -> io::Result<()>;
}

impl Evented for Box<dyn Evented> {
    fn register(&self, poll: &Poll, token: TokenEntry, interest: Ready,
            opts: PollOpt, job: Job) -> io::Result<()> {
        self.as_ref().register(poll, token, interest, opts, job)
    }
    fn reregister(&self, poll: &Poll, token: TokenEntry, interest: Ready,
            opts: PollOpt) -> io::Result<()> {
        self.as_ref().reregister(poll, token, interest, opts)
    }
    fn deregister(&self, poll: &Poll) -> io::Result<()> {
        self.as_ref().deregister(poll)
    }
}

impl <T: Evented> Evented for Box<T> {
    fn register(&self, poll: &Poll, token: TokenEntry, interest: Ready,
            opts: PollOpt, job: Job) -> io::Result<()> {
        self.as_ref().register(poll, token, interest, opts, job)
    }
    fn reregister(&self, poll: &Poll, token: TokenEntry, interest: Ready,
            opts: PollOpt) -> io::Result<()> {
        self.as_ref().reregister(poll, token, interest, opts)
    }
    fn deregister(&self, poll: &Poll) -> io::Result<()> {
        self.as_ref().deregister(poll)
    }
}

impl <T: Evented> Evented for ::std::sync::Arc<T> {
    fn register(&self, poll: &Poll, token: TokenEntry, interest: Ready,
            opts: PollOpt, job: Job) -> io::Result<()> {
        self.as_ref().register(poll, token, interest, opts, job)
    }
    fn reregister(&self, poll: &Poll, token: TokenEntry, interest: Ready,
            opts: PollOpt) -> io::Result<()> {
        self.as_ref().reregister(poll, token, interest, opts)
    }
    fn deregister(&self, poll: &Poll) -> io::Result<()> {
        self.as_ref().deregister(poll)
    }
}

