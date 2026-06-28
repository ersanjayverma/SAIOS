use crate::memory::page_table::entry::PageTableEntry;
use crate::memory::types::PhysAddr;
use crate::memory::vmm::flags::PageFlags;
pub struct PageTable {
    pub entries: [PageTableEntry; 512],
}

impl PageTable {
    pub const fn new() -> Self {
        Self {
            entries: [PageTableEntry::new(); 512],
        }
    }
    pub fn entry(&self, index: usize) -> &PageTableEntry {
        &self.entries[index]
    }

    pub fn entry_mut(&mut self, index: usize) -> &mut PageTableEntry {
        &mut self.entries[index]
    }
    pub fn present(&self, index: usize) -> bool {
        self.entries[index].present()
    }

    pub fn writable(&self, index: usize) -> bool {
        self.entries[index].writable()
    }

    pub fn user(&self, index: usize) -> bool {
        self.entries[index].user()
    }

    pub fn frame(&self, index: usize) -> PhysAddr {
        self.entries[index].frame()
    }

    pub fn set_frame(&mut self, index: usize, frame: PhysAddr) {
        self.entries[index].set_frame(frame)
    }

    pub fn clear(&mut self, index: usize) {
        self.entries[index].clear()
    }
    pub fn set_writable(&mut self, index: usize, writable: bool) {
        self.entries[index].set_writable(writable);
    }
    pub fn set_user(&mut self, index: usize, user: bool) {
        self.entries[index].set_user(user);
    }

    pub fn set_flags(&mut self, index: usize, flags: PageFlags) {
        self.entries[index].set_flags(flags);
    }
}
