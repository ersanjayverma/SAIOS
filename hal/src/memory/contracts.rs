use crate::memory::{PageFlags, PagingRoot, VirtAddr};

pub trait MmuHal {
    fn active_root(&self) -> PagingRoot;

    /// # Safety
    ///
    /// The supplied root must reference a valid, fully initialized page-table
    /// hierarchy for the current architecture. Switching to an invalid root may
    /// immediately fault the CPU or leave address translation undefined.
    unsafe fn switch_root(&self, root: PagingRoot);

    fn flush(&self, address: VirtAddr);
    fn flush_all(&self);
    fn page_size(&self) -> usize;
    fn supports_nx(&self) -> bool;
    fn supports_huge_pages(&self) -> bool;
    fn supports_1g_pages(&self) -> bool;
    fn supports_pcid(&self) -> bool;

    fn sanitize_page_flags(&self, requested: PageFlags) -> PageFlags {
        let mut flags = requested;
        if !self.supports_nx() && flags.contains(PageFlags::NO_EXECUTE) {
            flags = PageFlags::empty();
            flags |= requested;
        }
        flags
    }
}