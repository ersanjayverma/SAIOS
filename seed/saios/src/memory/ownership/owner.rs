use crate::memory::types::AddressSpaceId;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Owner {
    Pmm,
    AddressSpace(AddressSpaceId),
    KernelHeap,
    Driver(u16),
    Dma(u16),
    PageTable(AddressSpaceId),
    SharedMemory(u16),
}
