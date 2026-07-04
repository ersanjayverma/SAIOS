//! Virtual memory manager (VMM).
//!
//! Manages the kernel page tables, tracking virtual-to-physical mappings and
//! allocating kernel virtual address space. All operations are serialized with
//! a simple spinlock.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::arch::asm;
use core::sync::atomic::{fence, AtomicBool, Ordering};

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
/// Dedicated higher-half mirror window used for early kernel image mappings.
pub const KERNEL_IMAGE_MIRROR_BASE: VirtAddr = 0xFFFF_9000_0000_0000;
/// Page-table slot used for recursive mapping.
const RECURSIVE_SLOT: u64 = 511;
const EARLY_TABLE_MIN_PHYS: u64 = 0x0010_0000; // 1 MiB
const EARLY_TABLE_MAX_PHYS: u64 = 0x4000_0000; // 1 GiB
const EARLY_TABLE_FALLBACK_MIN_PHYS: u64 = 0x0000_1000; // avoid page 0

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

/// High-level memory types that the VMM can apply to a mapping.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryType {
    /// Default write-back caching.
    WriteBack,
    /// Write-combining cacheability; enabled conservatively until PAT is wired up.
    WriteCombining,
    /// Device/un-cacheable mapping for MMIO.
    Device,
}

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
static NX_PAGE_PROTECTION_ENABLED: AtomicBool = AtomicBool::new(true);

pub fn set_nx_page_protection_enabled(enabled: bool) {
    NX_PAGE_PROTECTION_ENABLED.store(enabled, Ordering::Release);
}

pub fn nx_page_protection_enabled() -> bool {
    NX_PAGE_PROTECTION_ENABLED.load(Ordering::Acquire)
}

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

fn align_down(value: u64, align: u64) -> u64 {
    value & !(align - 1)
}

fn align_up(value: u64, align: u64) -> u64 {
    (value + align - 1) & !(align - 1)
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

fn memory_type_from_flags(vmm_flags: u64) -> MemoryType {
    if (vmm_flags & FLAG_DEVICE) != 0 {
        MemoryType::Device
    } else if (vmm_flags & FLAG_WRITE_COMBINE) != 0 {
        MemoryType::WriteCombining
    } else {
        MemoryType::WriteBack
    }
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
    match memory_type_from_flags(vmm_flags) {
        MemoryType::WriteBack => {}
        MemoryType::WriteCombining => {
            // Keep the initial mapping conservative until PAT is enabled.
            f |= paging::FLAG_PWT;
        }
        MemoryType::Device => {
            f |= paging::FLAG_PCD | paging::FLAG_PWT;
        }
    }
    if nx_page_protection_enabled() && (vmm_flags & FLAG_EXEC) == 0 {
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

fn alloc_zeroed_table() -> Result<PhysAddr, &'static str> {
    // Before we switch CR3, only low physical memory is guaranteed to be
    // identity-mapped on all firmware implementations.
    let mut held = [0u64; 4096];
    let mut held_count = 0usize;

    let mut preferred: Option<u64> = None;
    while held_count < held.len() {
        let candidate = pmm::alloc_page().ok_or("vmm: out of memory allocating page table")?;
        held[held_count] = candidate;
        held_count += 1;

        if (EARLY_TABLE_MIN_PHYS..EARLY_TABLE_MAX_PHYS).contains(&candidate) {
            preferred = Some(candidate);
            break;
        }
    }

    let chosen = if let Some(p) = preferred {
        p
    } else {
        let mut fallback = None;
        for &candidate in held.iter().take(held_count) {
            if (EARLY_TABLE_FALLBACK_MIN_PHYS..EARLY_TABLE_MAX_PHYS).contains(&candidate) {
                fallback = Some(candidate);
                break;
            }
        }
        fallback.ok_or("vmm: no suitable low-memory page available for page table")?
    };

    for &candidate in held.iter().take(held_count) {
        if candidate != chosen {
            pmm::free_page(candidate);
        }
    }

    let table = unsafe { &mut *(chosen as *mut paging::Table) };
    table.clear();
    Ok(chosen)
}

fn map_kernel_page(
    root: &mut paging::Table,
    virt: u64,
    phys: u64,
    flags: u64,
) -> Result<bool, &'static str> {
    let (l4, l3, l2, l1, _) = level_indices(virt);

    if !root.entries[l4].is_present() {
        let pdpt_phys = alloc_zeroed_table()?;
        root.entries[l4].set_page(pdpt_phys, paging::FLAG_WRITABLE);
    }

    let pdpt = unsafe { &mut *(root.entries[l4].address() as *mut paging::Table) };
    if !pdpt.entries[l3].is_present() {
        let pd_phys = alloc_zeroed_table()?;
        pdpt.entries[l3].set_page(pd_phys, paging::FLAG_WRITABLE);
    }

    let pd = unsafe { &mut *(pdpt.entries[l3].address() as *mut paging::Table) };
    if !pd.entries[l2].is_present() {
        let pt_phys = alloc_zeroed_table()?;
        pd.entries[l2].set_page(pt_phys, paging::FLAG_WRITABLE);
    }

    if (pd.entries[l2].0 & paging::FLAG_HUGE) != 0 {
        let huge_phys = pd.entries[l2].address().saturating_add(virt & 0x1f_ffff);
        if huge_phys == phys {
            return Ok(false);
        }
        return Err("vmm: page already mapped");
    }

    let pt = unsafe { &mut *(pd.entries[l2].address() as *mut paging::Table) };
    if pt.entries[l1].is_present() {
        if pt.entries[l1].address() == phys {
            return Ok(false);
        }
        return Err("vmm: page already mapped");
    }

    pt.entries[l1].set_page(phys, leaf_flags(flags) | paging::FLAG_PRESENT);
    Ok(true)
}

fn map_identity_low_mem(root: &mut paging::Table) -> Result<(), &'static str> {
    let pml4_entry = &mut root.entries[0];
    let pdpt_phys = alloc_zeroed_table()?;
    pml4_entry.set_page(pdpt_phys, paging::FLAG_WRITABLE);

    let pdpt = unsafe { &mut *(pdpt_phys as *mut paging::Table) };
    let pd_phys = alloc_zeroed_table()?;
    pdpt.entries[0].set_page(pd_phys, paging::FLAG_WRITABLE);

    let pd = unsafe { &mut *(pd_phys as *mut paging::Table) };
    for index in 0..512usize {
        let phys = (index as u64) * 0x200000;
        pd.entries[index].set_page(
            phys,
            paging::FLAG_WRITABLE | paging::FLAG_PRESENT | paging::FLAG_GLOBAL | paging::FLAG_HUGE,
        );
    }

    Ok(())
}

