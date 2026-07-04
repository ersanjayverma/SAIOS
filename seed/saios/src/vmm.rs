//! Virtual memory manager (VMM).
//!
//! Manages the kernel page tables, tracking virtual-to-physical mappings and
//! allocating kernel virtual address space. All operations are serialized with
//! a simple spinlock.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::arch::asm;
use core::sync::atomic::{AtomicBool, Ordering};

use hal::arch::paging;
use hal::arch::x86_64::sync::StaticCell;

use crate::kernel::testing::report::{VerifyCheck, VerifyReport};
use crate::pmm;

/// Virtual address type.
pub type VirtAddr = u64;
/// Physical address type.
pub type PhysAddr = u64;

/// Size of a page in bytes.
pub const PAGE_SIZE: u64 = 4096;
/// Base of the kernel's higher-half virtual address space.
pub const KERNEL_VIRT_BASE: VirtAddr = 0xFFFF_8000_0000_0000;
/// Page-table slot used for recursive mapping.
const RECURSIVE_SLOT: u64 = 511;

/// Page-table flag: readable.
pub const FLAG_READ: u64 = 1 << 0;
/// Page-table flag: writable.
pub const FLAG_WRITE: u64 = 1 << 1;
/// Page-table flag: executable.
pub const FLAG_EXEC: u64 = 1 << 2;
/// Page-table flag: user-accessible.
pub const FLAG_USER: u64 = 1 << 3;
/// Page-table flag: global (not flushed on TLB switch).
pub const FLAG_GLOBAL: u64 = 1 << 4;
/// Page-table flag: device memory (uncached).
pub const FLAG_DEVICE: u64 = 1 << 5;
/// Page-table flag: write-combining memory.
pub const FLAG_WRITE_COMBINE: u64 = 1 << 6;

/// A recorded virtual-to-physical mapping.
#[derive(Clone, Debug)]
pub struct Mapping {
    /// Start of the virtual range.
    pub virt_start: VirtAddr,
    /// Start of the physical range.
    pub phys_start: PhysAddr,
    /// Number of pages in the mapping.
    pub pages: usize,
    /// Page-table flags for the mapping.
    pub flags: u64,
    /// Human-readable owner/description.
    pub owner: String,
    /// True if the physical pages were allocated by the VMM and should be
    /// freed on unmap.
    pub owned_physical: bool,
}

/// Snapshot of VMM state.
#[derive(Copy, Clone, Debug, Default)]
pub struct VmmStats {
    /// True if the VMM has been initialized.
    pub initialized: bool,
    /// Physical address of the current PML4.
    pub cr3: u64,
    /// Number of recorded mappings.
    pub mappings: usize,
    /// Total number of mapped pages.
    pub mapped_pages: usize,
    /// Next free kernel virtual address.
    pub next_kernel_virt: VirtAddr,
}

struct VmmState {
    initialized: bool,
    cr3: u64,
    mappings: Vec<Mapping>,
    next_kernel_virt: VirtAddr,
}

impl VmmState {
    fn new() -> Self {
        Self {
            initialized: false,
            cr3: 0,
            mappings: Vec::new(),
            next_kernel_virt: KERNEL_VIRT_BASE,
        }
    }
}

static STATE: StaticCell<Option<VmmState>> = StaticCell::new(None);
static LOCK: AtomicBool = AtomicBool::new(false);

fn lock() {
    while LOCK
        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

fn unlock() {
    LOCK.store(false, Ordering::Release);
}

fn with_state_mut<R>(f: impl FnOnce(&mut VmmState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(VmmState::new());
            }
            slot.as_mut().expect("vmm: state unavailable")
        };
        f(state)
    };
    unlock();
    out
}

fn with_state<R>(f: impl FnOnce(&VmmState) -> R) -> R {
    lock();
    let out = {
        let state = unsafe {
            let slot = &mut *STATE.get();
            if slot.is_none() {
                *slot = Some(VmmState::new());
            }
            slot.as_ref().expect("vmm: state unavailable")
        };
        f(state)
    };
    unlock();
    out
}

