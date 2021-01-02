use std::ops;
use std::collections::LinkedList;

pub type Slice = Box<[u8]>;

pub struct Fixed {
    id: usize,
    data: Slice,
}

impl Fixed {
    pub fn new(id: usize, size: usize) -> Self {
        Self{id, data: vec![0; size].into_boxed_slice()}
    }

    pub fn id(&self) -> usize {
        self.id
    }
}

impl ops::Deref for Fixed {
    type Target = Slice;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl ops::DerefMut for Fixed {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

pub struct Pool {
    buffers: LinkedList<Fixed>,
}

impl Pool {
    pub fn new() -> Self {
        Self{buffers: LinkedList::new()}
    }

    pub fn give(&mut self, buffer: Fixed) {
        self.buffers.push_back(buffer);
    }

    pub fn take(&mut self) -> Option<Fixed> {
        // Take from the back because it's more likely to be cached?
        // TODO benchmark
        self.buffers.pop_back()
    }
}
