//! x86_64 4-level page table manager.
//!
//! Our boot.s already identity-maps the first 1 GiB with 2 MiB huge pages.
//! This module adds:
//!   - translate(virt)  → Option<PhysAddr>
//!   - map(virt, phys, flags)  — create a 4 KiB mapping
//!   - unmap(virt)             — remove a mapping
//!   - map_range(virt, phys, bytes, flags)
//!   - split_huge_page(virt)   — split a 2 MiB page into 512 4 KiB pages
//!
//! Identity mapping (virt == phys for kernel) is assumed for page table
//! access — we dereference page table pointers directly.

use crate::memory::frame::{FRAME_SIZE, FrameAllocator};
use alloc::collections::BTreeMap;
use spin::Mutex;
use x86_64::registers::control::Cr3;

// Page table entry flags
pub const PTE_PRESENT: u64 = 1 << 0;
pub const PTE_WRITABLE: u64 = 1 << 1;
pub const PTE_USER: u64 = 1 << 2;
pub const PTE_HUGE: u64 = 1 << 7;
pub const PTE_COW: u64 = 1 << 9;
pub const PTE_NO_EXEC: u64 = 1 << 63;
pub const PTE_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

pub const KERNEL_FLAGS: u64 = PTE_PRESENT | PTE_WRITABLE;
pub const USER_FLAGS: u64 = PTE_PRESENT | PTE_WRITABLE | PTE_USER;

/// Physical address of the currently active PML4 (from CR3).
#[inline]
pub fn active_pml4() -> u64 {
    let (pml4_frame, _) = Cr3::read();
    pml4_frame.start_address().as_u64()
}

/// Translate a virtual address in the *active* address space.
pub fn translate(virt: u64) -> Option<u64> {
    translate_in(active_pml4(), virt)
}

/// Translate a virtual address by walking a *specific* PML4's page tables.
/// `pml4_phys` is the physical (== virtual, identity-mapped) address of the PML4.
pub fn translate_in(pml4_phys: u64, virt: u64) -> Option<u64> {
    translate_entry_in(pml4_phys, virt).map(|(phys, _flags)| phys)
}

/// Translate a virtual address and return both the resolved physical address
/// and the leaf page-table flags. Used by ring-3 transition diagnostics.
pub fn translate_entry_in(pml4_phys: u64, virt: u64) -> Option<(u64, u64)> {
    let pml4 = pml4_phys as *const u64;

    unsafe {
        let pml4e = *pml4.add(pml4_index(virt));
        if pml4e & PTE_PRESENT == 0 {
            return None;
        }

        let pdpt = (pml4e & PTE_ADDR_MASK) as *const u64;
        let pdpte = *pdpt.add(pdpt_index(virt));
        if pdpte & PTE_PRESENT == 0 {
            return None;
        }
        if pdpte & PTE_HUGE != 0 {
            // 1 GiB page
            return Some((
                (pdpte & 0x000F_FFFF_C000_0000) | (virt & 0x3FFF_FFFF),
                pdpte & !PTE_ADDR_MASK,
            ));
        }

        let pd = (pdpte & PTE_ADDR_MASK) as *const u64;
        let pde = *pd.add(pd_index(virt));
        if pde & PTE_PRESENT == 0 {
            return None;
        }
        if pde & PTE_HUGE != 0 {
            // 2 MiB page
            return Some((
                (pde & 0x000F_FFFF_FFE0_0000) | (virt & 0x001F_FFFF),
                pde & !PTE_ADDR_MASK,
            ));
        }

        let pt = (pde & PTE_ADDR_MASK) as *const u64;
        let pte = *pt.add(pt_index(virt));
        if pte & PTE_PRESENT == 0 {
            return None;
        }
        Some(((pte & PTE_ADDR_MASK) | (virt & 0xFFF), pte & !PTE_ADDR_MASK))
    }
}

/// Write a value to user memory safely through the active PML4.
/// Returns true if the write succeeded, false if unmapped or not writable.
pub fn write_user<T: Copy>(virt: u64, val: T) -> bool {
    write_user_in(active_pml4(), virt, val)
}

