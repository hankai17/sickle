use std::{fmt, io, ptr, usize};
use std::{mem, ops, isize};
use std::os::unix::io::{AsRawFd, RawFd};
use std::process;
use std::sync::atomic::{AtomicUsize, AtomicPtr, AtomicBool};
use std::cell::UnsafeCell;
use std::sync::{Arc, Mutex, Condvar};
use std::sync::atomic::Ordering::{self, Acquire, Release, AcqRel, Relaxed, SeqCst};
use std::time::{Duration, Instant};
use log::{debug};

use event_imp::{self as event, Ready, Event, Evented, PollOpt, Job, JobEntry, TokenEntry, TokenType};
use {Token, sys};

const READINESS_SHIFT: usize = 0;
const INTEREST_SHIFT: usize = 4;
const POLL_OPT_SHIFT: usize = 8;
const TOKEN_RD_SHIFT: usize = 12;
const TOKEN_WR_SHIFT: usize = 14;
const QUEUED_SHIFT: usize = 16;
const DROPPED_SHIFT: usize = 17;

const MASK_2: usize = 4 - 1;
const MASK_4: usize = 16 - 1;
const QUEUED_MASK: usize = 1 << QUEUED_SHIFT;
const DROPPED_MASK: usize = 1 << DROPPED_SHIFT;
const MAX_REFCOUNT: usize = (isize::MAX) as usize;

pub struct SelectorId {
    id: AtomicUsize,
}

impl SelectorId {
    pub fn new() -> SelectorId {
        SelectorId {
            id: AtomicUsize::new(0)
        }
    }
    pub fn associate_selector(&self, poll: &Poll) -> io::Result<()> {
        let selector_id = self.id.load(Ordering::SeqCst);
        if selector_id != 0 && selector_id != poll.selector.id() {
            Err(io::Error::new(io::ErrorKind::Other, "Socket already registered"))
        } else {
            self.id.store(poll.selector.id(), Ordering::SeqCst);
            Ok(())
        }
    }
}