fn is_page_aligned(value: u64) -> bool {
    (value & (PAGE_SIZE - 1)) == 0
}

fn checked_range_end(start: u64, pages: usize) -> Option<u64> {
    let bytes = (pages as u64).checked_mul(PAGE_SIZE)?;
    start.checked_add(bytes)
}

fn ranges_overlap(a_start: u64, a_end: u64, b_start: u64, b_end: u64) -> bool {
    a_start < b_end && b_start < a_end
}

fn mapping_range(mapping: &Mapping) -> (u64, u64) {
    let start = mapping.virt_start;
    let end = checked_range_end(start, mapping.pages).unwrap_or(start);
    (start, end)
}

fn canonicalize_48(addr: u64) -> u64 {
    if (addr & (1u64 << 47)) != 0 {
        addr | 0xFFFF_0000_0000_0000
    } else {
        addr & 0x0000_FFFF_FFFF_FFFF
    }
}

fn level_indices(virt: VirtAddr) -> (usize, usize, usize, usize, u64) {
    let l4 = ((virt >> 39) & 0x1ff) as usize;
    let l3 = ((virt >> 30) & 0x1ff) as usize;
    let l2 = ((virt >> 21) & 0x1ff) as usize;
    let l1 = ((virt >> 12) & 0x1ff) as usize;
    let off = virt & 0xfff;
    (l4, l3, l2, l1, off)
}

fn pml4_table_ptr() -> *mut paging::Table {
    let va = canonicalize_48(
        (RECURSIVE_SLOT << 39)
            | (RECURSIVE_SLOT << 30)
            | (RECURSIVE_SLOT << 21)
            | (RECURSIVE_SLOT << 12),
    );
    va as *mut paging::Table
}

fn pdpt_table_ptr(l4: usize) -> *mut paging::Table {
    let va = canonicalize_48(
        (RECURSIVE_SLOT << 39)
            | (RECURSIVE_SLOT << 30)
            | (RECURSIVE_SLOT << 21)
            | ((l4 as u64) << 12),
    );
    va as *mut paging::Table
}

fn pd_table_ptr(l4: usize, l3: usize) -> *mut paging::Table {
    let va = canonicalize_48(
        (RECURSIVE_SLOT << 39) | (RECURSIVE_SLOT << 30) | ((l4 as u64) << 21) | ((l3 as u64) << 12),
    );
    va as *mut paging::Table
}

fn pt_table_ptr(l4: usize, l3: usize, l2: usize) -> *mut paging::Table {
    let va = canonicalize_48(
        (RECURSIVE_SLOT << 39) | ((l4 as u64) << 30) | ((l3 as u64) << 21) | ((l2 as u64) << 12),
    );
    va as *mut paging::Table
}

fn has_any_present_entries(table: &paging::Table) -> bool {
    table.entries.iter().any(|e| e.is_present())
}

fn nonleaf_flags(vmm_flags: u64) -> u64 {
    let mut f = paging::FLAG_WRITABLE;
    if (vmm_flags & FLAG_USER) != 0 {
        f |= paging::FLAG_USER;
    }
    f
}

fn leaf_flags(vmm_flags: u64) -> u64 {
    // Translate high-level VMM memory flags into leaf page-table bits.
    let mut f = 0u64;
    if (vmm_flags & FLAG_WRITE) != 0 {
        f |= paging::FLAG_WRITABLE;
    }
    if (vmm_flags & FLAG_USER) != 0 {
        f |= paging::FLAG_USER;
    }
    if (vmm_flags & FLAG_GLOBAL) != 0 {
        f |= paging::FLAG_GLOBAL;
    }
    if (vmm_flags & FLAG_WRITE_COMBINE) != 0 {
        f |= paging::FLAG_PWT;
    }
    if (vmm_flags & FLAG_DEVICE) != 0 {
        f |= paging::FLAG_PCD | paging::FLAG_PWT;
    }
    if (vmm_flags & FLAG_EXEC) == 0 {
        f |= paging::FLAG_NX;
    }
    f
}