/// Write a value to user memory through a specific process PML4.
pub fn write_user_in<T: Copy>(pml4: u64, virt: u64, val: T) -> bool {
    let result = translate_entry_in(pml4, virt);
    let (phys, flags) = match result {
        Some(r) => r,
        None => return false,
    };
    if flags & PTE_PRESENT == 0 {
        return false;
    }
    if flags & PTE_WRITABLE == 0 && flags & PTE_COW != 0 {
        let cow_result =
            crate::memory_contract::MemoryContract::resolve_current_cow_fault(pml4, virt);
        if cow_result.ok() != Some(true) {
            return false;
        }
        return write_user_in(pml4, virt, val);
    }
    if flags & (PTE_PRESENT | PTE_WRITABLE) != (PTE_PRESENT | PTE_WRITABLE) {
        return false;
    }
    unsafe {
        core::ptr::write_volatile(phys as *mut T, val);
    }
    true
}

/// Dump page table entries for a virtual address to serial.
/// Prints the PML4/PDPT/PD/PT entries along with their flags.
/// Used for debugging page faults by examining the mapping chain.
pub fn dump_page_mapping_in(pml4_phys: u64, virt: u64) {
    let pml4 = pml4_phys as *const u64;

    unsafe {
        let pml4e = *pml4.add(pml4_index(virt));
        crate::serial_print!("    PML4[{:3}] = {:#018x}", pml4_index(virt), pml4e);
        if pml4e & PTE_PRESENT == 0 {
            crate::serial_println!("  (not present)");
            return;
        }
        crate::serial_println!();

        let pdpt = (pml4e & PTE_ADDR_MASK) as *const u64;
        let pdpte = *pdpt.add(pdpt_index(virt));
        crate::serial_print!("    PDPT[{:2}] = {:#018x}", pdpt_index(virt), pdpte);
        if pdpte & PTE_PRESENT == 0 {
            crate::serial_println!("  (not present)");
            return;
        }
        if pdpte & PTE_HUGE != 0 {
            crate::serial_println!("  [1 GiB huge page]");
            return;
        }
        crate::serial_println!();

        let pd = (pdpte & PTE_ADDR_MASK) as *const u64;
        let pde = *pd.add(pd_index(virt));
        crate::serial_print!("    PD[{:3}] = {:#018x}", pd_index(virt), pde);
        if pde & PTE_PRESENT == 0 {
            crate::serial_println!("  (not present)");
            return;
        }
        if pde & PTE_HUGE != 0 {
            crate::serial_println!("  [2 MiB huge page]");
            return;
        }
        crate::serial_println!();

        let pt = (pde & PTE_ADDR_MASK) as *const u64;
        let pte = *pt.add(pt_index(virt));
        crate::serial_print!("    PT[{:3}] = {:#018x}", pt_index(virt), pte);
        if pte & PTE_PRESENT == 0 {
            crate::serial_println!("  (not present)");
        } else {
            crate::serial_println!("  [4 KiB page]");
        }
    }
}

/// Wrapper that uses the active PML4.
pub fn dump_page_mapping(virt: u64) {
    dump_page_mapping_in(active_pml4(), virt);
}

