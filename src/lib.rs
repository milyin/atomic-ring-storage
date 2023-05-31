use std::sync::atomic::{AtomicI32, Ordering};

#[repr(C)]
pub struct Lock {
    // refcount == -1 means write lock
    // refcount == 0 means protected data is not in use and can be owerwritten at any moment
    // refcount > 0 means protected data exists and can be read. To safely read it refcount must be incremented first and then decremented when read is finished
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
        if self
            .refcount
            .compare_exchange_weak(0, -1, Ordering::Acquire, Ordering::Relaxed)
            == Ok(0)
        {
            let r = f();
            self.refcount
                .store(if r.is_some() { 1 } else { 0 }, Ordering::Release);
            r
        } else {
            None
        }
    }
    // Tries to acquire a read lock.
    // If writer is active (refcount == -1) or there is no data (refcount == 0), returns None.
    // Otherwise increments reference count, calls read function f, decrements reference count and returns Some with f's result.
    pub fn read<R>(&self, f: impl FnOnce() -> R) -> Option<R> {
        if self
            .refcount
            .fetch_update(Ordering::Acquire, Ordering::Relaxed, |x| {
                if x > 0 {
                    Some(x + 1)
                } else {
                    None
                }
            })
            .is_err()
        {
            return None;
        }
        let r = f();
        self.refcount.fetch_sub(1, Ordering::Release);
        Some(r)
    }
    // Revmoes the data if reference count is 1, i.e. the data exists and not readed or written at the moment.
    pub fn remove(&self) -> bool {
        self.refcount
            .compare_exchange_weak(1, 0, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
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