fn invlpg(virt: VirtAddr) {
    unsafe {
        asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
    }
}

fn map_page_hw(virt: VirtAddr, phys: PhysAddr, flags: u64) -> Result<(), &'static str> {
    let (l4, l3, l2, l1, _) = level_indices(virt);

    let pml4 = unsafe { &mut *pml4_table_ptr() };
    if !pml4.entries[l4].is_present() {
        let new_page = pmm::alloc_page().ok_or("vmm: out of memory for pdpt")?;
        pml4.entries[l4].set_page(new_page, nonleaf_flags(flags));
        unsafe { (&mut *pdpt_table_ptr(l4)).clear() };
    }

    let pdpt = unsafe { &mut *pdpt_table_ptr(l4) };
    if !pdpt.entries[l3].is_present() {
        let new_page = pmm::alloc_page().ok_or("vmm: out of memory for pd")?;
        pdpt.entries[l3].set_page(new_page, nonleaf_flags(flags));
        unsafe { (&mut *pd_table_ptr(l4, l3)).clear() };
    }

    let pd = unsafe { &mut *pd_table_ptr(l4, l3) };
    if !pd.entries[l2].is_present() {
        let new_page = pmm::alloc_page().ok_or("vmm: out of memory for pt")?;
        pd.entries[l2].set_page(new_page, nonleaf_flags(flags));
        unsafe { (&mut *pt_table_ptr(l4, l3, l2)).clear() };
    }

    let pt = unsafe { &mut *pt_table_ptr(l4, l3, l2) };
    if pt.entries[l1].is_present() {
        return Err("vmm: page already mapped");
    }

    pt.entries[l1].set_page(phys, leaf_flags(flags));
    invlpg(virt);
    Ok(())
}

fn unmap_page_hw(virt: VirtAddr) -> Result<(), &'static str> {
    let (l4, l3, l2, l1, _) = level_indices(virt);

    let pml4 = unsafe { &mut *pml4_table_ptr() };
    if !pml4.entries[l4].is_present() {
        return Err("vmm: page not mapped");
    }

    let pdpt = unsafe { &mut *pdpt_table_ptr(l4) };
    if !pdpt.entries[l3].is_present() {
        return Err("vmm: page not mapped");
    }

    let pd = unsafe { &mut *pd_table_ptr(l4, l3) };
    if !pd.entries[l2].is_present() {
        return Err("vmm: page not mapped");
    }

    let pt = unsafe { &mut *pt_table_ptr(l4, l3, l2) };
    if !pt.entries[l1].is_present() {
        return Err("vmm: page not mapped");
    }

    pt.entries[l1] = paging::Entry::new();
    invlpg(virt);

    // Trim now-empty intermediate tables to prevent unbounded growth.
    if !has_any_present_entries(pt) {
        pd.entries[l2] = paging::Entry::new();
        if !has_any_present_entries(pd) {
            pdpt.entries[l3] = paging::Entry::new();
            if !has_any_present_entries(pdpt) {
                pml4.entries[l4] = paging::Entry::new();
            }
        }
    }

    Ok(())
}

fn translate_hw(virt: VirtAddr) -> Option<PhysAddr> {
    let (l4, l3, l2, l1, off) = level_indices(virt);
    let pml4 = unsafe { &*pml4_table_ptr() };
    if !pml4.entries[l4].is_present() {
        return None;
    }

    let pdpt = unsafe { &*pdpt_table_ptr(l4) };
    if !pdpt.entries[l3].is_present() {
        return None;
    }

    let pd = unsafe { &*pd_table_ptr(l4, l3) };
    if !pd.entries[l2].is_present() {
        return None;
    }

    let pt = unsafe { &*pt_table_ptr(l4, l3, l2) };
    if !pt.entries[l1].is_present() {
        return None;
    }

    pt.entries[l1].address().checked_add(off)
}

