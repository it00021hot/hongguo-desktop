use std::{
    ops::Deref,
    sync::{Arc, Mutex, Weak},
};

#[derive(Default)]
struct FreeBuffers {
    buffers: Vec<Vec<u8>>,
    bytes: usize,
    budget: usize,
    reused: u64,
    allocated: u64,
}

#[derive(Default)]
pub struct FrameBufferPool(Arc<Mutex<FreeBuffers>>);

impl FrameBufferPool {
    pub fn set_budget(&self, budget: usize) {
        let mut free = self.0.lock().expect("frame buffer pool");
        if free.budget != budget {
            free.buffers.clear();
            free.bytes = 0;
            free.budget = budget;
        }
    }

    pub fn take(&self, capacity: usize) -> FrameBytes {
        let recycled = {
            let mut free = self.0.lock().expect("frame buffer pool");
            let index = free
                .buffers
                .iter()
                .position(|buffer| buffer.capacity() == capacity);
            match index {
                Some(index) => {
                    let buffer = free.buffers.remove(index);
                    free.bytes -= buffer.capacity();
                    free.reused += 1;
                    Some(buffer)
                }
                None => {
                    free.allocated += 1;
                    None
                }
            }
        };
        FrameBytes {
            bytes: recycled.unwrap_or_else(|| Vec::with_capacity(capacity)),
            pool: Arc::downgrade(&self.0),
        }
    }
}

impl std::fmt::Debug for FrameBufferPool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let free = self.0.lock().expect("frame buffer pool");
        formatter
            .debug_struct("FrameBufferPool")
            .field("reused", &free.reused)
            .field("allocated", &free.allocated)
            .field("idleBytes", &free.bytes)
            .field("budgetBytes", &free.budget)
            .finish()
    }
}

pub struct FrameBytes {
    bytes: Vec<u8>,
    pool: Weak<Mutex<FreeBuffers>>,
}

impl FrameBytes {
    pub fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }
}

impl From<Vec<u8>> for FrameBytes {
    fn from(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            pool: Weak::new(),
        }
    }
}

impl Deref for FrameBytes {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.bytes
    }
}

impl std::fmt::Debug for FrameBytes {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FrameBytes")
            .field("len", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

impl Drop for FrameBytes {
    fn drop(&mut self) {
        let Some(pool) = self.pool.upgrade() else {
            return;
        };
        let mut free = pool.lock().expect("frame buffer pool");
        let capacity = self.bytes.capacity();
        if capacity == 0 || capacity > free.budget {
            return;
        }
        while capacity > free.budget - free.bytes {
            let oldest = free.buffers.remove(0);
            free.bytes -= oldest.capacity();
        }
        self.bytes.clear();
        free.bytes += capacity;
        free.buffers.push(std::mem::take(&mut self.bytes));
    }
}
