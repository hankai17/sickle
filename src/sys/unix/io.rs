use std::fs::File;
use std::{io};
use std::io::{Read, Write};
use std::os::unix::io::{IntoRawFd, AsRawFd, FromRawFd, RawFd};

use {Ready, Poll, PollOpt, Job, TokenEntry};
use event::Evented;
use unix::EventedFd;
use sys::unix::cvt;

pub fn set_nonblock(fd: libc::c_int) -> io::Result<()> {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        cvt(libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK)).map(|_|())
    }
}

pub fn set_cloexec(fd: libc::c_int) -> io::Result<()> {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        cvt(libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC)).map(|_|())
    }
}

pub struct Io {
    fd: File,
}

impl Io {
    pub fn try_clone(&self) -> io::Result<Io> {
        Ok(Io { fd: self.fd.try_clone()? })
    }
}

/*
impl Drop for Io {
    fn drop(&mut self) {
        println!("Dropping Io!");
    }
}
*/

impl FromRawFd for Io {
    unsafe fn from_raw_fd(fd: RawFd) -> Io {
        Io { fd: File::from_raw_fd(fd) }
    }
}

impl IntoRawFd for Io {
    fn into_raw_fd(self) -> RawFd {
        self.fd.into_raw_fd()
    }
}

impl AsRawFd for Io {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl Evented for Io {
    fn register(&self, poll: &Poll, token: TokenEntry, interest: Ready,
            opts: PollOpt, job: Job) -> io::Result<()> {
        EventedFd(&self.as_raw_fd()).register(poll, token, interest, opts, job)
    }
    fn reregister(&self, poll: &Poll, token: TokenEntry, interest: Ready,
            opts: PollOpt) -> io::Result<()> {
        EventedFd(&self.as_raw_fd()).reregister(poll, token, interest, opts)
    }
    fn deregister(&self, poll: &Poll) -> io::Result<()> {
        EventedFd(&self.as_raw_fd()).deregister(poll)
    }
}

impl Read for Io {
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        println!("fd {:?} read", self.fd);
        (&self.fd).read(dst)
    }
}

impl <'a> Read for &'a Io {
    fn read(&mut self, dst: &mut [u8]) -> io::Result<usize> {
        (&self.fd).read(dst)
    }
}

impl Write for Io {
    fn write(&mut self, src: &[u8]) -> io::Result<usize> {
        (&self.fd).write(src)
    }
    fn flush(&mut self) -> io::Result<()> {
        (&self.fd).flush()
    }
}

impl <'a> Write for &'a Io {
    fn write(&mut self, src: &[u8]) -> io::Result<usize> {
        (&self.fd).write(src)
    }
    fn flush(&mut self) -> io::Result<()> {
        (&self.fd).flush()
    }
}
