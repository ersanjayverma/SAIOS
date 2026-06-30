//! x86_64 4-level paging (IA-32e / long mode).
//!
//! Full implementation supporting 4 KiB, 2 MiB, and 1 GiB pages,
//! recursive page-table mapping, NX/WP control, and TLB shootdown.
// NEED TO REVIEW NAMUALY ONCE MORE
use core::arch::asm;
use core::ops::{Index, IndexMut};

use crate::println;

// ── Constants ─────────────────────────────────────────────────────

pub const ENTRY_COUNT: usize = 512;
pub const PAGE_SIZE: u64 = 4096;
pub const HUGE_2MIB: u64 = 2 * 1024 * 1024;
pub const HUGE_1GIB: u64 = 1024 * 1024 * 1024;

/// Recursive page-table index — PML4[511] points to the PML4 itself.
pub const RECURSIVE_INDEX: usize = 511;
// ── Page-table entry flags ────────────────────────────────────────

pub const FLAG_PRESENT: u64 = 1 << 0;
pub const FLAG_WRITABLE: u64 = 1 << 1;
pub const FLAG_USER: u64 = 1 << 2;
pub const FLAG_PWT: u64 = 1 << 3;
pub const FLAG_PCD: u64 = 1 << 4;
pub const FLAG_ACCESSED: u64 = 1 << 5;
pub const FLAG_DIRTY: u64 = 1 << 6;
pub const FLAG_HUGE: u64 = 1 << 7;
pub const FLAG_GLOBAL: u64 = 1 << 8;
pub const FLAG_NX: u64 = 1 << 63;
const ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

// ── Page-table entry ──────────────────────────────────────────────


#[repr(transparent)]
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct Entry(u64);


impl Entry {
    pub const fn new() -> Self {
        Self(0)
    }

    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    pub const fn raw(&self) -> u64 {
        self.0
    }

    pub const fn is_present(&self) -> bool {
        (self.0 & FLAG_PRESENT) != 0
    }

    pub const fn is_writable(&self) -> bool {
        (self.0 & FLAG_WRITABLE) != 0
    }

    pub const fn is_user(&self) -> bool {
        (self.0 & FLAG_USER) != 0
    }

    pub const fn is_huge(&self) -> bool {
        (self.0 & FLAG_HUGE) != 0
    }

    pub const fn address(&self) -> u64 {
        self.0 & ADDR_MASK
    }

    // ── Builder-style setters ──────────────────────────────────

    pub fn set(&mut self, flags: u64) {
        self.0 |= flags;
    }

    pub fn clear(&mut self, flags: u64) {
        self.0 &= !flags;
    }

    pub fn set_present(&mut self, v: bool) {
        if v { self.0 |= FLAG_PRESENT; } else { self.0 &= !FLAG_PRESENT; }
    }

    pub fn set_writable(&mut self, v: bool) {
        if v { self.0 |= FLAG_WRITABLE; } else { self.0 &= !FLAG_WRITABLE; }
    }

    pub fn set_user(&mut self, v: bool) {
        if v { self.0 |= FLAG_USER; } else { self.0 &= !FLAG_USER; }
    }

    pub fn set_huge(&mut self, v: bool) {
        if v { self.0 |= FLAG_HUGE; } else { self.0 &= !FLAG_HUGE; }
    }

    pub fn set_global(&mut self, v: bool) {
        if v { self.0 |= FLAG_GLOBAL; } else { self.0 &= !FLAG_GLOBAL; }
    }

    pub fn set_nx(&mut self, v: bool) {
        if v { self.0 |= FLAG_NX; } else { self.0 &= !FLAG_NX; }
    }

    /// Set the physical address.  Only bits 12..51 are used.
    pub fn set_address(&mut self, addr: u64) {
        debug_assert!(addr & 0xFFF == 0, "address must be page-aligned");
        self.0 = (self.0 & !ADDR_MASK) | (addr & ADDR_MASK);
    }