/// Split a 2 MiB huge page at the given virtual address into 512 4 KiB pages.
/// This is needed when mapping 4 KiB pages in a region covered by huge pages.
pub fn split_huge_page_in(
    pml4_phys: u64,
    virt: u64,
    alloc: &mut FrameAllocator,
) -> Result<(), &'static str> {
    let virt = align_down(virt, 2 * 1024 * 1024); // Align to 2 MiB boundary
    let pml4 = pml4_phys as *mut u64;
    let inter = PTE_PRESENT | PTE_WRITABLE;

    unsafe {
        let pdpt = ensure_table(pml4.add(pml4_index(virt)), inter, alloc)?;
        let pd = ensure_table(pdpt.add(pdpt_index(virt)), inter, alloc)?;
        let pde_ptr = pd.add(pd_index(virt));
        let pde = *pde_ptr;

        if pde & PTE_PRESENT == 0 || pde & PTE_HUGE == 0 {
            return Ok(()); // Not a huge page, nothing to split
        }

        // Get the physical base address of the 2 MiB page
        let huge_phys = pde & PTE_ADDR_MASK;
        let flags = pde & !PTE_ADDR_MASK;

        // Allocate a page table
        let pt_frame = alloc
            .alloc()
            .ok_or("paging: OOM allocating PT for huge page split")?;
        crate::memory_contract::MemoryContract::record_page_table_frame(
            pt_frame,
            "split_huge_page",
        );
        let pt = pt_frame as *mut u64;
        core::ptr::write_bytes(pt, 0, 512);

        // Create 512 4 KiB entries mapping the same physical range
        for i in 0..512 {
            let entry_phys = huge_phys + (i as u64) * FRAME_SIZE as u64;
            *pt.add(i) = entry_phys | flags;
        }

        // Update the PD entry to point to the new PT (clear HUGE bit)
        *pde_ptr = pt_frame | inter;

        // Flush TLB for the entire 2 MiB range
        for i in 0..512 {
            flush_tlb(virt + (i as u64) * FRAME_SIZE as u64);
        }

        crate::serial_println!("[paging] split 2 MiB huge page at {:#x}", virt);
    }
    Ok(())
}

/// Map one 4 KiB page in the *active* address space.
pub fn map(
    virt: u64,
    phys: u64,
    flags: u64,
    alloc: &mut FrameAllocator,
) -> Result<(), &'static str> {
    map_in(active_pml4(), virt, phys, flags, alloc)
}

/// Map one 4 KiB virtual page to one physical frame in a *specific* PML4.
/// Allocates intermediate page-table frames from `alloc` as needed.
/// Automatically splits huge pages if needed.
pub fn map_in(
    pml4_phys: u64,
    virt: u64,
    phys: u64,
    flags: u64,
    alloc: &mut FrameAllocator,
) -> Result<(), &'static str> {
    let virt = virt & !0xFFF;
    let phys = phys & !0xFFF;

    let pml4 = pml4_phys as *mut u64;
    // Intermediate tables must carry the USER bit if the leaf is user-accessible,
    // otherwise ring-3 access is denied at the higher levels of the walk.
    let inter = PTE_PRESENT | PTE_WRITABLE | (flags & PTE_USER);

    unsafe {
        // PML4 → PDPT
        let pdpt = ensure_table(pml4.add(pml4_index(virt)), inter, alloc)?;
        // PDPT → PD
        let pd = ensure_table(pdpt.add(pdpt_index(virt)), inter, alloc)?;
        // PD → PT  (split huge page if needed)
        let pde = pd.add(pd_index(virt));
        if *pde & PTE_PRESENT != 0 && *pde & PTE_HUGE != 0 {
            // Split the 2 MiB huge page into 4 KiB pages
            split_huge_page_in(pml4_phys, virt, alloc)?;
        }
        let pt = ensure_table(pde, inter, alloc)?;
        // PT entry
        let pte = pt.add(pt_index(virt));
        *pte = phys | flags;
        flush_tlb(virt);
    }
    Ok(())
}

/// Remove a 4 KiB mapping in the *active* address space and flush TLB.
pub fn unmap(virt: u64) {
    unmap_in(active_pml4(), virt);
}

/// Remove a 4 KiB mapping in a *specific* PML4.
pub fn unmap_in(pml4_phys: u64, virt: u64) {
    let pml4 = pml4_phys as *mut u64;
    unsafe {
        if let Ok(pdpt) = get_table(pml4.add(pml4_index(virt)))
            && let Ok(pd) = get_table(pdpt.add(pdpt_index(virt)))
            && let Ok(pt) = get_table(pd.add(pd_index(virt)))
            && *pt.add(pt_index(virt)) & PTE_PRESENT != 0
        {
            *pt.add(pt_index(virt)) = 0;
            if pml4_phys == active_pml4() {
                flush_tlb(virt);
            }
        }
    }
}

