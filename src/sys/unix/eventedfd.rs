use {Ready, Poll, PollOpt, poll, Job, TokenEntry};
use event::Evented;
use std::os::unix::io::RawFd;
use std::{io};

pub struct EventedFd<'a>(pub &'a RawFd);

impl <'a> Evented for EventedFd<'a> {
    fn register(&self, poll: &Poll, token: TokenEntry, interest: Ready,
            opts: PollOpt, job: Job) -> io::Result<()> {
        poll::selector(poll).register(*self.0, token, interest, opts, job)
    }
    fn reregister(&self, poll: &Poll, token: TokenEntry, interest: Ready,
            opts: PollOpt) -> io::Result<()> {
        poll::selector(poll).reregister(*self.0, token, interest, opts)
    }
    fn deregister(&self, poll: &Poll) -> io::Result<()> {
        poll::selector(poll).deregister(*self.0)
    }
}

