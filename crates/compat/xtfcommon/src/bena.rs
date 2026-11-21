use crate::sys::*;
use core::sync::atomic::{AtomicI32, Ordering};
use std::sync::Mutex;

pub struct Benaphore {
    sem: HANDLE,
    count: AtomicI32,
}

unsafe impl Send for Benaphore {}
unsafe impl Sync for Benaphore {}

impl Benaphore {
    pub fn new() -> Self {
        let sem = unsafe { CreateSemaphoreW(core::ptr::null_mut(), 0, 1, core::ptr::null()) };
        Benaphore { sem, count: AtomicI32::new(0) }
    }

    pub fn lock(&self) {
        if self.count.fetch_add(1, Ordering::AcqRel) + 1 > 1 {
            unsafe { WaitForSingleObject(self.sem, INFINITE) };
        }
    }

    pub fn unlock(&self) {
        if self.count.fetch_sub(1, Ordering::AcqRel) - 1 > 0 {
            unsafe { ReleaseSemaphore(self.sem, 1, core::ptr::null_mut()) };
        }
    }

    pub fn counter(&self) -> i32 {
        self.count.load(Ordering::Acquire)
    }
}

impl Default for Benaphore {
    fn default() -> Self {
        Benaphore::new()
    }
}

impl Drop for Benaphore {
    fn drop(&mut self) {
        if !self.sem.is_null() {
            unsafe { CloseHandle(self.sem) };
        }
    }
}

pub type AssertHook = fn(file: &'static [u8], func: &'static [u8], line: u32, expr: &'static [u8]);

pub struct CheckedBenaphore {
    inner: Benaphore,
    owners: Mutex<Vec<u32>>,
    hook: AssertHook,
}

const CONC_FILE: &[u8] = b"xbox\\devex\\xtf\\lib\\xtfbase\\xtfbaseconcurrency_stl.cpp\0";

impl CheckedBenaphore {
    pub fn new(hook: AssertHook) -> Self {
        CheckedBenaphore { inner: Benaphore::new(), owners: Mutex::new(Vec::new()), hook }
    }

    pub fn lock(&self) {
        let tid = unsafe { GetCurrentThreadId() };
        let added = {
            let mut o = self.owners.lock().unwrap();
            if o.contains(&tid) {
                false
            } else {
                o.push(tid);
                true
            }
        };
        if !added {
            (self.hook)(CONC_FILE, b"xtfbase::BenaphoreNonRecursiveChecked::Lock\0", 0x18, b"AddThreadID(GetCurrentThreadId())\0");
        }
        self.inner.lock();
    }

    pub fn unlock(&self) {
        let tid = unsafe { GetCurrentThreadId() };
        let removed = {
            let mut o = self.owners.lock().unwrap();
            match o.iter().position(|&t| t == tid) {
                Some(i) => {
                    o.remove(i);
                    true
                }
                None => false,
            }
        };
        if !removed {
            (self.hook)(CONC_FILE, b"xtfbase::BenaphoreNonRecursiveChecked::Unlock\0", 0x1e, b"RemoveThreadID(GetCurrentThreadId())\0");
        }
        self.inner.unlock();
    }

    pub fn held_by_current_thread(&self) -> bool {
        let tid = unsafe { GetCurrentThreadId() };
        self.owners.lock().unwrap().contains(&tid)
    }
}
