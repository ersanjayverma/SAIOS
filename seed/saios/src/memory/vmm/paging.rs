pub use hal::memory::PageFlags;
pub use hal::memory::PagingRoot;
use hal::memory::x86_64::X64Paging;
use hal::memory::{MmuHal, VirtAddr};

static PAGING: spin::Once<X64Paging> = spin::Once::new();

pub fn init() {
    PAGING.call_once(X64Paging::new);
}
fn paging() -> &'static X64Paging {
    PAGING.get().expect("Paging not initialized")
}

pub fn active_root() -> PagingRoot {
    paging().active_root()
}

pub unsafe fn switch_root(root: PagingRoot) {
    unsafe { paging().switch_root(root) }
}

pub fn flush(address: VirtAddr) {
    paging().flush(address);
}

pub fn flush_all() {
    paging().flush_all();
}

pub fn page_size() -> usize {
    paging().page_size()
}

pub fn supports_nx() -> bool {
    paging().cpu_features().nx
}

pub fn supports_huge_pages() -> bool {
    paging().cpu_features().huge_pages
}

pub fn supports_1g_pages() -> bool {
    paging().cpu_features().page1g
}

pub fn supports_pcid() -> bool {
    paging().cpu_features().pcid
}
