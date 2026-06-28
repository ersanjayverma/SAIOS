#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct HeapStats {
    pub total_bytes: usize,
    pub used_bytes: usize,
    pub free_bytes: usize,
    pub active_allocations: usize,
    pub failed_allocations: usize,
}

impl HeapStats {
    pub const fn empty(total_bytes: usize) -> Self {
        Self {
            total_bytes,
            used_bytes: 0,
            free_bytes: total_bytes,
            active_allocations: 0,
            failed_allocations: 0,
        }
    }
}
