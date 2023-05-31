use std::{sync::atomic::{AtomicI32, Ordering, AtomicU32, AtomicUsize}, cell::UnsafeCell};

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
    size: usize,
    next_id: AtomicUsize,
}

impl StorageHdr {
    pub fn new(size: usize) -> Self {
        Self {
            size,
            next_id: AtomicUsize::new(0),
        }
    }
}


#[repr(C)]
pub struct ItemHdr {
    lock: Lock,
    refcount: AtomicU32,
    id: usize,
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
pub struct Token {
    // current position in the ring buffer is defined as id % size
    id: usize,
}

#[repr(C)]
pub struct Storage<'a, T: 'a> {
    header: &'a StorageHdr,
    items: &'a [UnsafeCell<T>],
    item_hdrs: &'a [ItemHdr],
}

impl<'a, T> Storage<'a, T> {
    pub fn new(header: &'a StorageHdr,
        items: &'a [UnsafeCell<T>], item_hdrs: &'a [ItemHdr]) -> Self {
        Self {
            header,
            items,
            item_hdrs,
        }
    }
    pub fn size(&self) -> usize {
        self.header.size
    }

    pub fn put(&self, f: impl FnOnce(&mut T) -> bool ) -> Option<Token> {
        let id_start = self.header.next_id.fetch_add(1, Ordering::Relaxed);
        // try all items in the ring buffer starting from pos, wrapping around the end and finishing at pos-1
        for i in 0..self.header.size {
            let id = id_start + i;
            let pos = (id_start + i) % self.header.size;
            let hdr = &self.item_hdrs[pos];
            if let Some(item) = hdr.lock.create(|| {
                Some(unsafe { &mut *self.items[pos].get() })
            }) {
                // Token is not given to anyone yet so it's safe to access the data outside of the write lock
                f(item);
                return Some(Token { id});
            }
        }
        // no free slots
        None
    }

    pub fn get<R>(&self, token: Token, f: impl FnOnce(&T)-> R) -> Option<R> {
        let pos = token.id % self.header.size;
        let hdr = &self.item_hdrs[pos];
        hdr.lock.read(|| {
            f(unsafe { &*self.items[pos].get() })
        })
    }
}

#[test]
fn test_init_storage() {
    let header = StorageHdr::new(10);
    let items = [(); 10] .map(|_| UnsafeCell::new(0));
    let item_hdrs = [(); 10].map(|_| ItemHdr::default());
    let storage = Storage::new(&header, &items, &item_hdrs);
    assert_eq!(storage.size(), 10);
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