/// Create a fresh address space (a new PML4) that shares the kernel's
/// identity mapping but has a private, empty user region.
///
/// The boot page tables place the 0-128 GiB identity map (kernel code, heap,
/// frame-backed memory, page-table frames, MMIO/framebuffer) under PML4[0].
/// Copy only that kernel entry. PML4[1..] hold process-private user mappings
/// and must start empty so fork/exec cannot share page-table ownership.
///
/// Returns the physical address of the new PML4.
pub fn new_address_space(alloc: &mut FrameAllocator) -> Result<u64, &'static str> {
    let frame = alloc.alloc().ok_or("paging: OOM allocating PML4")?;
    crate::memory_contract::MemoryContract::record_page_table_frame(frame, "new_address_space");
    let new_pml4 = frame as *mut u64;
    let kernel_pml4 = active_pml4() as *const u64;
    unsafe {
        // Zero the entire new PML4
        core::ptr::write_bytes(new_pml4, 0, 512);

        *new_pml4.add(0) = *kernel_pml4.add(0);
    }
    Ok(frame)
}

/// Load `pml4_phys` into CR3, switching the active address space.
/// No-op if it is already active (avoids a needless full TLB flush).
#[inline]
pub fn switch_address_space(pml4_phys: u64) {
    if pml4_phys == 0 || pml4_phys == active_pml4() {
        return;
    }
    unsafe {
        core::arch::asm!("mov cr3, {}", in(reg) pml4_phys, options(nostack, preserves_flags));
    }
}

/// Map a contiguous virtual range to a contiguous physical range.
pub fn map_range(
    virt_start: u64,
    phys_start: u64,
    bytes: usize,
    flags: u64,
    alloc: &mut FrameAllocator,
) -> Result<(), &'static str> {
    let pages = bytes.div_ceil(FRAME_SIZE);
    for i in 0..pages as u64 {
        map(
            virt_start + i * FRAME_SIZE as u64,
            phys_start + i * FRAME_SIZE as u64,
            flags,
            alloc,
        )?;
    }
    Ok(())
}

