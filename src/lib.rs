use std::sync::atomic::{AtomicBool, AtomicI32};

#[repr(C)]
pub struct Lock {
    // refcount == -1 means write lock
    refcount: AtomicI32,
}

impl Default for Lock {
    fn default() -> Self {
        Self {
            refcount: AtomicI32::new(0),
        }
    }
}

impl Lock {
    // Tries to acquire a write lock.
    // If reference count is positive or another writer is active (refcount == -1), returns None.
    // Otherwirs acquires the lock, calls write function f, sets reference count to 1 if f returns Some, and returns f's result.
    pub fn write<R>(&self, f: impl FnOnce() -> Option<R>) -> Option<R> {
        if self.refcount.compare_exchange_weak(
            0,
            -1,
            std::sync::atomic::Ordering::Acquire,
            std::sync::atomic::Ordering::Relaxed,
        ) == Ok(0)
        {
            let r = f();
            self.refcount.store(
                if r.is_some() { 1 } else { 0 },
                std::sync::atomic::Ordering::Release,
            );
            r
        } else {
            None
        }
    }
}

#[repr(C)]
pub struct Storage<'a, T: 'a> {
    size: usize,
    data: &'a [T],
    locks: &'a [Lock],
}

impl<'a, T> Storage<'a, T> {
    pub fn new<const SIZE: usize>(data: &'a [T; SIZE], locks: &'a [Lock; SIZE]) -> Self {
        Self {
            size: SIZE,
            data,
            locks,
        }
    }
}

#[test]
fn test() {
    let mut data = [0; 10];
    let mut locks = [(); 10].map(|_| Lock::default());
    let storage = Storage::new(&mut data, &mut locks);
    assert_eq!(storage.size, 10);
}
