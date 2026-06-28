use crate::memory::constants::FRAME_BITMAP_WORDS;

#[derive(Copy, Clone)]
pub struct FrameBitmap {
    words: [u64; FRAME_BITMAP_WORDS],
}

impl FrameBitmap {
    pub const fn new() -> Self {
        Self {
            words: [0; FRAME_BITMAP_WORDS],
        }
    }

    pub fn clear_all(&mut self) {
        self.words.fill(0);
    }

    pub fn set(&mut self, index: usize) {
        let word = index / 64;
        let bit = index % 64;
        self.words[word] |= 1u64 << bit;
    }

    pub fn clear(&mut self, index: usize) {
        let word = index / 64;
        let bit = index % 64;
        self.words[word] &= !(1u64 << bit);
    }

    pub fn is_set(&self, index: usize) -> bool {
        let word = index / 64;
        let bit = index % 64;
        (self.words[word] & (1u64 << bit)) != 0
    }
}
