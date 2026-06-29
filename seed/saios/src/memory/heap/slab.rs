use crate::memory::constants::MAX_SLAB_RECYCLED_BLOCKS;

#[derive(Debug, Copy, Clone)]
pub struct SlabCache {
    pub block_size: usize,
    recycled: [usize; MAX_SLAB_RECYCLED_BLOCKS],
    recycled_count: usize,
    bump_offset: usize,
}

impl SlabCache {
    pub const fn new(block_size: usize) -> Self {
        Self {
            block_size,
            recycled: [0; MAX_SLAB_RECYCLED_BLOCKS],
            recycled_count: 0,
            bump_offset: 0,
        }
    }

    pub fn recycle(&mut self, offset: usize) {
        if self.recycled_count < MAX_SLAB_RECYCLED_BLOCKS {
            self.recycled[self.recycled_count] = offset;
            self.recycled_count += 1;
        }
    }

    pub fn take_recycled(&mut self) -> Option<usize> {
        if self.recycled_count == 0 {
            return None;
        }

        self.recycled_count -= 1;
        Some(self.recycled[self.recycled_count])
    }

    pub fn next_bump(&mut self) -> Option<usize> {
        let offset = self.bump_offset;
        self.bump_offset = self.bump_offset.checked_add(self.block_size)?;
        Some(offset)
    }
}
