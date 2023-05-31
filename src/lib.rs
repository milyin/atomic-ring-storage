use std::sync::atomic::{AtomicI32, Ordering, AtomicU32};

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
pub struct StorageHdr {
    size: u32,
    // position to allocate next element
    pos: AtomicU32,
}

impl StorageHdr {
    pub fn new(size: u32) -> Self {
        Self {
            size,
            pos: AtomicU32::new(0),
        }
    }
}


#[repr(C)]
pub struct ItemHdr {
    lock: Lock,
    refcount: AtomicU32,
    id: u64,
}

impl Default for ItemHdr {
    fn default() -> Self {
        Self {
            lock: Lock::default(),
            refcount: AtomicU32::new(0),
            id: 0,
        }
    }
}

#[repr(C)]
pub struct Storage<'a, T: 'a> {
    header: &'a StorageHdr,
    items: &'a [T],
    item_hdrs: &'a [ItemHdr],
}

impl<'a, T> Storage<'a, T> {
    pub fn new(header: &'a StorageHdr,
        items: &'a [T], item_hdrs: &'a [ItemHdr]) -> Self {
        Self {
            header,
            items,
            item_hdrs,
        }
    }
    pub fn len(&self) -> usize {
        self.header.size as usize
    }
}

#[test]
fn test_init_storage() {
    let header = StorageHdr::new(10);
    let items = [0; 10];
    let item_hdrs = [(); 10].map(|_| ItemHdr::default());
    let storage = Storage::new(&header, &items, &item_hdrs);
    assert_eq!(storage.len(), 10);
}

#[test]
fn test_lock_api() {
    let lock = Lock::default();
    assert_eq!(lock.refcount.load(Ordering::Relaxed), 0);
    assert_eq!(lock.create(|| Some(1)), Some(1));
    assert_eq!(lock.refcount.load(Ordering::Relaxed), 1);
    assert_eq!(lock.create(|| Some(2)), None);
    assert_eq!(lock.refcount.load(Ordering::Relaxed), 1);
    assert_eq!(lock.update(|| Some(3)), Some(3));
    assert_eq!(lock.refcount.load(Ordering::Relaxed), 1);
    assert_eq!(lock.update(|| Option::<()>::None), None);
    assert_eq!(lock.refcount.load(Ordering::Relaxed), 0);

    assert_eq!(
        lock.create(|| {
            assert_eq!(lock.refcount.load(Ordering::Relaxed), -1);
            Some(4)
        }),
        Some(4)
    );
    assert_eq!(
        lock.read(|| {
            assert_eq!(lock.refcount.load(Ordering::Relaxed), 2);
            5
        }),
        Some(5)
    );
    assert_eq!(lock.refcount.load(Ordering::Relaxed), 1);
    assert_eq!(
        lock.update(|| {
            assert_eq!(lock.refcount.load(Ordering::Relaxed), -1);
            Option::<()>::None
        }),
        None
    );
}
