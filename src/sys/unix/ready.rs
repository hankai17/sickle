use event_imp::{Ready, ready_from_usize};
use std::ops;

#[derive(Copy, PartialEq, Eq, Clone, PartialOrd, Ord)]
pub struct UnixReady(Ready);

const ERROR: usize  = 0b00_0100;
const HUP: usize    = 0b00_1000;
const AIO: usize    = 0b00_0000;
const LIO: usize    = 0b10_1000;
const PRI: usize    = 0b100_1000;

pub const READY_ALL: usize = ERROR | HUP | AIO | LIO | PRI;

impl UnixReady {
    pub fn aio() -> UnixReady {
        UnixReady(ready_from_usize(AIO))
    }
    pub fn hup() -> UnixReady {
        UnixReady(ready_from_usize(HUP))
    }
    pub fn error() -> UnixReady {
        UnixReady(ready_from_usize(ERROR))
    }
    pub fn priority() -> UnixReady {
        UnixReady(ready_from_usize(PRI))
    }
    pub fn is_priority(&self) -> bool {
        self.contains(ready_from_usize(PRI))
    }
    pub fn is_error(&self) -> bool {
        self.contains(ready_from_usize(ERROR))
    }
}

impl From<Ready> for UnixReady {
    fn from(src: Ready) -> UnixReady {
        UnixReady(src)
    }
}

impl From<UnixReady> for Ready {
    fn from(src: UnixReady) -> Ready {
        src.0
    }

}

impl ops::Deref for UnixReady {
    type Target = Ready;
    fn deref(&self) -> &Ready {
        &self.0
    }
}

impl ops::DerefMut for UnixReady {
    fn deref_mut(&mut self) -> &mut Ready {
        &mut self.0
    }
}

