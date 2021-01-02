use std::collections::LinkedList;
use std::ops;

pub struct Slice {
    data: Box<[u8]>,
}

impl Slice {
    pub fn new(size: usize) -> Self {
        Self {
            data: vec![0; size].into_boxed_slice(),
        }
    }
}

impl ops::Deref for Slice {
    type Target = Box<[u8]>;

    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl ops::DerefMut for Slice {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

pub struct Fixed {
    id: usize,
    data: Slice,
}

impl Fixed {
    pub fn new(id: usize, size: usize) -> Self {
        Self {
            id,
            data: Slice::new(size),
        }
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
    pub fn give(&mut self, buffer: Fixed) {
        self.buffers.push_back(buffer);
    }

    pub fn take(&mut self) -> Option<Fixed> {
        // Take from the back because it's more likely to be cached?
        // TODO benchmark
        self.buffers.pop_back()
    }
}

impl Default for Pool {
    fn default() -> Self {
        Self {
            buffers: LinkedList::new(),
        }
    }
}
