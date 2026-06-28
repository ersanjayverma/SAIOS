use hal::memory::x86_64::X64Paging;
use hal::memory::{MmuHal, VirtAddr};

pub use hal::memory::PagingRoot;
pub use hal::memory::PageFlags;

static PAGING: X64Paging = X64Paging::new();

pub fn active_root() -> PagingRoot {
    PAGING.active_root()
}

pub unsafe fn switch_root(root: PagingRoot) {
    unsafe { PAGING.switch_root(root) }
}

pub fn flush(address: VirtAddr) {
    PAGING.flush(address);
}

pub fn flush_all() {
    PAGING.flush_all();
}

pub fn page_size() -> usize {
    PAGING.page_size()
}

pub fn supports_nx() -> bool {
    PAGING.supports_nx()
}

pub fn supports_huge_pages() -> bool {
    PAGING.supports_huge_pages()
}

pub fn supports_1g_pages() -> bool {
    PAGING.supports_1g_pages()
}

pub fn supports_pcid() -> bool {
    PAGING.supports_pcid()
}
