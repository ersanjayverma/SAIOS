//! Thread-Local Storage — sets up the TLS block for a new thread/process.
//!
//! glibc/musl expect FS.base to point to the TCB (thread control block).
//! We allocate a TLS block per process and set FS.base via arch_prctl.

use alloc::vec;

/// Minimum TLS region size (glibc needs at least 512 bytes for its TCB).
const TLS_SIZE: usize = 4096;

/// __tls_get_addr - dynamic TLS access function.
/// This is called by the dynamic linker when resolving TLS relocations.
/// Returns a pointer to the TLS block offset for the given module and offset.
#[unsafe(no_mangle)]
pub extern "C" fn __tls_get_addr(addr: u64) -> *mut u8 {
    // For now, return the current TLS block base + offset
    // A full implementation would track per-module TLS offsets
    let tls_base = crate::process::with_current_process(|p| p.fs_base.fs_base).unwrap_or(0);
    if tls_base == 0 {
        // Fallback: return a static TLS block
        static TLS_BLOCK: [u8; TLS_SIZE] = [0; TLS_SIZE];
        return TLS_BLOCK.as_ptr() as *mut u8;
    }
    (tls_base + addr) as *mut u8
}

/// Allocate a TLS block and set FS.base to it.
/// Called from execve and clone(CLONE_THREAD).
pub fn setup_tls_for_process(proc: &mut crate::process::Process) -> u64 {
    // Allocate a TLS region in the process's address space
    let pages = TLS_SIZE.div_ceil(0x1000);
    let phys = crate::memory::alloc_frames(pages).unwrap_or(0);
    if phys == 0 {
        return 0;
    }

    // Map it
    let tls_virt = proc.mmap_base;
    proc.mmap_base += (pages * 0x1000) as u64;
    let target = if proc.address_space_pml4() != 0 {
        proc.address_space_pml4()
    } else {
        crate::memory::paging::active_pml4()
    };
    if crate::address_space_contract::AddressSpaceContract::map_user_frames_in(
        crate::address_space_contract::AddressSpaceHandle {
            id: target,
            pml4: target,
            owner_pid: proc.pid,
        },
        tls_virt,
        phys,
        pages,
    )
    .is_err()
    {
        crate::memory_contract::MemoryContract::free_frames(phys, pages, "tls_failed");
        return 0;
    }

    // The TCB pointer (tp) is stored at [FS:0] in the glibc/musl convention.
    // For x86_64: FS.base = address of the TCB struct.
    // The TCB at FS:0 must contain a pointer to itself (pthread_self()).
    unsafe {
        let tcb = tls_virt as *mut u64;
        *tcb = tls_virt; // self-pointer
    }

    // Set FS.base via MSR 0xC0000100
    unsafe {
        crate::arch::process::set_fs_base(tls_virt);
    }

    tls_virt
}
