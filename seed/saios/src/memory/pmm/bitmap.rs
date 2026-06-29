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

    /// Set all bits in the range `[start, end)` using word-level operations.
    /// Returns the count of bits that were *newly* set (were 0 before).
    ///
    /// `start` and `end` are clamped to `[0, MAX_TRACKED_FRAMES)`.
    pub fn set_range(&mut self, start: usize, end: usize) -> usize {
        let start = start.min(MAX_TRACKED_FRAMES);
        let end = end.min(MAX_TRACKED_FRAMES);
        if start >= end {
            return 0;
        }

        let first_word = start / 64;
        // last_word is the index of the word containing the final frame.
        // When `end` sits on a word boundary, the last frame is in the
        // previous word, so we use (end - 1) / 64.
        let last_word = (end - 1) / 64;
        let mut newly_set = 0usize;

        if first_word == last_word {
            // Range fits within a single word.
            // If end lands on a word boundary, the "bit position" is 64
            // (the whole word), not 0 (which would mean an empty range).
            let hi = if end % 64 == 0 { 64 } else { end % 64 };
            let mask = bitmask_range(start % 64, hi);
            let old = self.words[first_word];
            self.words[first_word] |= mask;
            newly_set += (mask & !old).count_ones() as usize;
        } else {
            // First partial word
            let first_mask = bitmask_from(start % 64);
            let old = self.words[first_word];
            self.words[first_word] |= first_mask;
            newly_set += (first_mask & !old).count_ones() as usize;

            // Full words in the middle.
            // When `end` is word-aligned the last word is also full and
            // must be included in this loop.
            let full_end = if end % 64 == 0 { last_word + 1 } else { last_word };
            for w in (first_word + 1)..full_end {
                let old = self.words[w];
                self.words[w] = !0u64;
                newly_set += (!old).count_ones() as usize;
            }

            // Last partial word — only when end is NOT word-aligned
            if end % 64 != 0 {
                let last_mask = bitmask_to(end % 64);
                let old = self.words[last_word];
                self.words[last_word] |= last_mask;
                newly_set += (last_mask & !old).count_ones() as usize;
            }
        }

        newly_set
    }
}

/// Return a mask with bits [lo, hi) set within a single 64-bit word.
/// `lo` and `hi` are bit positions (0..64).  `hi == 64` means "all bits
/// from lo to the end of the word".
#[inline]
fn bitmask_range(lo: usize, hi: usize) -> u64 {
    if lo >= hi {
        return 0;
    }
    let len = hi - lo;
    if len >= 64 {
        return !0u64 << lo;
    }
    ((1u64 << len) - 1) << lo
}

/// Return a mask with bits [lo, 64) set.
#[inline]
fn bitmask_from(lo: usize) -> u64 {
    if lo >= 64 {
        return 0;
    }
    !0u64 << lo
}

/// Return a mask with bits [0, hi) set. Returns 0 if hi == 0.
#[inline]
fn bitmask_to(hi: usize) -> u64 {
    if hi == 0 {
        return 0;
    }
    if hi >= 64 {
        return !0u64;
    }
    (1u64 << hi) - 1
}
