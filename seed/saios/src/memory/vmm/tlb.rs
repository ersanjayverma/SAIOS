use crate::memory::types::VirtAddr;

pub fn flush(address: VirtAddr) {
    crate::memory::vmm::paging::flush(address);
}

pub fn flush_all() {
    crate::memory::vmm::paging::flush_all();
}