fn map_boot_stack(root: &mut paging::Table) -> Result<(), &'static str> {
    let rsp: u64;
    unsafe {
        asm!("mov {}, rsp", out(reg) rsp, options(nomem, nostack, preserves_flags));
    }
    // The stack grows downward on x86_64; ensure pages below current RSP are mapped.
    let below_pages = PAGE_SIZE.saturating_mul(16);
    let start = align_down(rsp.saturating_sub(below_pages), PAGE_SIZE as u64);
    let end = align_up(rsp.saturating_add(PAGE_SIZE), PAGE_SIZE as u64);
    map_range_identity(root, start, end, FLAG_WRITE | FLAG_GLOBAL)
}

fn map_range_identity(
    root: &mut paging::Table,
    start: u64,
    end: u64,
    flags: u64,
) -> Result<(), &'static str> {
    let mut current = align_down(start, PAGE_SIZE as u64);
    let limit = align_up(end, PAGE_SIZE as u64);
    while current < limit {
        match map_kernel_page(root, current, current, flags) {
            Ok(_) => {}
            Err("vmm: page already mapped") => {
                // Reuse existing mapping; early firmware mappings can already
                // cover parts of these ranges and should not be fatal.
            }
            Err(e) => return Err(e),
        }
        current = current.saturating_add(PAGE_SIZE);
    }
    Ok(())
}

fn map_range_higher_half_mirror(
    root: &mut paging::Table,
    start: u64,
    end: u64,
    flags: u64,
) -> Result<(), &'static str> {
    let mut current = align_down(start, PAGE_SIZE as u64);
    let limit = align_up(end, PAGE_SIZE as u64);
    while current < limit {
        let virt = KERNEL_IMAGE_MIRROR_BASE
            .checked_add(current)
            .ok_or("vmm: higher-half mirror overflow")?;
        match map_kernel_page(root, virt, current, flags) {
            Ok(_) => {}
            Err("vmm: page already mapped") => {
                // Reuse existing mapping; this range can be re-entered during retries.
            }
            Err(e) => return Err(e),
        }
        current = current.saturating_add(PAGE_SIZE);
    }
    Ok(())
}