    /// Convenience: configure as a 4 KiB page pointer.
    pub fn set_page(&mut self, addr: u64, writable: bool, user: bool, nx: bool) {
        self.set_address(addr);
        self.set_present(true);
        self.set_writable(writable);
        self.set_user(user);
        self.set_nx(nx);
    }

    /// Convenience: configure as a 2 MiB huge page.
    pub fn set_huge_2mib(&mut self, addr: u64, writable: bool, user: bool, nx: bool) {
        self.set_address(addr);
        self.set_present(true);
        self.set_writable(writable);
        self.set_user(user);
        self.set_huge(true);
        self.set_nx(nx);
    }

    /// Convenience: configure as a 1 GiB huge page.
    pub fn set_huge_1gib(&mut self, addr: u64, writable: bool, user: bool, nx: bool) {
        self.set_address(addr);
        self.set_present(true);
        self.set_writable(writable);
        self.set_user(user);
        self.set_huge(true);
        self.set_nx(nx);
    }
    pub fn atomic_write(&mut self, val: u64) {
        unsafe {
            core::ptr::write_volatile(&mut self.0, val);
        }
    }
}

impl core::fmt::Debug for Entry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Entry")
            .field("P", &self.is_present())
            .field("W", &self.is_writable())
            .field("U", &self.is_user())
            .field("H", &self.is_huge())
            .field("NX", &((self.0 & FLAG_NX) != 0))
            .field("addr", &format_args!("0x{:016X}", self.address()))
            .finish()
    }
}

// ── Page-table level ──────────────────────────────────────────────

/// One page-table (PML4 / PDPT / PD / PT).  4 KiB aligned, 512 entries.
#[repr(C, align(4096))]
pub struct Table {
    pub entries: [Entry; ENTRY_COUNT],
}

impl Table {
    
    pub const fn new() -> Self {
        Self { entries: [Entry::new(); ENTRY_COUNT] }
    }

    pub fn clear(&mut self) {
        self.entries.fill(Entry::new());
    }

    /// Returns the physical address of this table.
    ///
    /// During early bootstrap the kernel is identity-mapped
    /// (virtual == physical), so this is just `self as *const Self as u64`.
    /// After we move to a higher-half layout, subtract KERNEL_VIRT_BASE.
    pub fn as_phys_addr(&self) -> u64 {
        self as *const Self as u64
    }
}

impl Index<usize> for Table {
    type Output = Entry;
    fn index(&self, i: usize) -> &Entry { &self.entries[i] }
}

impl IndexMut<usize> for Table {
    fn index_mut(&mut self, i: usize) -> &mut Entry { &mut self.entries[i] }
}

// ── Virtual address decomposition ──────────────────────────────────

/// Decompose a virtual address into the four level indices and the
/// page offset.
#[derive(Debug, Clone, Copy)]
pub struct VirtAddr {
    pub pml4: usize,
    pub pdp: usize,
    pub pd: usize,
    pub pt: usize,
    pub offset: u16,
}

impl VirtAddr {
    pub const fn new(va: u64) -> Self {
        Self {
            pml4: ((va >> 39) & 0x1FF) as usize,
            pdp:  ((va >> 30) & 0x1FF) as usize,
            pd:   ((va >> 21) & 0x1FF) as usize,
            pt:   ((va >> 12) & 0x1FF) as usize,
            offset: (va & 0xFFF) as u16,
        }
    }

    /// Build a virtual address from its components.
    pub const fn to_va(pml4: usize, pdp: usize, pd: usize, pt: usize, offset: u16) -> u64 {
    let mut va = ((pml4 as u64) & 0x1FF) << 39
        | ((pdp as u64) & 0x1FF) << 30
        | ((pd as u64) & 0x1FF) << 21
        | ((pt as u64) & 0x1FF) << 12
        | ((offset as u64) & 0xFFF);
    
    // If bit 47 is set, sign-extend bits 48..63 to 1
    if (va & (1 << 47)) != 0 {
        va |= 0xFFFF_0000_0000_0000;
    }
    va
}

}