impl Clone for SelectorId {
    fn clone(&self) -> SelectorId {
        SelectorId {
            id: AtomicUsize::new(self.id.load(Ordering::SeqCst))
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct ReadinessState(usize);   // | queue |  RW  | opt | interest | readiness
                                // 20      16     12    8          4      <--0
impl ReadinessState {
    fn new(interest: Ready, opt: PollOpt) -> ReadinessState {
        let interest = event::ready_as_usize(interest);
        let opt = event::opt_as_usize(opt);
        debug_assert!(interest <= MASK_4);
        debug_assert!(opt <= MASK_4);
        let mut val = interest << INTEREST_SHIFT;
        val |= opt << POLL_OPT_SHIFT;
        ReadinessState(val)
    }
    fn get(self, mask: usize, shift: usize) -> usize {
        (self.0 >> shift) & mask
    }
    fn set(&mut self, val: usize, mask: usize, shift: usize) {
        self.0 = (self.0 & !(mask << shift)) | (val << shift) 
    }
    fn readiness(self) -> Ready {
        let v = self.get(MASK_4, READINESS_SHIFT);
        event::ready_from_usize(v)
    }
    fn interest(self) -> Ready {
        let v = self.get(MASK_4, INTEREST_SHIFT);
        event::ready_from_usize(v)
    }
    fn poll_opt(self) -> PollOpt {
        let v = self.get(MASK_4, POLL_OPT_SHIFT);
        event::opt_from_usize(v)
    }
    fn effective_readiness(self) -> Ready {
        self.readiness() & self.interest()
    }
    fn set_readiness(&mut self, v: Ready) {
        self.set(event::ready_as_usize(v), MASK_4, READINESS_SHIFT);
    }
    fn set_interest(&mut self, v: Ready) {
        self.set(event::ready_as_usize(v), MASK_4, INTEREST_SHIFT);
    }
    fn set_poll_opt(&mut self, v: PollOpt) {
        self.set(event::opt_as_usize(v), MASK_4, POLL_OPT_SHIFT); 
    }
    fn disarm(&mut self) {
        self.set_interest(Ready::empty())
    }
    fn is_queued(self) -> bool {
        self.0 & QUEUED_MASK == QUEUED_MASK
    }
    fn is_dropped(self) -> bool {
        self.0 & DROPPED_MASK == DROPPED_MASK 
    }
    fn set_queued(&mut self) {
        debug_assert!(!self.is_dropped());
        self.0 |= QUEUED_MASK;
    }
    fn set_dequeued(&mut self) {
        debug_assert!(self.is_queued());
        self.0 &= !QUEUED_MASK
    }
    fn token_read_pos(self) -> usize {
        self.get(MASK_2, TOKEN_RD_SHIFT)
    }
    fn token_write_pos(self) -> usize {
        self.get(MASK_2, TOKEN_WR_SHIFT)
    }
    fn set_token_write_pos(&mut self, val: usize) {
        self.set(val, MASK_2, TOKEN_WR_SHIFT)
    }
    fn update_token_read_pos(&mut self) {
        let val = self.token_write_pos();
        self.set(val, MASK_2, TOKEN_RD_SHIFT);
    }
    fn next_token_pos(self) -> usize {
        let rd = self.token_read_pos();
        let wr = self.token_write_pos();
        match wr {
            0 => {
                match rd {
                    1 => 2,
                    2 => 1,
                    0 => 1,
                    _ => unreachable!(),
                }
            }
            1 => {
                match rd {
                    0 => 2,
                    2 => 0,
                    1 => 2,
                    _ => unreachable!(),
                }
            }
            2 => {
                match rd {
                    0 => 1,
                    1 => 0,
                    2 => 0,
                    _ => unreachable!(),
                }
            }
            _ => unreachable!(),
        }
    }
}

impl From<ReadinessState> for usize {
    fn from(src: ReadinessState) -> usize {
        src.0
    }
}

impl From<usize> for ReadinessState {
    fn from(src: usize) -> ReadinessState {
        ReadinessState(src)
    }
}

struct AtomicState {
    inner: AtomicUsize,
}

impl AtomicState {
    fn new(interest: Ready, opt: PollOpt) -> AtomicState {
        let state = ReadinessState::new(interest, opt);
        AtomicState {
            inner: AtomicUsize::new(state.into()),
        }
    }
    fn load(&self, order: Ordering) -> ReadinessState {
        self.inner.load(order).into()
    }
    fn compare_and_swap(&self, current: ReadinessState, 
            new: ReadinessState, order: Ordering) -> ReadinessState {
        let success_order = order;
        let mut fail_order = order;
        if order == Release {
            fail_order = Relaxed;
        } else if order == AcqRel {
            fail_order = Acquire;
        }
        let res = self.inner.compare_exchange(current.into(), new.into(), 
                success_order, fail_order);
        match res {
            Ok(val) => val.into(),
            Err(val) => val.into(),
        }
    }
    fn flag_as_dropped(&self) -> bool {
        let prev: ReadinessState = self.inner.fetch_or(DROPPED_MASK | QUEUED_MASK, Release).into();
        debug_assert!(!prev.is_dropped());
        !prev.is_queued()
    }
}

struct ReadinessNode {
    state: AtomicState,
    token_0: UnsafeCell<TokenEntry>,
    token_1: UnsafeCell<TokenEntry>,
    token_2: UnsafeCell<TokenEntry>,
    next_readiness: AtomicPtr<ReadinessNode>,
    update_lock: AtomicBool,
    readiness_queue: AtomicPtr<()>,
    ref_count: AtomicUsize,
    job: Job,
}

enum Dequeue {
    Data(*mut ReadinessNode),
    Empty,
    Inconsistent,
}

fn enqueue_with_wakeup(queue: *mut(), node: &ReadinessNode) -> io::Result<()> {
    debug_assert!(!queue.is_null());
    let queue: &Arc<ReadinessQueueInner> = unsafe {
        &*(&queue as *const *mut () as * const Arc<ReadinessQueueInner>)
    };
    queue.enqueue_node_with_wakeup(node)
}

impl ReadinessNode {
    fn new(queue: *mut(), token: TokenEntry, interest: Ready,
            opt: PollOpt, ref_count: usize) -> ReadinessNode {
        ReadinessNode {
            state: AtomicState::new(interest, opt),
            token_0: UnsafeCell::new(token),
            token_1: UnsafeCell::new(TokenEntry { token: Token(0), ttype: TokenType::TokenEvent} ),
            token_2: UnsafeCell::new(TokenEntry { token: Token(0), ttype: TokenType::TokenEvent} ),
            next_readiness: AtomicPtr::new(ptr::null_mut()),
            update_lock: AtomicBool::new(false),
            readiness_queue: AtomicPtr::new(queue),
            ref_count: AtomicUsize::new(ref_count),
            job: Arc::new(Mutex::new(move |_| {})),
        }
    }

    fn marker() -> ReadinessNode {
        ReadinessNode {
            state: AtomicState::new(Ready::empty(), PollOpt::empty()),
            token_0: UnsafeCell::new(TokenEntry { token: Token(0), ttype: TokenType::TokenEvent}),
            token_1: UnsafeCell::new(TokenEntry { token: Token(0), ttype: TokenType::TokenEvent}),
            token_2: UnsafeCell::new(TokenEntry { token: Token(0), ttype: TokenType::TokenEvent}),
            next_readiness: AtomicPtr::new(ptr::null_mut()),
            update_lock: AtomicBool::new(false),
            readiness_queue: AtomicPtr::new(ptr::null_mut()),
            ref_count: AtomicUsize::new(0),
            job: Arc::new(Mutex::new(move |_| {})),
        }
    }

    fn enqueue_with_wakeup(&self) -> io::Result::<()> {
        let queue = self.readiness_queue.load(Acquire);
        if queue.is_null() {
            return Ok(())
        }
        enqueue_with_wakeup(queue, self)
    }
}

struct ReadinessQueueInner {
    awakener: sys::Awakener,
    head_readiness: AtomicPtr<ReadinessNode>,
    tail_readiness: UnsafeCell<*mut ReadinessNode>,
    end_marker: Box<ReadinessNode>,
    sleep_marker: Box<ReadinessNode>,
    closed_marker: Box<ReadinessNode>,
}

fn release_node(ptr: *mut ReadinessNode) {
    unsafe {
        if (*ptr).ref_count.fetch_sub(1, AcqRel) != 1 {
            return;
        }
        let node = Box::from_raw(ptr);
        let queue = node.readiness_queue.load(Acquire);
        if queue.is_null() {
            return;
        }
        let _: Arc<ReadinessQueueInner> = mem::transmute(queue);
    }
}

impl ReadinessQueueInner {
    fn end_marker(&self) -> *mut ReadinessNode {
        &*self.end_marker as *const ReadinessNode as *mut ReadinessNode
    }

    fn sleep_marker(&self) -> *mut ReadinessNode {
        &*self.sleep_marker as *const ReadinessNode as *mut ReadinessNode
    }

    fn closed_marker(&self) -> *mut ReadinessNode {
        &*self.closed_marker as *const ReadinessNode as *mut ReadinessNode
    }

    fn wakeup(&self) -> io::Result<()> {
        self.awakener.wakeup()
    }

    fn enqueue_node_with_wakeup(&self, node: &ReadinessNode) -> io::Result<()> {
        if self.enqueue_node(node) {
            self.wakeup()?;
        } else {
        }
        Ok(())
    }

    fn enqueue_node(&self, node: &ReadinessNode) -> bool {
        let node_ptr = node as * const _ as * mut _;
        node.next_readiness.store(ptr::null_mut(), Relaxed);
        unsafe {
            let mut prev = self.head_readiness.load(Acquire);
            loop {
                if prev == self.closed_marker() {
                    debug_assert!(node_ptr != self.closed_marker());
                    debug_assert!(node_ptr != self.sleep_marker());
                    if node_ptr != self.end_marker() {
                        debug_assert!(node.ref_count.load(Relaxed) >= 2);
                        release_node(node_ptr);
                    }
                    return false;
                }
                let res = self.head_readiness.compare_exchange(prev, node_ptr,
                        AcqRel, Acquire);
                match res {
                    Ok(_) => break,
                    Err(val) => prev = val,
                }
            }
            debug_assert!((*prev).next_readiness.load(Relaxed).is_null());
            (*prev).next_readiness.store(node_ptr, Release);
            prev == self.sleep_marker()
        }
    }

    fn clear_sleep_marker(&self) {
        let e_marker = self.end_marker();
        let s_marker = self.sleep_marker();
        unsafe {
            let tail = *self.tail_readiness.get();
            if tail != self.sleep_marker() {
                return;
            }
            self.end_marker.next_readiness.store(ptr::null_mut(), Relaxed);
            let res = self.head_readiness.compare_exchange(s_marker, e_marker,
                    AcqRel, Acquire);
            match res {
                Ok(val) => {
                    debug_assert!(val != e_marker);
                    *self.tail_readiness.get() = e_marker;
                },
                Err(val) => {
                    debug_assert!(val != e_marker);
                    return;
                },
            }
        }
    }

    unsafe fn dequeue_node(&self, until: *mut ReadinessNode)
            -> Dequeue {
        let mut tail = *(self.tail_readiness.get());
        let mut next = (*tail).next_readiness.load(Acquire);
        if tail == self.end_marker()
                || tail == self.sleep_marker()
                || tail == self.closed_marker() {
            if next.is_null() {
                self.clear_sleep_marker();
                return Dequeue::Empty;
            }
            *self.tail_readiness.get() = next;
            tail = next;
            next = (*next).next_readiness.load(Acquire);
        }
        if tail == until {
            return Dequeue::Empty;
        }
        if !next.is_null() {
            *self.tail_readiness.get() = next;
            return Dequeue::Data(tail);
        }
        if self.head_readiness.load(Acquire) != tail {
            return Dequeue::Inconsistent;
        }
        self.enqueue_node(&*self.end_marker);
        next = (*tail).next_readiness.load(Acquire);
        if !next.is_null() {
            *self.tail_readiness.get() = next;
            return Dequeue::Data(tail);
        }
        return Dequeue::Inconsistent;
    }
}

#[derive(Clone)]
struct ReadinessQueue {
    inner: Arc<ReadinessQueueInner>,
}

unsafe impl Send for ReadinessQueue {}
unsafe impl Sync for ReadinessQueue {}
unsafe fn token(node: &ReadinessNode, pos: usize) -> TokenEntry {
    match pos {
        0 => *node.token_0.get(),
        1 => *node.token_1.get(),
        2 => *node.token_2.get(),
        _ => unreachable!(),
    }
}

impl ReadinessQueue {
    fn new() -> io::Result<ReadinessQueue> {
        is_send::<Self>();
        is_sync::<Self>();
        let end_marker = Box::new(ReadinessNode::marker());
        let sleep_marker = Box::new(ReadinessNode::marker());
        let closed_marker = Box::new(ReadinessNode::marker());
        let ptr = &(*end_marker) as *const _ as *mut _;
        Ok(ReadinessQueue {
            inner: Arc::new(ReadinessQueueInner {
                awakener: sys::Awakener::new()?,
                head_readiness: AtomicPtr::new(ptr),
                tail_readiness: UnsafeCell::new(ptr),
                end_marker,
                sleep_marker,
                closed_marker,
            })
        })
    }

    fn poll(&self, dst: &mut sys::Events) {
        let mut until = ptr::null_mut();
        if dst.len() == dst.capacity() {
            self.inner.clear_sleep_marker();
        }
        'outer:
        while dst.len() < dst.capacity() {
            let ptr = match unsafe { self.inner.dequeue_node(until) } {
                Dequeue::Empty | Dequeue::Inconsistent => break,
                Dequeue::Data(ptr) => ptr,
            };
            //let node = unsafe { &*ptr };
            let node = unsafe { &mut *ptr };
            let mut state = node.state.load(Acquire);
            let mut next;
            let mut readiness;
            let mut opt;
            loop {
                next = state;
                debug_assert!(state.is_queued());
                if state.is_dropped() {
                    release_node(ptr);
                    continue 'outer;
                }
                readiness = state.effective_readiness();
                opt = state.poll_opt();
                if opt.is_edge() {
                    next.set_dequeued();
                    if opt.is_oneshot() && !readiness.is_empty() {
                        next.disarm();
                    }
                } else if readiness.is_empty() {
                    next.set_dequeued();
                }
                next.update_token_read_pos();
                if state == next {
                    break;
                }
                let actual = node.state.compare_and_swap(state, next, AcqRel);
                if actual == state {
                    break;
                }
                state = actual;
            }
            if next.is_queued() {
                if until.is_null() {
                    until = ptr;
                }
                self.inner.enqueue_node(node);
            }
            if !readiness.is_empty() {
                let token = unsafe { token(node, next.token_read_pos()) };
                dst.push_event(Event::new(readiness, token), node.job.clone());
                node.job = Arc::new(Mutex::new(|_| {}));
            }
        }
    }

    fn prepare_for_sleep(&self) -> bool {
        let emarker = self.inner.end_marker();
        let smarker = self.inner.sleep_marker();
        let tail = unsafe { *self.inner.tail_readiness.get() };
        if tail == smarker {
            return self.inner.head_readiness.load(Acquire) == smarker;
        }
        if tail != emarker {
            return false;
        }
        self.inner.sleep_marker.next_readiness
                .store(ptr::null_mut(), Relaxed);
        let res = self.inner.head_readiness
                .compare_exchange(emarker, smarker, AcqRel, Acquire);
        match res {
            Ok(val) => {
                debug_assert!(val != smarker);
                debug_assert!(unsafe {
                    *self.inner.tail_readiness.get() == emarker
                });
                debug_assert!(self.inner.end_marker.next_readiness
                        .load(Relaxed).is_null());
                unsafe { *self.inner.tail_readiness.get() = smarker };
                true
            },
            Err(val) => {
                debug_assert!(val != smarker);
                return false;
            },
        }
    }
}

impl Drop for ReadinessQueue {
    fn drop(&mut self) {
        self.inner.enqueue_node(&*self.inner.closed_marker);
        loop {
            let ptr = match unsafe { self.inner.dequeue_node(ptr::null_mut()) } {
                Dequeue::Empty => break,
                Dequeue::Inconsistent => {
                    continue;
                }
                Dequeue::Data(ptr) => ptr,
            };
            let node = unsafe { &*ptr };
            let state = node.state.load(Acquire);
            debug_assert!(state.is_queued());
            release_node(ptr);
        }
    }
}

pub struct Poll {
    selector: sys::Selector,
    readiness_queue: ReadinessQueue,
    lock_state: AtomicUsize,
    lock: Mutex<()>,
    condvar: Condvar,
}

fn is_send<T: Send>() {}
fn is_sync<T: Sync>() {}

pub fn selector(poll: &Poll) -> &sys::Selector {
    &poll.selector
}

impl Poll {
    pub fn new() -> io::Result<Poll> {
        is_send::<Poll>(); 
        is_sync::<Poll>(); 
        let poll = Poll {
            selector: sys::Selector::new()?,
            readiness_queue: ReadinessQueue::new()?,
            lock_state: AtomicUsize::new(0),
            lock: Mutex::new(()),
            condvar: Condvar::new(),
        };
        poll.readiness_queue.inner.awakener.register(
                &poll,
                TokenEntry {
                    ttype: TokenType::NotifyEvent, 
                    token: Token(0)
                }, 
                Ready::readable(),
                PollOpt::edge(),
                Arc::new(Mutex::new(move |_| {}))
        )?;
        Ok(poll)
    }

    pub fn register<E: ?Sized>(&self, handle: &E, token: TokenEntry,
            interest: Ready, opts: PollOpt, job: Job) -> io::Result<()>
        where E: Evented
    {
        log::trace!("register poller");
        handle.register(self, token, interest, opts, job)?;
        Ok(())
    }

    pub fn reregister<E: ?Sized>(&self, handle: &E, token: TokenEntry,
            interest: Ready, opts: PollOpt) -> io::Result<()>
        where E: Evented
    {
        log::trace!("reregister poller");
        handle.reregister(self, token, interest, opts)?;
        Ok(())
    }

    pub fn deregister<E: ?Sized>(&self, handle: &E) -> io::Result<()>
        where E: Evented
    {
        log::trace!("degister poller");
        handle.deregister(self)?;
        Ok(())
    }

    pub fn poll(&self, events: &mut Events, timeout: Option<Duration>)
            -> io::Result<usize> {
        self.poll1(events, timeout, false)
    }

    pub fn poll_interruptible(&self, events: &mut Events,
            timeout: Option<Duration>) -> io::Result<usize> {
        self.poll1(events, timeout, true)
    }

    fn poll2(&self, events: &mut Events, mut timeout: Option<Duration>,
            interruptible: bool) -> io::Result<usize> {
        debug!("poll2 begin -------------->");
        //debug!("poll2 timeout1: {:?}", timeout);
        if timeout == Some(Duration::from_millis(0)) {
        } else if self.readiness_queue.prepare_for_sleep() {
        } else {
            timeout = Some(Duration::from_millis(0))
        }
        //debug!("poll2 timeout2: {:?}", timeout);
        loop {
            let now = Instant::now();
            let res = self.selector.select(&mut events.inner, timeout);
            match res {
                Ok(true) => {
                    self.readiness_queue.inner.awakener.cleanup();
                    break;
                }
                Ok(false) => break,
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted 
                        && !interruptible => {
                    if let Some(to) = timeout {
                        let elapsed = now.elapsed();
                        if elapsed >= to {
                            break;
                        } else {
                            timeout = Some(to - elapsed);
                        }
                    }
                },
                Err(e) => return Err(e),
            }
        }
        self.readiness_queue.poll(&mut events.inner);
        debug!("poll2 end <--------------");
        Ok(events.inner.len())
    }

    fn poll1(&self, events: &mut Events, mut timeout: Option<Duration>,
            interruptible: bool) -> io::Result<usize> {
        let zero = Some(Duration::from_millis(0));
        let mut curr = match self.lock_state
                .compare_exchange(0, 1, SeqCst, SeqCst) {
            Ok(val) => val,
            Err(val) => val,
        };
        if 0 != curr {
            let mut lock = self.lock.lock().unwrap();
            let mut inc = false;
            loop {
                if curr & 1 == 0 {
                    let mut next = curr | 1;
                    if inc {
                        next -= 2;
                    }
                    let actual = match self.lock_state
                            .compare_exchange(curr, next, SeqCst, SeqCst) {
                        Ok(val) => val,
                        Err(val) => val,
                    };
                    if actual != curr {
                        curr = actual;
                        continue;
                    }
                    break;
                }
                if timeout == zero {
                    if inc {
                        self.lock_state.fetch_sub(2, SeqCst);
                    }
                    return Ok(0);
                }
                if !inc {
                    let next = curr.checked_add(2).expect("overflow");
                    let actual = match self.lock_state
                            .compare_exchange(curr, next, SeqCst, SeqCst) {
                        Ok(val) => val,
                        Err(val) => val,
                    };
                    if actual != curr {
                        curr = actual;
                        continue;
                    }
                    inc = true;
                }
                lock = match timeout {
                    Some(to) => {
                        let now = Instant::now();
                        let (l, _) = self.condvar.wait_timeout(lock, to).unwrap();
                        let elapsed = now.elapsed();
                        if elapsed >= to {
                            timeout = zero;
                        } else {
                            timeout = Some(to - elapsed);
                        }
                        l
                    }
                    None => {
                        self.condvar.wait(lock).unwrap()
                    }
                };
                curr = self.lock_state.load(SeqCst);
            }
        }
        let ret = self.poll2(events, timeout, interruptible);
        if 1 != self.lock_state.fetch_and(!1, Release) {
            let _lock = self.lock.lock().unwrap();
            self.condvar.notify_one();
        }
        ret
    }
}

impl fmt::Debug for Poll {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Poll").finish()
    }
}

impl AsRawFd for Poll {
    fn as_raw_fd(&self) -> RawFd {
        self.selector.as_raw_fd()
    }
}

pub struct Events {
    inner: sys::Events,
}

impl Events {
    pub fn with_capacity(capacity: usize) -> Events {
        Events {
            inner: sys::Events::with_capacity(capacity),
        }
    }
    pub fn get_mut(&mut self, idx: usize) -> Option<&mut JobEntry> {
        self.inner.get_mut(idx)
    }
    pub fn len(&self) -> usize {
        self.inner.len()
    }
    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
    pub fn clear(&mut self) {
        self.inner.clear()
    }
}

struct RegistrationInner {
    node: *mut ReadinessNode,
}

impl RegistrationInner {
    fn readiness(&self) -> Ready {
        self.state.load(Relaxed).readiness()
    }

    fn set_readiness(&self, ready: Ready) -> io::Result<()> {
        let mut state = self.state.load(Acquire);
        let mut next;
        loop {
            next = state;
            if state.is_dropped() {
                return Ok(());
            }
            next.set_readiness(ready);
            if !next.effective_readiness().is_empty() {
                next.set_queued();
            }
            let actual = self.state.compare_and_swap(state, next, AcqRel);
            if state == actual {
                break;
            }
            state = actual;
        }
        if !state.is_queued() && next.is_queued() {
            self.enqueue_with_wakeup()?;
        }
        Ok(())
    }

    fn update(&self, poll: &Poll, token: TokenEntry, interest: Ready,
            opt: PollOpt, job: Job) -> io::Result<()> {
        let mut queue = self.readiness_queue.load(Relaxed);
        let other: &*mut () = unsafe {
            &*(&poll.readiness_queue.inner as *const _ as *const *mut())
        };
        let other = *other;
        debug_assert!(mem::size_of::<Arc<ReadinessQueueInner>>() == mem::size_of::<*mut ()>());
        if queue.is_null() {
            let actual = match self.readiness_queue.compare_exchange(queue, other,
                    Release, Relaxed) {
                Ok(val) => val,
                Err(val) => val,
            };
            if actual.is_null() {
                self.ref_count.fetch_add(1, Relaxed);
                mem::forget(poll.readiness_queue.clone());
            } else {
                if actual != other {
                    return Err(io::Error::new(io::ErrorKind::Other,
                            "Registration handle associated with another Poll instance"));
                }
            }
            queue = other;
        } else if queue != other {
            return Err(io::Error::new(io::ErrorKind::Other,
                    "Registration handle associated with another Poll instance"));
        }
        unsafe {
            let actual = &poll.readiness_queue.inner as *const _ as *const usize;
            debug_assert_eq!(queue as usize, *actual);
        }
        let res = self.update_lock.compare_exchange(false, true, Acquire, Acquire);
        match res {
            Ok(_) => {},
            Err(_) => return Ok(()),
        }
        let mut state = self.state.load(Relaxed);
        let mut next;
        let curr_token_pos = state.token_write_pos();
        let curr_token = unsafe { self::token(self, curr_token_pos) };
        let mut next_token_pos = curr_token_pos;
        if token != curr_token {
            next_token_pos = state.next_token_pos();
            match next_token_pos {
                0 => unsafe { *self.token_0.get() = token },
                1 => unsafe { *self.token_1.get() = token },
                2 => unsafe { *self.token_2.get() = token },
                _ => unreachable!(),
            }
        }
        loop {
            next = state;
            debug_assert!(!state.is_dropped());
            next.set_token_write_pos(next_token_pos);
            next.set_interest(interest);
            next.set_poll_opt(opt);
            if !next.effective_readiness().is_empty() {
                next.set_queued();
            }
            let actual = self.state.compare_and_swap(state, next, Release);
            if actual == state {
                break;
            }
            debug_assert_eq!(curr_token_pos, actual.token_write_pos());
            state = actual;
        }
        self.update_lock.store(false, Release);
        if !state.is_queued() && next.is_queued() {
            unsafe { 
                (*self.node).job = job;
            }
            enqueue_with_wakeup(queue, self)?;
        }
        Ok(())
    }
}

impl ops::Deref for RegistrationInner {
    type Target = ReadinessNode;
    fn deref(&self) -> &ReadinessNode {
        unsafe { &*self.node }
    }
}

impl Clone for RegistrationInner {
    fn clone(&self) -> RegistrationInner {
        let old_size = self.ref_count.fetch_add(1, Relaxed);
        if old_size & !MAX_REFCOUNT != 0 {
            process::abort();
        }
        RegistrationInner {
            node: self.node,
        }
    }
}

impl Drop for RegistrationInner {
    fn drop(&mut self) {
        release_node(self.node);
    }
}

#[derive(Clone)]
pub struct SetReadiness {
    inner: RegistrationInner,
}

unsafe impl Send for SetReadiness {}
unsafe impl Sync for SetReadiness {}

pub struct Registration {
    inner: RegistrationInner,
}

impl SetReadiness {
    pub fn readiness(&self) -> Ready {
        self.inner.readiness()
    }

    pub fn set_readiness(&self, ready: Ready) -> io::Result<()> {
        self.inner.set_readiness(ready)
    }
}

impl fmt::Debug for SetReadiness {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("SetReadiness").finish()
    }
}


unsafe impl Send for Registration {}
unsafe impl Sync for Registration {}

impl Registration {
    pub fn new2() -> (Registration, SetReadiness) {
        let node = Box::into_raw(Box::new(ReadinessNode::new(
                ptr::null_mut(),
                TokenEntry {
                    token: Token(0),
                    ttype: TokenType::TokenEvent
                },
                Ready::empty(),
                PollOpt::empty(), 2)));
        let r = Registration {
            inner: RegistrationInner {
                node,
            },
        };
        let s = SetReadiness {
            inner: RegistrationInner {
                node,
            },
        };
        (r, s)
    }