/// Copy every present user-space page from `src_pml4` into `dst_pml4`, giving
/// the destination its own freshly-allocated, byte-identical frames (eager
/// copy — no copy-on-write yet).  Used by fork() to isolate the child.
///
/// Walks the page tables directly rather than scanning the virtual range, and
/// only touches the lower-half user slots (PML4[1..256]); PML4[0] is the shared
/// kernel mapping and is left pointing at the same tables in both spaces.
pub fn copy_user_space(
    src_pml4: u64,
    dst_pml4: u64,
    alloc: &mut FrameAllocator,
) -> Result<(), &'static str> {
    let src = src_pml4 as *const u64;
    unsafe {
        for i4 in 1..256usize {
            let e4 = *src.add(i4);
            if e4 & PTE_PRESENT == 0 {
                continue;
            }
            let pdpt = (e4 & PTE_ADDR_MASK) as *const u64;
            for i3 in 0..512usize {
                let e3 = *pdpt.add(i3);
                if e3 & PTE_PRESENT == 0 || e3 & PTE_HUGE != 0 {
                    continue;
                }
                let pd = (e3 & PTE_ADDR_MASK) as *const u64;
                for i2 in 0..512usize {
                    let e2 = *pd.add(i2);
                    if e2 & PTE_PRESENT == 0 || e2 & PTE_HUGE != 0 {
                        continue;
                    }
                    let pt = (e2 & PTE_ADDR_MASK) as *const u64;
                    for i1 in 0..512usize {
                        let e1 = *pt.add(i1);
                        if e1 & PTE_PRESENT == 0 {
                            continue;
                        }
                        let virt = ((i4 as u64) << 39)
                            | ((i3 as u64) << 30)
                            | ((i2 as u64) << 21)
                            | ((i1 as u64) << 12);
                        let flags = e1 & !PTE_ADDR_MASK;
                        let src_phys = e1 & PTE_ADDR_MASK;
                        let new_phys = alloc.alloc().ok_or("fork: OOM copying user page")?;
                        core::ptr::copy_nonoverlapping(
                            src_phys as *const u8,
                            new_phys as *mut u8,
                            0x1000,
                        );
                        // If map_in fails, free the page we just allocated
                        if map_in(dst_pml4, virt, new_phys, flags, alloc).is_err() {
                            alloc.free(new_phys);
                            return Err("paging: OOM copying user page");
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Share user pages between parent and child using copy-on-write.
/// Originally writable pages become read-only COW in both address spaces.
/// Originally read-only pages stay read-only shared mappings.
pub fn clone_user_space_cow(
    src_pml4: u64,
    dst_pml4: u64,
    alloc: &mut FrameAllocator,
) -> Result<(), &'static str> {
    let src = src_pml4 as *const u64;
    unsafe {
        for i4 in 1..256usize {
            let e4 = *src.add(i4);
            if e4 & PTE_PRESENT == 0 {
                continue;
            }
            let pdpt = (e4 & PTE_ADDR_MASK) as *const u64;
            for i3 in 0..512usize {
                let e3 = *pdpt.add(i3);
                if e3 & PTE_PRESENT == 0 || e3 & PTE_HUGE != 0 {
                    continue;
                }
                let pd = (e3 & PTE_ADDR_MASK) as *const u64;
                for i2 in 0..512usize {
                    let e2 = *pd.add(i2);
                    if e2 & PTE_PRESENT == 0 || e2 & PTE_HUGE != 0 {
                        continue;
                    }
                    let pt = (e2 & PTE_ADDR_MASK) as *const u64;
                    for i1 in 0..512usize {
                        let e1 = *pt.add(i1);
                        if e1 & PTE_PRESENT == 0 {
                            continue;
                        }
                        let virt = ((i4 as u64) << 39)
                            | ((i3 as u64) << 30)
                            | ((i2 as u64) << 21)
                            | ((i1 as u64) << 12);
                        let phys = e1 & PTE_ADDR_MASK;
                        let flags = e1 & !PTE_ADDR_MASK;

                        let shared_flags = if flags & PTE_WRITABLE != 0 {
                            (flags | PTE_COW) & !PTE_WRITABLE
                        } else {
                            flags & !PTE_COW
                        };
                        if map_in(dst_pml4, virt, phys, shared_flags, alloc).is_err() {
                            return Err("paging: OOM in clone_user_space_cow");
                        }
                        shared_page_retain(phys, shared_flags);
                        if update_page_flags_in(src_pml4, virt, shared_flags).is_err() {
                            unmap_in(dst_pml4, virt);
                            shared_page_release(phys);
                            return Err("paging: OOM in clone_user_space_cow");
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Handle a write fault on a copy-on-write user page.
/// Returns true if the page was fixed up and the faulting instruction can be retried.
pub fn resolve_cow_fault_in(pml4_phys: u64, virt: u64) -> Result<bool, &'static str> {
    let page = virt & !0xFFF;
    let pte = unsafe { leaf_pte_ptr_in(pml4_phys, page) }.ok_or("paging: COW page not mapped")?;

    unsafe {
        let entry = *pte;
        if entry & PTE_PRESENT == 0 || entry & PTE_COW == 0 {
            return Ok(false);
        }

        let phys = entry & PTE_ADDR_MASK;
        let flags = entry & !PTE_ADDR_MASK;
        let new_flags = (flags | PTE_WRITABLE) & !PTE_COW;

        if shared_page_refcount(phys) > 1 {
            let new_phys = crate::memory::alloc_frame().ok_or("paging: OOM resolving COW")?;
            core::ptr::copy_nonoverlapping(phys as *const u8, new_phys as *mut u8, 0x1000);
            shared_page_release(phys);
            *pte = new_phys | new_flags;
            crate::memory_contract::MemoryContract::record_mapping(
                new_phys,
                1,
                new_flags,
                "resolve_cow_new",
            );
        } else {
            shared_page_forget_if_unique(phys, new_flags);
            *pte = phys | new_flags;
        }

        if pml4_phys == active_pml4() {
            flush_tlb(page);
        }
    }

    Ok(true)
}

/// Destroy a private user address space and release its user pages.
/// Handles both 4 KiB pages and huge pages (2 MiB and 1 GiB).
/// Callers must have switched away from `pml4_phys` if it was active in CR3.
pub fn destroy_address_space(pml4_phys: u64) -> Result<(), &'static str> {
    if pml4_phys == 0 {
        return Ok(());
    }
    if pml4_phys == active_pml4() {
        return Err("paging: refusing to destroy active address space");
    }

    let mut fa = crate::memory::FRAME_ALLOCATOR.lock();
    unsafe {
        let pml4 = pml4_phys as *mut u64;
        for i4 in 1..256usize {
            let e4 = *pml4.add(i4);
            if e4 & PTE_PRESENT == 0 {
                continue;
            }

            let pdpt_phys = e4 & PTE_ADDR_MASK;
            let pdpt = pdpt_phys as *mut u64;
            for i3 in 0..512usize {
                let e3 = *pdpt.add(i3);
                if e3 & PTE_PRESENT == 0 {
                    continue;
                }

                if e3 & PTE_HUGE != 0 {
                    // 1 GiB huge page - release and free directly
                    let phys = e3 & PTE_ADDR_MASK;
                    release_shared_or_free(phys, &mut fa);
                    *pdpt.add(i3) = 0;
                    continue;
                }

                let pd_phys = e3 & PTE_ADDR_MASK;
                let pd = pd_phys as *mut u64;
                for i2 in 0..512usize {
                    let e2 = *pd.add(i2);
                    if e2 & PTE_PRESENT == 0 {
                        continue;
                    }

                    if e2 & PTE_HUGE != 0 {
                        // 2 MiB huge page - release and free directly
                        let phys = e2 & PTE_ADDR_MASK;
                        release_shared_or_free(phys, &mut fa);
                        *pd.add(i2) = 0;
                        continue;
                    }

                    let pt_phys = e2 & PTE_ADDR_MASK;
                    let pt = pt_phys as *mut u64;
                    for i1 in 0..512usize {
                        let entry = *pt.add(i1);
                        if entry & PTE_PRESENT == 0 {
                            continue;
                        }
                        let phys = entry & PTE_ADDR_MASK;
                        release_shared_or_free(phys, &mut fa);
                        *pt.add(i1) = 0;
                    }
                    fa.free(pt_phys);
                    crate::memory_contract::MemoryContract::record_released_frame(
                        pt_phys,
                        "destroy_address_space_pt",
                    );
                    *pd.add(i2) = 0;
                }
                fa.free(pd_phys);
                crate::memory_contract::MemoryContract::record_released_frame(
                    pd_phys,
                    "destroy_address_space_pd",
                );
                *pdpt.add(i3) = 0;
            }
            fa.free(pdpt_phys);
            crate::memory_contract::MemoryContract::record_released_frame(
                pdpt_phys,
                "destroy_address_space_pdpt",
            );
            *pml4.add(i4) = 0;
        }
        fa.free(pml4_phys);
        crate::memory_contract::MemoryContract::record_released_frame(
            pml4_phys,
            "destroy_address_space_pml4",
        );
    }
    Ok(())
}

// -- Helpers ----------------------------------------------------------------

fn pml4_index(v: u64) -> usize {
    ((v >> 39) & 0x1FF) as usize
}
fn pdpt_index(v: u64) -> usize {
    ((v >> 30) & 0x1FF) as usize
}
fn pd_index(v: u64) -> usize {
    ((v >> 21) & 0x1FF) as usize
}
fn pt_index(v: u64) -> usize {
    ((v >> 12) & 0x1FF) as usize
}

fn shared_page_retain(phys: u64, flags: u64) {
    crate::memory_contract::MemoryContract::retain_shared_page(phys, flags, "cow_retain");
    // Note: TLB shootdown for shared pages across CPUs requires IPIs.
    // Current implementation only flushes TLB on the local CPU (active_pml4).
    // For multi-CPU support, implement APIC IPI-based TLB shootdown.
}

fn shared_page_refcount(phys: u64) -> u32 {
    crate::memory_contract::MemoryContract::shared_page_refcount(phys)
}

fn shared_page_release(phys: u64) {
    crate::memory_contract::MemoryContract::release_shared_page(phys, "cow_release");
}

fn shared_page_forget_if_unique(phys: u64, flags: u64) {
    crate::memory_contract::MemoryContract::forget_shared_if_unique(phys, flags, "cow_unique");
}

fn release_shared_or_free(phys: u64, alloc: &mut FrameAllocator) {
    crate::memory_contract::MemoryContract::release_shared_or_free_with_allocator(
        phys,
        alloc,
        "address_space_destroy",
    );
}

pub fn update_page_flags_in(pml4_phys: u64, virt: u64, flags: u64) -> Result<(), &'static str> {
    let page = virt & !0xFFF;
    let pte = unsafe { leaf_pte_ptr_in(pml4_phys, page) }.ok_or("paging: page not mapped")?;
    unsafe {
        let existing = *pte;
        let phys = existing & PTE_ADDR_MASK;
        let preserved = existing & PTE_COW;
        let writable = if preserved != 0 {
            flags & !PTE_WRITABLE
        } else {
            flags
        };
        *pte = phys | preserved | writable;
        crate::memory_contract::MemoryContract::update_page_flags(
            phys,
            preserved | writable,
            "update_page_flags",
        );
    }
    if pml4_phys == active_pml4() {
        flush_tlb(page);
    }
    Ok(())
}

pub fn update_user_page_flags(virt: u64, flags: u64) -> Result<(), &'static str> {
    update_page_flags_in(active_pml4(), virt, flags)
}

unsafe fn leaf_pte_ptr_in(pml4_phys: u64, virt: u64) -> Option<*mut u64> {
    unsafe {
        let pml4 = pml4_phys as *mut u64;

        let pml4e = *pml4.add(pml4_index(virt));
        if pml4e & PTE_PRESENT == 0 {
            return None;
        }

        let pdpt = (pml4e & PTE_ADDR_MASK) as *mut u64;
        let pdpte = *pdpt.add(pdpt_index(virt));
        if pdpte & PTE_PRESENT == 0 || pdpte & PTE_HUGE != 0 {
            return None;
        }

        let pd = (pdpte & PTE_ADDR_MASK) as *mut u64;
        let pde = *pd.add(pd_index(virt));
        if pde & PTE_PRESENT == 0 || pde & PTE_HUGE != 0 {
            return None;
        }

        let pt = (pde & PTE_ADDR_MASK) as *mut u64;
        Some(pt.add(pt_index(virt)))
    }
}

/// Return the child table pointer from `entry`, creating one if absent.
/// `inter` is the flag set for a newly created intermediate entry; if the entry
/// already exists, its USER bit is widened to match so a kernel-only intermediate
/// table can be promoted to also cover a user mapping.
unsafe fn ensure_table(
    entry: *mut u64,
    inter: u64,
    alloc: &mut FrameAllocator,
) -> Result<*mut u64, &'static str> {
    unsafe {
        if *entry & PTE_PRESENT != 0 {
            // Widen permissions if this table must now also serve a user mapping.
            *entry |= inter & (PTE_USER | PTE_WRITABLE);
            return Ok((*entry & PTE_ADDR_MASK) as *mut u64);
        }
        // Allocate a new zeroed page-table frame
        let frame = alloc.alloc().ok_or("paging: OOM allocating page table")?;
        crate::memory_contract::MemoryContract::record_page_table_frame(frame, "ensure_table");
        // Zero the new table (identity mapped — physical == virtual for kernel)
        let ptr = frame as *mut u64;
        core::ptr::write_bytes(ptr, 0, 512);
        *entry = frame | inter;
        Ok(ptr)
    }
}

unsafe fn get_table(entry: *mut u64) -> Result<*mut u64, ()> {
    unsafe {
        if *entry & PTE_PRESENT != 0 {
            Ok((*entry & PTE_ADDR_MASK) as *mut u64)
        } else {
            Err(())
        }
    }
}

pub(crate) fn flush_tlb(virt: u64) {
    unsafe {
        core::arch::asm!("invlpg [{}]", in(reg) virt, options(nostack, preserves_flags));
    }
}

fn align_down(addr: u64, align: u64) -> u64 {
    addr & !(align - 1)
}
