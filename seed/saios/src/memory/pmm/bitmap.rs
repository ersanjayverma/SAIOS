use crate::memory::constants::{FRAME_BITMAP_WORDS, MAX_TRACKED_FRAMES};

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

    /// Set the bit for a given frame number
    /// Returns Err(()) if index is out of bounds
    pub fn set(&mut self, index: usize) -> Result<(), ()> {
        if index >= MAX_TRACKED_FRAMES {
            return Err(());  // Out of bounds
        }
        let word = index / 64;
        let bit = index % 64;
        self.words[word] |= 1u64 << bit;
        Ok(())
    }

    /// Clear the bit for a given frame number
    /// Returns Err(()) if index is out of bounds
    pub fn clear(&mut self, index: usize) -> Result<(), ()> {
        if index >= MAX_TRACKED_FRAMES {
            return Err(());  // Out of bounds
        }
        let word = index / 64;
        let bit = index % 64;
        self.words[word] &= !(1u64 << bit);
        Ok(())
    }

    /// Check if the bit for a given frame number is set
    /// Returns false if index is out of bounds
    pub fn is_set(&self, index: usize) -> bool {
        if index >= MAX_TRACKED_FRAMES {
            return false;  // Out of bounds
        }
        let word = index / 64;
        let bit = index % 64;
        (self.words[word] & (1u64 << bit)) != 0
    }
}
