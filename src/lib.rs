use std::sync::atomic::AtomicI32;

#[repr(C)]
pub struct Storage<'a, T: 'a> {
    size: usize,
    data: &'a mut [T],
    locks: &'a mut [AtomicI32],
}

impl<'a, T> Storage<'a, T> {
    pub fn new<const SIZE: usize>(
        data: &'a mut [T; SIZE],
        locks: &'a mut [AtomicI32; SIZE],
    ) -> Self {
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
    let mut locks = [(); 10].map(|_| AtomicI32::new(0));
    let storage = Storage::new(&mut data, &mut locks);
    assert_eq!(storage.size, 10);
}
