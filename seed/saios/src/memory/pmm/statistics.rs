#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PhysicalMemoryStats {
    pub total_bytes: usize,
    pub free_bytes: usize,
    pub reserved_bytes: usize,
    pub allocated_frames: usize,
}

impl PhysicalMemoryStats {
    pub const fn empty() -> Self {
        Self {
            total_bytes: 0,
            free_bytes: 0,
            reserved_bytes: 0,
            allocated_frames: 0,
        }
    }
}