/// Initializes the VMM with the given physical PML4 address.
pub fn init(kernel_pml4_phys: PhysAddr) -> Result<(), &'static str> {
    if !is_page_aligned(kernel_pml4_phys) {
        return Err("vmm: cr3 physical address must be page aligned");
    }

    with_state_mut(|state| {
        if state.initialized {
            return Ok(());
        }

        state.cr3 = kernel_pml4_phys;
        state.initialized = true;
        state.next_kernel_virt = KERNEL_VIRT_BASE;
        Ok(())
    })
}

/// Maps `pages` pages from `phys_start` to `virt_start` with the given flags.
pub fn map(
    virt_start: VirtAddr,
    phys_start: PhysAddr,
    pages: usize,
    flags: u64,
    owner: &str,
) -> Result<(), &'static str> {
    if pages == 0 {
        return Err("vmm: pages must be > 0");
    }
    if owner.is_empty() {
        return Err("vmm: owner must be non-empty");
    }
    if !is_page_aligned(virt_start) || !is_page_aligned(phys_start) {
        return Err("vmm: addresses must be page aligned");
    }

    let new_end = checked_range_end(virt_start, pages).ok_or("vmm: mapping range overflow")?;

    with_state_mut(|state| {
        if !state.initialized {
            return Err("vmm: not initialized");
        }

        if (flags & FLAG_USER) == 0 && virt_start < KERNEL_VIRT_BASE {
            return Err("vmm: kernel mappings must be in higher-half range");
        }

        for existing in &state.mappings {
            let (start, end) = mapping_range(existing);
            if ranges_overlap(virt_start, new_end, start, end) {
                return Err("vmm: overlapping virtual mapping");
            }
        }

        let mut mapped = 0usize;
        while mapped < pages {
            let v = virt_start.saturating_add((mapped as u64).saturating_mul(PAGE_SIZE));
            let p = phys_start.saturating_add((mapped as u64).saturating_mul(PAGE_SIZE));
            if let Err(e) = map_page_hw(v, p, flags) {
                for rollback in 0..mapped {
                    let rv = virt_start.saturating_add((rollback as u64).saturating_mul(PAGE_SIZE));
                    let _ = unmap_page_hw(rv);
                }
                return Err(e);
            }
            mapped = mapped.saturating_add(1);
        }

        state.mappings.push(Mapping {
            virt_start,
            phys_start,
            pages,
            flags,
            owner: owner.to_string(),
            owned_physical: false,
        });

        Ok(())
    })
}

/// Maps `pages` pages from `phys_start` to `virt_start` and records the VMM
/// as the owner of the physical pages.
pub fn map_owned(
    virt_start: VirtAddr,
    phys_start: PhysAddr,
    pages: usize,
    flags: u64,
    owner: &str,
) -> Result<(), &'static str> {
    map(virt_start, phys_start, pages, flags, owner)?;
    with_state_mut(|state| {
        if let Some(last) = state.mappings.last_mut()
            && last.virt_start == virt_start
            && last.phys_start == phys_start
            && last.pages == pages
        {
            last.owned_physical = true;
        }
    });
    Ok(())
}

/// Allocates physical pages and maps them into kernel virtual address space.
pub fn alloc_and_map(pages: usize, flags: u64, owner: &str) -> Result<VirtAddr, &'static str> {
    if pages == 0 {
        return Err("vmm: pages must be > 0");
    }

    let phys = pmm::alloc_pages(pages).ok_or("vmm: physical allocation failed")?;

    let virt = with_state_mut(|state| {
        if !state.initialized {
            return Err("vmm: not initialized");
        }
        let start = state.next_kernel_virt;
        let end = checked_range_end(start, pages).ok_or("vmm: virtual range overflow")?;
        state.next_kernel_virt = end;
        Ok(start)
    })?;

    if let Err(e) = map_owned(virt, phys, pages, flags, owner) {
        let _ = pmm::free_pages_range(phys, pages);
        return Err(e);
    }

    Ok(virt)
}