// ── Recursive mapping helpers ─────────────────────────────────────

/// Compute the virtual address of the PML4 table itself via the
/// recursive entry.
pub fn pml4_virt_addr() -> u64 {
    VirtAddr::to_va(RECURSIVE_INDEX, RECURSIVE_INDEX, RECURSIVE_INDEX, RECURSIVE_INDEX, 0)
}

/// Compute the virtual address of the PDP table for a given PML4 index.
pub fn pdp_virt_addr(pml4_idx: usize) -> u64 {
    VirtAddr::to_va(RECURSIVE_INDEX, RECURSIVE_INDEX, RECURSIVE_INDEX, pml4_idx, 0)
}

/// Compute the virtual address of the PD table for given PML4/PDP indices.
pub fn pd_virt_addr(pml4_idx: usize, pdp_idx: usize) -> u64 {
    VirtAddr::to_va(RECURSIVE_INDEX, RECURSIVE_INDEX, pml4_idx, pdp_idx, 0)
}

/// Compute the virtual address of the PT table for given PML4/PDP/PD indices.
pub fn pt_virt_addr(pml4_idx: usize, pdp_idx: usize, pd_idx: usize) -> u64 {
    VirtAddr::to_va(RECURSIVE_INDEX, pml4_idx, pdp_idx, pd_idx, 0)
}

// ── Page-table walker ─────────────────────────────────────────────

/// Walk the page tables for a virtual address, returning mutable
/// references to each level's entry.  Creates intermediate tables
/// on demand if `alloc_table` is provided.
///
/// Returns `(pml4e, pdpe, pde, pte)` — the entries at each level
/// that lead to the page.
///
/// # Safety
///
/// The PML4 must be valid and the recursive mapping must be set up.
pub unsafe fn walk(
    va: u64,
    mut alloc_table: impl FnMut() -> *mut Table,
) -> Option<(*mut Entry, *mut Entry, *mut Entry, *mut Entry)> {
    let v = VirtAddr::new(va);

    // 1. PML4 Entry
    let pml4_ptr = pml4_virt_addr() as *mut Table;
    let pml4 = unsafe { &mut *pml4_ptr };
    let pml4e = &mut pml4[v.pml4];

    if !pml4e.is_present() {
        let new_table = alloc_table();
        // Convert the raw pointer check cleanly into an Option bubble
        if new_table.is_null() { return None; }
        unsafe { (*new_table).clear(); }
        pml4e.set_page(new_table as u64, true, false, false);
    }
    // PML4 entries cannot be huge pages (huge page feature starts at PDPT level)

    // 2. PDP Entry (for 1 GiB huge pages)
    let pdp_ptr = pdp_virt_addr(v.pml4) as *mut Table;
    let pdp = unsafe { &mut *pdp_ptr };
    let pdpe = &mut pdp[v.pdp];

    if !pdpe.is_present() {
        let new_table = alloc_table();
        if new_table.is_null() { return None; }
        unsafe { (*new_table).clear(); }
        pdpe.set_page(new_table as u64, true, false, false);
    }
    if pdpe.is_huge() { return Some((pml4e, pdpe, core::ptr::null_mut(), core::ptr::null_mut())); }

    // 3. PD Entry (for 2 MiB huge pages)
    let pd_ptr = pd_virt_addr(v.pml4, v.pdp) as *mut Table;
    let pd = unsafe { &mut *pd_ptr };
    let pde = &mut pd[v.pd];

    if !pde.is_present() {
        let new_table = alloc_table();
        if new_table.is_null() { return None; }
        unsafe { (*new_table).clear(); }
        pde.set_page(new_table as u64, true, false, false);
    }
    if pde.is_huge() { return Some((pml4e, pdpe, pde, core::ptr::null_mut())); }

    // 4. PT Entry
    let pt_ptr = pt_virt_addr(v.pml4, v.pdp, v.pd) as *mut Table;
    let pt = unsafe { &mut *pt_ptr };
    let pte = &mut pt[v.pt];

    Some((pml4e, pdpe, pde, pte))
}

