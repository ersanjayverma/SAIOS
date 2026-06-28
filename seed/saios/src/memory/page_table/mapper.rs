use crate::memory::types::{AddressSpaceId, PhysicalFrame, VirtAddr};
use crate::memory::vmm::flags::PageFlags;
#[derive(Copy, Clone)]
pub struct MappingEntry {
    pub active: bool,
    pub owner: AddressSpaceId,
    pub virt: VirtAddr,
    pub frame: PhysicalFrame,
    pub flags: PageFlags,
}

impl MappingEntry {
    pub const fn empty() -> Self {
        Self {
            active: false,
            owner: AddressSpaceId::KERNEL,
            virt: VirtAddr::new(0),
            frame: PhysicalFrame::new(0),
            flags: PageFlags::empty(),
        }
    }
}