    pub fn new(poll: &Poll, token: TokenEntry, interest: Ready, opt: PollOpt)
            -> (Registration, SetReadiness)
    {
        let (r, s) = Registration::new_priv(poll, token, interest, opt);
        (r, s)
    }

    fn new_priv(poll: &Poll, token: TokenEntry, interest: Ready, opt: PollOpt)
            -> (Registration, SetReadiness)
    {
        is_send::<Registration>();
        is_sync::<Registration>();
        is_send::<SetReadiness>();
        is_sync::<SetReadiness>();
        let queue = poll.readiness_queue.inner.clone();
        let queue1: *mut () = unsafe { mem::transmute(queue) };
        let node = Box::into_raw(Box::new(ReadinessNode::new(queue1, token,
                interest, opt, 3)));
        let r = Registration {
            inner: RegistrationInner {
                node,
            },
        };
        let s = SetReadiness {
            inner: RegistrationInner {
                node,
            },
        };
        (r, s)
    }

    pub fn update(&self, poll: &Poll, token: TokenEntry, interest: Ready,
            opt: PollOpt, job: Job) -> io::Result<()> {
        self.inner.update(poll, token, interest, opt, job)
    }

    pub fn deregister(&self, poll: &Poll) -> io::Result<()> {
        self.inner.update(poll,
            TokenEntry {
                ttype: TokenType::TokenEvent,
                token: Token(0)
            },
            Ready::empty(),
            PollOpt::empty(),
            Arc::new(Mutex::new(|_| {})))
    }
}

impl Evented for Registration {
    fn register(&self, poll: &Poll, token: TokenEntry, interest: Ready,
            opts: PollOpt, job: Job) -> io::Result<()> {
        self.inner.update(poll, token, interest, opts, job)
    }

    fn reregister(&self, poll: &Poll, token: TokenEntry, interest: Ready,
            opts: PollOpt) -> io::Result<()> {
        self.inner.update(poll, token, 
            interest, opts, Arc::new(Mutex::new(|_| {})))
    }

    fn deregister(&self, poll: &Poll) -> io::Result<()> {
        self.inner.update(poll, 
                TokenEntry {
                    ttype: TokenType::TokenEvent,
                    token: Token(0)
                },
                Ready::empty(),
                PollOpt::empty(),
                Arc::new(Mutex::new(|_| {}))
        )
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        if self.inner.state.flag_as_dropped() {
            let _ = self.inner.enqueue_with_wakeup();
        }
    }
}

impl fmt::Debug for Registration {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Registration").finish()
    }
}