// ── Mapping functions ─────────────────────────────────────────────

/// Map a 4 KiB page at virtual address `va` to physical address `pa`.
///
/// Returns `true` on success, `false` if allocation failed.
pub unsafe fn map_4kib(
    va: u64,
    pa: u64,
    writable: bool,
    user: bool,
    nx: bool,
    mut alloc_table: impl FnMut() -> *mut Table,
) -> bool {
    debug_assert!(pa & 0xFFF == 0, "physical address must be page-aligned");

    let Some((_pml4e, _pdpe, _pde, pte)) = (unsafe { walk(va, &mut alloc_table) }) else {
        return false;
    };

    if pte.is_null() {
        // Hit a huge page — cannot map a 4 KiB sub-page without splitting.
        return false;
    }

    unsafe {
        (*pte).set_page(pa, writable, user, nx);
    }
    true
}

/// Map a 2 MiB huge page at virtual address `va` to physical address `pa`.
pub unsafe fn map_2mib(
    va: u64,
    pa: u64,
    writable: bool,
    user: bool,
    nx: bool,
    mut alloc_table: impl FnMut() -> *mut Table,
) -> bool {
    debug_assert!(pa & (HUGE_2MIB - 1) == 0, "pa must be 2 MiB aligned");
    debug_assert!(va & (HUGE_2MIB - 1) == 0, "va must be 2 MiB aligned");

    let v = VirtAddr::new(va);

    // Walk down to PD level only.
    let pml4_ptr = pml4_virt_addr() as *mut Table;
    let pml4 = unsafe { &mut *pml4_ptr };
    let pml4e = &mut pml4[v.pml4];

    if !pml4e.is_present() {
        let t = alloc_table();
        if t.is_null() { return false; }
        unsafe { (*t).clear(); }
        pml4e.set_page(t as u64, true, false, false);
    }

    let pdp_ptr = pdp_virt_addr(v.pml4) as *mut Table;
    let pdp = unsafe { &mut *pdp_ptr };
    let pdpe = &mut pdp[v.pdp];

    if !pdpe.is_present() {
        let t = alloc_table();
        if t.is_null() { return false; }
        unsafe { (*t).clear(); }
        pdpe.set_page(t as u64, true, false, false);
    }

    let pd_ptr = pd_virt_addr(v.pml4, v.pdp) as *mut Table;
    let pd = unsafe { &mut *pd_ptr };
    let pde = &mut pd[v.pd];

    pde.set_huge_2mib(pa, writable, user, nx);
    true
}

/// Map a 1 GiB huge page at virtual address `va` to physical address `pa`.
pub unsafe fn map_1gib(
    va: u64,
    pa: u64,
    writable: bool,
    user: bool,
    nx: bool,
    mut alloc_table: impl FnMut() -> *mut Table,
) -> bool {
    debug_assert!(pa & (HUGE_1GIB - 1) == 0, "pa must be 1 GiB aligned");
    debug_assert!(va & (HUGE_1GIB - 1) == 0, "va must be 1 GiB aligned");

    let v = VirtAddr::new(va);

    let pml4_ptr = pml4_virt_addr() as *mut Table;
    let pml4 = unsafe { &mut *pml4_ptr };
    let pml4e = &mut pml4[v.pml4];

    if !pml4e.is_present() {
        let t = alloc_table();
        if t.is_null() { return false; }
        unsafe { (*t).clear(); }
        pml4e.set_page(t as u64, true, false, false);
    }

    let pdp_ptr = pdp_virt_addr(v.pml4) as *mut Table;
    let pdp = unsafe { &mut *pdp_ptr };
    let pdpe = &mut pdp[v.pdp];

    pdpe.set_huge_1gib(pa, writable, user, nx);
    true
}

