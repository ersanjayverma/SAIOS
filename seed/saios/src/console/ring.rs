const RING_BYTES: usize = 16 * 1024;

pub struct LogRing {
    buf: [u8; RING_BYTES],
    head: usize,
    len: usize,
}

impl LogRing {
    pub const fn new() -> Self {
        Self {
            buf: [0; RING_BYTES],
            head: 0,
            len: 0,
        }
    }

    pub fn append(&mut self, text: &str) {
        for b in text.bytes() {
            self.buf[self.head] = b;
            self.head = (self.head + 1) % RING_BYTES;
            if self.len < RING_BYTES {
                self.len += 1;
            }
        }
    }

    pub fn replay<F: FnMut(u8)>(&self, mut emit: F) {
        if self.len == 0 {
            return;
        }

        let start = if self.len == RING_BYTES { self.head } else { 0 };

        for i in 0..self.len {
            let idx = (start + i) % RING_BYTES;
            emit(self.buf[idx]);
        }
    }
}