/// Maps physical pages at the next available kernel virtual address.
pub fn map_physical_anywhere(
    phys_start: PhysAddr,
    pages: usize,
    flags: u64,
    owner: &str,
) -> Result<VirtAddr, &'static str> {
    if pages == 0 {
        return Err("vmm: pages must be > 0");
    }
    if !is_page_aligned(phys_start) {
        return Err("vmm: physical address must be page aligned");
    }

    let virt = with_state_mut(|state| {
        if !state.initialized {
            return Err("vmm: not initialized");
        }

        let start = state.next_kernel_virt;
        let end = checked_range_end(start, pages).ok_or("vmm: virtual range overflow")?;
        state.next_kernel_virt = end;
        Ok(start)
    })?;

    if let Err(e) = map(virt, phys_start, pages, flags, owner) {
        with_state_mut(|state| {
            if state.next_kernel_virt
                >= virt.saturating_add((pages as u64).saturating_mul(PAGE_SIZE))
            {
                state.next_kernel_virt = virt;
            }
        });
        return Err(e);
    }

    Ok(virt)
}

/// Removes the mapping starting at `virt_start` and frees owned physical pages.
pub fn unmap(virt_start: VirtAddr) -> Result<(), &'static str> {
    if !is_page_aligned(virt_start) {
        return Err("vmm: virtual address must be page aligned");
    }

    with_state_mut(|state| {
        if !state.initialized {
            return Err("vmm: not initialized");
        }

        let idx = state
            .mappings
            .iter()
            .position(|m| m.virt_start == virt_start)
            .ok_or("vmm: mapping not found")?;

        let mapping = state.mappings.remove(idx);

        for page in 0..mapping.pages {
            let v = mapping
                .virt_start
                .saturating_add((page as u64).saturating_mul(PAGE_SIZE));
            let _ = unmap_page_hw(v);
        }

        if mapping.owned_physical {
            let _ = pmm::free_pages_range(mapping.phys_start, mapping.pages);
        }

        Ok(())
    })
}

/// Returns the physical address mapped at `virt`, if any.
pub fn translate(virt: VirtAddr) -> Option<PhysAddr> {
    with_state(|state| {
        if state.initialized {
            translate_hw(virt)
        } else {
            None
        }
    })
}

/// Returns a snapshot of all recorded mappings.
pub fn mappings() -> Vec<Mapping> {
    with_state(|state| state.mappings.clone())
}

/// Returns a snapshot of VMM statistics.
pub fn stats() -> VmmStats {
    with_state(|state| {
        let mapped_pages = state
            .mappings
            .iter()
            .fold(0usize, |acc, m| acc.saturating_add(m.pages));

        VmmStats {
            initialized: state.initialized,
            cr3: state.cr3,
            mappings: state.mappings.len(),
            mapped_pages,
            next_kernel_virt: state.next_kernel_virt,
        }
    })
}

pub fn verify() -> VerifyReport {
    let stats = stats();
    let mut checks = Vec::new();

    checks.push(if stats.initialized {
        VerifyCheck::pass("VMM initialized", "vmm state initialized")
    } else {
        VerifyCheck::fail("VMM initialized", "vmm not initialized")
    });

    checks.push(if stats.cr3 != 0 {
        VerifyCheck::pass("Kernel CR3", "kernel page table root recorded")
    } else {
        VerifyCheck::fail("Kernel CR3", "kernel page table root missing")
    });

    checks.push(if stats.next_kernel_virt >= KERNEL_VIRT_BASE {
        VerifyCheck::pass("Kernel VA cursor", "next kernel VA in high-half range")
    } else {
        VerifyCheck::fail("Kernel VA cursor", "next kernel VA below high-half base")
    });

    checks.push(if stats.mapped_pages >= stats.mappings {
        VerifyCheck::pass("Mapping accounting", "mapped pages >= mapping count")
    } else {
        VerifyCheck::fail("Mapping accounting", "mapped pages < mapping count")
    });

    VerifyReport {
        target: "vmm",
        checks,
    }
}