/// Unmap a 4 KiB page at virtual address `va`.
pub unsafe fn unmap_4kib(va: u64) {
    let v = VirtAddr::new(va);

    let pml4_ptr = pml4_virt_addr() as *mut Table;
    let pml4 = unsafe { &mut *pml4_ptr };
    if !pml4[v.pml4].is_present() { return; }

    let pdp_ptr = pdp_virt_addr(v.pml4) as *mut Table;
    let pdp = unsafe { &mut *pdp_ptr };
    if !pdp[v.pdp].is_present() { return; }
    if pdp[v.pdp].is_huge() { return; } // 1 GiB page, can't unmap sub-page

    let pd_ptr = pd_virt_addr(v.pml4, v.pdp) as *mut Table;
    let pd = unsafe { &mut *pd_ptr };
    if !pd[v.pd].is_present() { return; }
    if pd[v.pd].is_huge() { return; } // 2 MiB page, can't unmap sub-page

    let pt_ptr = pt_virt_addr(v.pml4, v.pdp, v.pd) as *mut Table;
    let pt = unsafe { &mut *pt_ptr };
    pt[v.pt] = Entry::new();

    // Invalidate TLB for this address.
    invlpg(va);
}

// ── TLB management ────────────────────────────────────────────────

/// Invalidate a single TLB entry for virtual address `va`.
#[inline(always)]
pub fn invlpg(va: u64) {
    unsafe {
        asm!("invlpg [{}]", in(reg) va, options(nostack, preserves_flags));
    }
}

/// Reload CR3 to flush the entire TLB.
pub fn flush_tlb() {
    let cr3: u64;
    unsafe {
        asm!("mov {}, cr3", out(reg) cr3, options(nostack));
        asm!("mov cr3, {}", in(reg) cr3, options(nostack));
    }
}

// ── CR3 / control register management ─────────────────────────────

/// Load CR3 with the physical address of a PML4 table.
pub unsafe fn load_cr3(pml4_phys: u64) {
    debug_assert!(pml4_phys & 0xFFF == 0, "PML4 must be page-aligned");
    unsafe {
        asm!("mov cr3, {}", in(reg) pml4_phys, options(nostack, preserves_flags));
    }
}

/// Read the current CR3 value.
pub fn read_cr3() -> u64 {
    let cr3: u64;
    unsafe { asm!("mov {}, cr3", out(reg) cr3, options(nostack)); }
    cr3
}

/// Enable the No-Execute bit in the EFER MSR.
pub fn enable_nx() {
    unsafe {
        asm!(
            "rdmsr",
            "or eax, {nx_bit}",
            "wrmsr",
            in("ecx") 0xC000_0080u32,
            nx_bit = const 1u32 << 11,
            out("eax") _,
            out("edx") _,
            options(nostack, preserves_flags)
        );
    }
}

/// Enable the Write-Protect bit in CR0 (kernel can't write read-only pages).
pub fn enable_write_protect() {
    unsafe {
        asm!(
            "mov rax, cr0",
            "or rax, 1 << 16",
            "mov cr0, rax",
            options(nostack, preserves_flags),
        );
    }
}

/// Check if NX is supported via CPUID.
pub fn nx_supported() -> bool {
    let (_, _, _, edx) = crate::arch::x86_64::cpuid::cpuid(0x8000_0001);
    (edx & (1 << 20)) != 0
}

// ── Initial identity mapping ──────────────────────────────────────

/// The kernel's PML4 table, statically allocated.
static mut KERNEL_PML4: Table = Table::new();

