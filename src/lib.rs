use std::{
    cell::UnsafeCell,
    sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering},
};

#[repr(C)]
pub struct Lock {
    // -1 means write lock, no operation is allowed
    // 0 means no locks, any operation is allowed
    // > 1 means numeber of read locks, only read operations are allowed
    rwlock: AtomicI32,
}

impl Default for Lock {
    fn default() -> Self {
        Self {
            rwlock: AtomicI32::new(0),
        }
    }
}

impl Lock {
    // Tries to acquire a write lock
    // If data is being read or written (rwlock != 0), returns None.
    // IF data is not being read or written (rwlock = 0):
    // - acquires the write lock (set rwlock to -1)
    // - calls update function f
    // - releases the lock (set rwlock to 0)
    // - returns Some of f's result.
    pub fn write<R>(&self, f: impl FnOnce() -> R) -> Option<R> {
        if self
            .rwlock
            .compare_exchange_weak(0, -1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            let r = f();
            self.rwlock.store(0, Ordering::Release);
            Some(r)
        } else {
            None
        }
    }
    // Tries to acquire a read lock
    // If writer is active (rwlock == -1), returns None.
    // Otherwise increments rwlock count, calls read function f, decrements rwlock and returns Some with f's result.
    pub fn read<R>(&self, f: impl FnOnce() -> R) -> Option<R> {
        if self
            .rwlock
            .fetch_update(Ordering::Acquire, Ordering::Relaxed, |x| {
                if x < 0 {
                    None
                } else {
                    Some(x + 1)
                }
            })
            .is_err()
        {
            return None;
        }
        let r = f();
        self.rwlock.fetch_sub(1, Ordering::Release);
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
    refcount: AtomicI32,
    id: AtomicUsize,
}

impl Default for ItemHdr {
    fn default() -> Self {
        Self {
            lock: Lock::default(),
            refcount: AtomicI32::new(0),
            id: AtomicUsize::new(0),
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
    pub fn new(
        header: &'a StorageHdr,
        items: &'a [UnsafeCell<T>],
        item_hdrs: &'a [ItemHdr],
    ) -> Self {
        Self {
            header,
            items,
            item_hdrs,
        }
    }
    pub fn size(&self) -> usize {
        self.header.size
    }

    pub fn put(&self, f: impl FnOnce(&mut T) -> bool) -> Option<Token> {
        // try all items in the ring buffer starting from pos, wrapping around the end and finishing at pos-1
        for i in 0..self.header.size {
            let id = self.header.next_id.fetch_add(1, Ordering::Relaxed);
            let pos = id % self.header.size;
            let hdr = &self.item_hdrs[pos];
            // If item is referenced by someone else, skip it
            // Negative refcount is not normally expected, but possible if someone decremented refcount too much
            if hdr.refcount.load(Ordering::Acquire) <= 0 {
                // Item is free, but it still might be read or write locked by someone else
                if let Some(item) = hdr.lock.write(|| unsafe { &mut *self.items[pos].get() }) {
                    // Token is not given to anyone yet so it's safe to access the data outside of the write lock
                    f(item);
                    hdr.refcount.store(1, Ordering::Relaxed);
                    hdr.id.store(id, Ordering::Release);
                    return Some(Token { id });
                }
            }
        }
        // no free slots found
        None
    }

    pub fn get<R>(&self, token: Token, f: impl FnOnce(&T) -> R) -> Option<R> {
        let pos = token.id % self.header.size;
        let hdr = &self.item_hdrs[pos];
        let id = hdr.id.load(Ordering::Acquire);
        if token.id != id {
            return None;
        }
        hdr.lock.read(|| f(unsafe { &*self.items[pos].get() }))
    }

    pub fn incref(&self, token: Token) -> Option<i32> {
        let pos = token.id % self.header.size;
        let hdr = &self.item_hdrs[pos];
        let id = hdr.id.load(Ordering::Acquire);
        hdr.lock
            .read(|| hdr.refcount.fetch_add(1, Ordering::Release) + 1)
    }

    pub fn decref(&self, token: Token) -> Option<i32> {
        let pos = token.id % self.header.size;
        let hdr = &self.item_hdrs[pos];
        let id = hdr.id.load(Ordering::Acquire);
        hdr.lock
            .read(|| hdr.refcount.fetch_sub(1, Ordering::Release) - 1)
    }
}

#[test]
fn test_init_storage() {
    let header = StorageHdr::new(10);
    let items = [(); 10].map(|_| UnsafeCell::new(0));
    let item_hdrs = [(); 10].map(|_| ItemHdr::default());
    let storage = Storage::new(&header, &items, &item_hdrs);
    assert_eq!(storage.size(), 10);
}

#[test]
fn test_lock_api() {
    let lock = Lock::default();
    assert_eq!(lock.rwlock.load(Ordering::Relaxed), 0);
    assert_eq!(
        lock.write(|| {
            assert_eq!(lock.rwlock.load(Ordering::Relaxed), -1);
        }),
        Some(())
    );
    assert_eq!(
        lock.read(|| {
            assert_eq!(lock.rwlock.load(Ordering::Relaxed), 1);
        }),
        Some(())
    );
    assert_eq!(lock.rwlock.load(Ordering::Relaxed), 0);
}
