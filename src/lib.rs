use std::sync::atomic::{AtomicI32, Ordering};

#[repr(C)]
pub struct Lock {
    // refcount == -1 means write lock, no operation is allowed
    // refcount == 0 means protected data is empty, only 'create' operation is allowed
    // refcount == 1 means protected data exists and not in use, 'update' and 'read' operations are allowed
    // refcount > 1 means protected data exists and is being read, only 'read' operation is allowed
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
    // Tries to acquire a write lock on empty data (refcount == 0).
    // If data is not empty (refcount > 0) or another writer is active (refcount == -1), returns None.
    // If data is empty (refcount == 0):
    // - acquires the write lock (set refcount to -1)
    // - calls write function f
    //   - sets reference count to 1 if f returns Some
    //   - sets reference count to 0 if f returns None
    // - returns f's result.
    pub fn create<R>(&self, f: impl FnOnce() -> Option<R>) -> Option<R> {
        if self
            .refcount
            .compare_exchange_weak(0, -1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            let r = f();
            self.refcount
                .store(if r.is_some() { 1 } else { 0 }, Ordering::Release);
            r
        } else {
            None
        }
    }
    // Tries to acquire a write lock on non-empty data (refcount == 1).
    // If data is empty (refcount == 0) or another writer is active (refcount == -1), or data is being read (refcount > 1) returns None.
    // IF data is not empty and not being read (refcount == 1):
    // - acquires the write lock (set refcount to -1)
    // - calls update function f
    //   - sets reference count to 1 if f returns Some
    //   - sets reference count to 0 if f returns None
    // - returns f's result.
    pub fn update<R>(&self, f: impl FnOnce() -> Option<R>) -> Option<R> {
        if self
            .refcount
            .compare_exchange_weak(1, -1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            let r = f();
            self.refcount
                .store(if r.is_some() { 1 } else { 0 }, Ordering::Release);
            r
        } else {
            None
        }
    }
    // Tries to acquire a read lock on non-empty data (refcount > 0).
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