/// Identity-map the first `size` bytes of physical memory using
/// 2 MiB huge pages.  Sets up the recursive mapping at
/// PML4[RECURSIVE_INDEX].
///
/// During bootstrap we access child page tables by their **physical**
/// address (which equals their virtual address because UEFI identity-
/// maps low memory).  Recursive mapping addresses do not work yet —
/// CR3 still points at the UEFI PML4.
///
/// # Safety
///
/// Must be called once during early kernel init.  `size` must be a
/// multiple of 2 MiB.
/// Identity-map the first `size` bytes of physical memory using 2 MiB huge pages.
/// Sets up the recursive mapping at PML4[RECURSIVE_INDEX].
///
/// `physical_offset` must be the value subtracted from a kernel virtual address 
/// to get its corresponding physical address (e.g., `virt_addr - physical_offset`).
///
/// # Safety
///
/// Must be called once during early kernel init. `size` must be a multiple of 2 MiB.
pub unsafe fn identity_map(size: u64, physical_offset: u64) -> u64 {
    // ── Calculate how many tables we need ─────────────────────────
    let total_2mib_pages = size / HUGE_2MIB;
    let pds_needed = (total_2mib_pages + 511) / 512;
    let pdps_needed = (pds_needed + 511) / 512;
    let total_from_pool = pdps_needed + pds_needed;

    if total_from_pool > STATIC_POOL_SIZE as u64 {
       println!("Not enough static tables for identity mapping");
        return 0; // Not enough static tables configuration error
    }

    // ── Build the page tables ────────────────────────────────────
    let pml4 = unsafe { &mut *core::ptr::addr_of_mut!(KERNEL_PML4) };
    pml4.clear();
    
    let mut phys = 0u64;
    let mut pds_left = pds_needed;
    let mut pages_left = total_2mib_pages;

    for pml4_idx in 0..pdps_needed as usize {
        let pdp_ptr = alloc_static_table();
        if pdp_ptr.is_null() { return 0; }
        let pdp = unsafe { &mut *pdp_ptr };
        pdp.clear();

        // Convert virtual table pointer to physical address
        let pdp_phys = (pdp_ptr as u64) - physical_offset;
        pml4[pml4_idx].set_page(pdp_phys, true, false, false);

        let pds_in_this_pdp = pds_left.min(512);
        pds_left -= pds_in_this_pdp;

        for pdp_idx in 0..pds_in_this_pdp as usize {
            let pd_ptr = alloc_static_table();
            if pd_ptr.is_null() { return 0; }
            let pd = unsafe { &mut *pd_ptr };
            pd.clear();

            // Convert virtual table pointer to physical address
            let pd_phys = (pd_ptr as u64) - physical_offset;
            pdp[pdp_idx].set_page(pd_phys, true, false, false);

            let pages_in_this_pd = pages_left.min(512);
            pages_left -= pages_in_this_pd;

            for i in 0..pages_in_this_pd as usize {
                pd[i].set_huge_2mib(phys, true, false, false);
                phys += HUGE_2MIB;
            }
        }
    }

    // Recursive mapping: PML4[511] → PML4 itself.
    let pml4_virt = pml4 as *const Table as u64;
    let pml4_phys = pml4_virt - physical_offset;
    pml4[RECURSIVE_INDEX].set_page(pml4_phys, true, false, false);

    pml4_phys
}

// ── Static page-table pool (bootstrap only) ───────────────────────

/// Number of statically-allocated page tables for bootstrap.
const STATIC_POOL_SIZE: usize = 16;

static mut STATIC_TABLES: [Table; STATIC_POOL_SIZE] = [
    Table::new(), Table::new(), Table::new(), Table::new(),
    Table::new(), Table::new(), Table::new(), Table::new(),
    Table::new(), Table::new(), Table::new(), Table::new(),
    Table::new(), Table::new(), Table::new(), Table::new(),
];

/// Simple bootstrap allocator — returns the next free static table.
/// Not thread-safe; only for early kernel init.
static mut STATIC_NEXT: usize = 0;

fn alloc_static_table() -> *mut Table {
    let idx = unsafe {
        let i = STATIC_NEXT;
        if i >= STATIC_POOL_SIZE {
            return core::ptr::null_mut();
        }
        STATIC_NEXT = i + 1;
        i
    };
    unsafe { core::ptr::addr_of_mut!(STATIC_TABLES[idx]) }
}

/// Return the kernel PML4 physical address (for use after `identity_map`).
pub fn kernel_pml4_phys() -> u64 {
    let pml4 = unsafe { &*core::ptr::addr_of!(KERNEL_PML4) };
    pml4.as_phys_addr()
}
