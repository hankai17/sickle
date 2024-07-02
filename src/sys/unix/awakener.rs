pub use self::pipe::Awakener;

mod pipe {
    use sys::unix;
    use {Ready, Poll, PollOpt, Job, TokenEntry};
    use std::io::{self, Read, Write};
    use event::Evented;

    pub struct Awakener {
        reader: unix::Io,
        writer: unix::Io,
    }

    impl Awakener {
        pub fn new() -> io::Result<Awakener> {
            let (rd, wr) = unix::pipe()?;
            Ok( Awakener {
                reader: rd,
                writer: wr,
            })
        }
        pub fn wakeup(&self) -> io::Result<()> {
            match (&self.writer).write(&[1]) {
                Ok(_) => Ok(()),
                Err(e) => {
                    if e.kind() == io::ErrorKind::WouldBlock {
                        Ok(())
                    } else {
                        Err(e)
                    }
                }
            }
        }
        pub fn cleanup(&self) {
            let mut buf = [0; 128];
            loop {
                match (&self.reader).read(&mut buf) {
                    Ok(i) if i > 0 => {},
                    _ => return
                }
            }
        }
        pub fn reader(&self) -> &unix::Io {
            &self.reader
        }
    }

    impl Evented for Awakener {
        fn register(&self, poll: &Poll, token: TokenEntry, interest: Ready,
                opts: PollOpt, job: Job) -> io::Result<()> {
            self.reader().register(poll, token, interest, opts, job)
        }
        fn reregister(&self, poll: &Poll, token: TokenEntry, interest: Ready,
                opts: PollOpt) -> io::Result<()> {
            self.reader().reregister(poll, token, interest, opts)
        }
        fn deregister(&self, poll: &Poll) -> io::Result<()> {
            self.reader().deregister(poll)
        }
    }
}

