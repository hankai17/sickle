use std::cell::UnsafeCell;
use std::mem;

pub struct LazyCell<T> {
    inner: UnsafeCell<Option<T>>,
}

impl<T> LazyCell<T> {
    pub fn new() -> LazyCell<T> {
        LazyCell { inner: UnsafeCell::new(None) }
    }

    pub fn fill(&self, value: T) -> Result<(), T> {
        let slot = unsafe { &mut *self.inner.get() };
        if slot.is_some() {
            return Err(value);
        }
        *slot = Some(value);
        Ok(())
    }

    #[allow(dead_code)]
    pub fn replace(&mut self, value: T) -> Option<T> {
        mem::replace(
            //unsafe { &mut *self.inner.get() },
            self.inner.get_mut(),
            Some(value)
        )
    }

    pub fn borrow(&self) -> Option<&T> {
        unsafe { &*self.inner.get() }.as_ref()          //  converts from &Option<T> to Option<&T>
    }

    #[allow(dead_code)]
    pub fn filled(&self) -> bool {
        self.borrow().is_some()
    }

    #[allow(dead_code)]
    pub fn borrow_mut(&mut self) -> Option<&mut T> {
        unsafe { &mut *self.inner.get() }.as_mut()      // converts from &mut Option<T> to Option<&mut T>
    }

    #[allow(dead_code)]
    pub fn borrow_with<F: FnOnce() -> T>(&self, f: F) -> &T {
        if let Some(value) = self.borrow() {
            return value;
        }
        let value = f();
        if self.fill(value).is_err() {
            panic!("borrow_with: cell was filled by closure")
        }
        self.borrow().unwrap()
    }

    #[allow(dead_code)]
    pub fn borrow_mut_with<F: FnOnce() -> T>(&mut self, f: F) -> &mut T {
        if !self.filled() {
            let value = f();
            if self.fill(value).is_err() {
                panic!("borrow_mut_with: cell was filled by closure")
            }
        }
        self.borrow_mut().unwrap()
    }

    #[allow(dead_code)]
    pub fn try_borrow_with<E, F>(&self, f: F) -> Result<&T, E>
        where F: FnOnce() -> Result<T, E>
    {
        if let Some(value) = self.borrow() {
            return Ok(value);
        }
        let value = f()?;
        if self.fill(value).is_err() {
            panic!("try_borrow_with: cell was filled by closure")
        }
        Ok(self.borrow().unwrap())
    }

    #[allow(dead_code)]
    pub fn try_borrow_mut_with<E, F>(&mut self, f: F) -> Result<&mut T, E>
        where F: FnOnce() -> Result<T, E>
    {
        if self.filled() {
            return Ok(self.borrow_mut().unwrap());
        }
        let value = f()?;
        if self.fill(value).is_err() {
            panic!("try_borrow_mut_with: cell was filled by closure")
        }
        Ok(self.borrow_mut().unwrap())
    }

    #[allow(dead_code)]
    pub fn into_inner(self) -> Option<T> {
        self.inner.into_inner()
    }
}

impl<T: Copy> LazyCell<T> {
    #[allow(dead_code)]
    pub fn get(&self) -> Option<T> {
        unsafe { *self.inner.get() }
    }
}

