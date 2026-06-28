use crate::arch::x86_64::paging::PagingRoot;
use crate::memory::constants::KERNEL_SPACE_START;
use crate::memory::errors::MemoryError;
use crate::memory::page_table::entry::PageTableEntry;
use crate::memory::page_table::table::PageTable;
use crate::memory::types::PhysicalFrame;
use crate::memory::types::{PhysAddr, VirtAddr};
#[inline]
fn phys_to_virt(addr: PhysAddr) -> *mut u8 {
    (addr.as_u64() + KERNEL_SPACE_START) as *mut u8
}
pub struct PageWalker;
pub struct WalkResult {
    pub pml4: *mut PageTable,
    pub pdpt: *mut PageTable,
    pub pd: *mut PageTable,
    pub pt: *mut PageTable,

    pub pml4_index: usize,
    pub pdpt_index: usize,
    pub pd_index: usize,
    pub pt_index: usize,
    pub leaf: *mut PageTableEntry,
}
#[derive(Copy, Clone, Eq, PartialEq)]
enum WalkMode {
    Lookup,
    Allocate,
}

pub struct VirtualAddressParts {
    pub pml4: usize,
    pub pdpt: usize,
    pub pd: usize,
    pub pt: usize,
    pub offset: usize,
}
impl VirtualAddressParts {
    pub fn from(addr: VirtAddr) -> Self {
        let addr = addr.as_u64();
        Self {
            pml4: ((addr >> 39) & 0x1ff) as usize,
            pdpt: ((addr >> 30) & 0x1ff) as usize,
            pd: ((addr >> 21) & 0x1ff) as usize,
            pt: ((addr >> 12) & 0x1ff) as usize,
            offset: (addr & 0xfff) as usize,
        }
    }
}
impl PageWalker {
    pub fn walk(root: PagingRoot, virt: VirtAddr) -> Result<WalkResult, MemoryError> {
        walk_internal(root, virt, WalkMode::Lookup)
    }

    pub fn ensure_tables(root: PagingRoot, virt: VirtAddr) -> Result<WalkResult, MemoryError> {
        walk_internal(root, virt, WalkMode::Allocate)
    }
}

fn walk_internal(
    root: PagingRoot,
    virt: VirtAddr,
    mode: WalkMode,
) -> Result<WalkResult, MemoryError> {
    let parts = VirtualAddressParts::from(virt);
    let pml4_table = unsafe { table_from_frame(PhysicalFrame::containing(root.phys_addr())) };
    let pml4_entry = pml4_table.entry_mut(parts.pml4);
    if !pml4_entry.present() && mode == WalkMode::Allocate {
        let new_pdpt_phys = crate::memory::pmm::alloc_frame()?;
        unsafe {
            core::ptr::write_bytes(table_from_frame(new_pdpt_phys), 0, 1);
        }
        pml4_entry.set_frame(new_pdpt_phys.start_address());
        pml4_entry.set_present(true);
        pml4_entry.set_writable(true);
    }
    if !pml4_entry.present() {
        return Err(MemoryError::PageNotPresent);
    }
    let pdpt_phys = pml4_entry.frame();
    let pdpt_table = unsafe { table_from_frame(PhysicalFrame::containing(pdpt_phys)) };
    let pdpt_entry = pdpt_table.entry_mut(parts.pdpt);
    if !pdpt_entry.present() && mode == WalkMode::Allocate {
        let new_pd_phys = crate::memory::pmm::alloc_frame()?;
        unsafe {
            core::ptr::write_bytes(table_from_frame(new_pd_phys), 0, 1);
        }
        pdpt_entry.set_frame(new_pd_phys.start_address());
        pdpt_entry.set_present(true);
        pdpt_entry.set_writable(true);
    }
    if !pdpt_entry.present() {
        return Err(MemoryError::PageNotPresent);
    }
    let pd_phys = pdpt_entry.frame();
    let pd_table = unsafe { table_from_frame(PhysicalFrame::containing(pd_phys)) };
    let pd_entry = pd_table.entry_mut(parts.pd);
    if !pd_entry.present() && mode == WalkMode::Allocate {
        let new_pt_phys = crate::memory::pmm::alloc_frame()?;
        unsafe {
            core::ptr::write_bytes(table_from_frame(new_pt_phys), 0, 1);
        }
        pd_entry.set_frame(new_pt_phys.start_address());
        pd_entry.set_present(true);
        pd_entry.set_writable(true);
    }
    if !pd_entry.present() {
        return Err(MemoryError::PageNotPresent);
    }
    let pt_phys = pd_entry.frame();
    let pt_table = unsafe { table_from_frame(PhysicalFrame::containing(pt_phys)) };
    let pt_entry = pt_table.entry_mut(parts.pt);
    if !pt_entry.present() && mode == WalkMode::Allocate {
        let new_frame_phys = crate::memory::pmm::alloc_frame()?;
        unsafe {
            core::ptr::write_bytes(table_from_frame(new_frame_phys), 0, 1);
        }
        pt_entry.set_frame(new_frame_phys.start_address());
        pt_entry.set_present(true);
        pt_entry.set_writable(true);
    }
    if !pt_entry.present() {
        return Err(MemoryError::PageNotPresent);
    }
    let leaf = pt_table.entry_mut(parts.pt) as *mut _;
    Ok(WalkResult {
        pml4: pml4_table,
        pdpt: pdpt_table,
        pd: pd_table,
        pt: pt_table,
        pml4_index: parts.pml4,
        pdpt_index: parts.pdpt,
        pd_index: parts.pd,
        pt_index: parts.pt,
        leaf,
    })
}
unsafe fn table_from_frame(frame: PhysicalFrame) -> &'static mut PageTable {
    let phys_addr = frame.start_address();
    let virt_addr = phys_to_virt(phys_addr);
    let table = virt_addr.cast::<PageTable>();

    unsafe { &mut *table }
}
