use std::sync::atomic::{AtomicBool, AtomicI32};

#[repr(C)]
pub struct Lock {
    write: AtomicBool,
    refcount: AtomicI32,
}

impl Default for Lock {
    fn default() -> Self {
        Self {
            write: AtomicBool::new(false),
            refcount: AtomicI32::new(0),
        }
    }
}

#[repr(C)]
pub struct Storage<'a, T: 'a> {
    size: usize,
    data: &'a mut [T],
    locks: &'a mut [Lock],
}

impl<'a, T> Storage<'a, T> {
    pub fn new<const SIZE: usize>(data: &'a mut [T; SIZE], locks: &'a mut [Lock; SIZE]) -> Self {
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