/// Performs structural validation of a prepared kernel PML4 root.
pub fn validate_prepared_kernel_pml4(root_phys: PhysAddr) -> Result<(), &'static str> {
    if !is_page_aligned(root_phys) {
        return Err("vmm: pml4 root must be page aligned");
    }

    let pml4 = unsafe { &*(root_phys as *const paging::Table) };
    let recursive = pml4.entries[RECURSIVE_SLOT as usize];
    if !recursive.is_present() {
        return Err("vmm: recursive pml4 entry missing");
    }
    if recursive.address() != root_phys {
        return Err("vmm: recursive pml4 entry address mismatch");
    }

    Ok(())
}

/// Returns true when `virt` resolves to a present mapping in `root_phys` page tables.
pub fn is_mapped_in_page_tables(root_phys: PhysAddr, virt: VirtAddr) -> Result<bool, &'static str> {
    if !is_page_aligned(root_phys) {
        return Err("vmm: root page table must be page aligned");
    }

    let (l4, l3, l2, l1, _) = level_indices(virt);
    let pml4 = unsafe { &*(root_phys as *const paging::Table) };
    let pml4e = pml4.entries[l4];
    if !pml4e.is_present() {
        return Ok(false);
    }

    let pdpt = unsafe { &*(pml4e.address() as *const paging::Table) };
    let pdpte = pdpt.entries[l3];
    if !pdpte.is_present() {
        return Ok(false);
    }
    if (pdpte.0 & paging::FLAG_HUGE) != 0 {
        return Ok(true);
    }

    let pd = unsafe { &*(pdpte.address() as *const paging::Table) };
    let pde = pd.entries[l2];
    if !pde.is_present() {
        return Ok(false);
    }
    if (pde.0 & paging::FLAG_HUGE) != 0 {
        return Ok(true);
    }

    let pt = unsafe { &*(pde.address() as *const paging::Table) };
    Ok(pt.entries[l1].is_present())
}

pub fn bootstrap_kernel_page_tables(
    framebuffer_base: u64,
    framebuffer_size: usize,
    boot_info_ptr: u64,
    boot_info_size: usize,
    kernel_start: u64,
    kernel_end: u64,
) -> Result<PhysAddr, &'static str> {
    let root_phys = alloc_zeroed_table()?;
    let root = unsafe { &mut *(root_phys as *mut paging::Table) };

    map_identity_low_mem(root)?;

    let ram_flags = FLAG_WRITE | FLAG_GLOBAL;
    let framebuffer_flags = FLAG_WRITE | FLAG_GLOBAL | FLAG_WRITE_COMBINE;
    let boot_info_flags = FLAG_WRITE | FLAG_GLOBAL;

    if kernel_end > kernel_start {
        map_range_identity(root, kernel_start, kernel_end, ram_flags)?;
        map_range_higher_half_mirror(root, kernel_start, kernel_end, ram_flags)?;
    }
    if boot_info_size > 0 {
        let start = align_down(boot_info_ptr, PAGE_SIZE as u64);
        let end = align_up(
            boot_info_ptr.saturating_add(boot_info_size as u64),
            PAGE_SIZE as u64,
        );
        map_range_identity(root, start, end, boot_info_flags)?;
    }
    if framebuffer_size > 0 {
        let start = align_down(framebuffer_base, PAGE_SIZE as u64);
        let end = align_up(
            framebuffer_base.saturating_add(framebuffer_size as u64),
            PAGE_SIZE as u64,
        );
        map_range_identity(root, start, end, framebuffer_flags)?;
    }
    map_boot_stack(root)?;

    root.entries[511].set_page(root_phys, paging::FLAG_WRITABLE);

    Ok(root_phys)
}

/// Activates a prepared PML4 by loading CR3.
pub fn activate_kernel_page_tables(kernel_pml4_phys: PhysAddr) -> Result<(), &'static str> {
    let target = kernel_pml4_phys & 0x000F_FFFF_FFFF_F000;
    if target == 0 {
        return Err("vmm: cr3 physical address must be non-zero");
    }

    // Ensure page-table writes are globally visible before loading CR3.
    fence(Ordering::SeqCst);

    unsafe {
        paging::write_cr3(target);
    }
    Ok(())
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

/// Updates tracked CR3 after a successful page-table switch.
pub fn set_active_cr3(kernel_pml4_phys: PhysAddr) -> Result<(), &'static str> {
    let target = kernel_pml4_phys & 0x000F_FFFF_FFFF_F000;
    if target == 0 {
        return Err("vmm: cr3 physical address must be non-zero");
    }

    with_state_mut(|state| {
        if !state.initialized {
            return Err("vmm: not initialized");
        }
        state.cr3 = target;
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
