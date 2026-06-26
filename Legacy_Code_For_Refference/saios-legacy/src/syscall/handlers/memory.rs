use super::{EFAULT, EINVAL, ENOMEM, ENOSYS};
use crate::process;

const PROT_NONE: u64 = 0;
const PROT_READ: u64 = 1;
const PROT_WRITE: u64 = 2;
const PROT_EXEC: u64 = 4;
const MAP_SHARED: u64 = 0x01;
const MAP_PRIVATE: u64 = 0x02;
const MAP_FIXED: u64 = 0x10;
const MAP_ANONYMOUS: u64 = 0x20;
const MAP_SUPPORTED: u64 = MAP_SHARED | MAP_PRIVATE | MAP_FIXED | MAP_ANONYMOUS;

fn pte_flags_for_prot(prot: u64) -> Result<u64, i64> {
    if prot & !(PROT_READ | PROT_WRITE | PROT_EXEC) != 0 {
        return Err(EINVAL);
    }

    let mut pte_flags = crate::memory::paging::PTE_PRESENT;
    if prot != PROT_NONE {
        pte_flags |= crate::memory::paging::PTE_USER;
    }
    if prot & PROT_WRITE != 0 {
        pte_flags |= crate::memory::paging::PTE_WRITABLE;
    }
    if prot & PROT_EXEC == 0 {
        pte_flags |= crate::memory::paging::PTE_NO_EXEC;
    }
    Ok(pte_flags)
}

fn mapped_range_overlaps(virt: u64, pages: usize) -> bool {
    for i in 0..pages {
        let Some(page) = virt.checked_add((i * 0x1000) as u64) else {
            return true;
        };
        if crate::memory::paging::translate(page).is_some() {
            return true;
        }
    }
    false
}

pub fn sys_mmap(addr: u64, len: u64, prot: u64, flags: u64, fd: u64, off: u64) -> i64 {
    let pages = (len as usize).div_ceil(0x1000);
    if pages == 0 {
        return EINVAL;
    }
    let Some(map_size) = (pages as u64).checked_mul(0x1000) else {
        return EINVAL;
    };
    let final_flags = match pte_flags_for_prot(prot) {
        Ok(flags) => flags,
        Err(errno) => return errno,
    };

    if flags & !MAP_SUPPORTED != 0 {
        return EINVAL;
    }
    if flags & MAP_FIXED != 0 || flags & MAP_SHARED != 0 {
        return ENOSYS;
    }
    if flags & MAP_PRIVATE == 0 || flags & MAP_ANONYMOUS == 0 {
        return ENOSYS;
    }
    if fd != !0u64 || off != 0 {
        return ENOSYS;
    }
    if addr != 0 && addr & 0xFFF != 0 {
        return EINVAL;
    }

    let virt = if addr == 0 {
        if let Some(Some(v)) = crate::process::with_current_process_mut(|p| {
            let v = p.mmap_base;
            let Some(next) = p.mmap_base.checked_add(map_size) else {
                return None;
            };
            p.mmap_base = next;
            Some(v)
        }) {
            v
        } else {
            return ENOMEM;
        }
    } else {
        addr
    };

    if virt.checked_add(map_size - 1).is_none() || mapped_range_overlaps(virt, pages) {
        return EINVAL;
    }

    let phys = match crate::memory_contract::MemoryContract::alloc_user_frames(pages, "mmap") {
        Some(p) => p,
        None => return ENOMEM,
    };

    if crate::address_space_contract::AddressSpaceContract::map_user_frames(virt, phys, pages)
        .is_err()
    {
        crate::memory_contract::MemoryContract::free_frames(phys, pages, "mmap_failed");
        return ENOMEM;
    }
    unsafe {
        core::ptr::write_bytes(virt as *mut u8, 0, map_size as usize);
    }
    if crate::address_space_contract::AddressSpaceContract::protect_user_range(
        virt,
        pages,
        final_flags,
    )
    .is_err()
    {
        crate::address_space_contract::AddressSpaceContract::unmap_user_range(virt, pages);
        return EFAULT;
    }
    virt as i64
}

pub fn sys_munmap(addr: u64, len: u64) -> i64 {
    let pages = (len as usize).div_ceil(0x1000);
    crate::address_space_contract::AddressSpaceContract::unmap_user_range(addr, pages);
    0
}

pub fn sys_mprotect(addr: u64, len: u64, prot: u64) -> i64 {
    if len == 0 || addr & 0xFFF != 0 {
        return EINVAL;
    }

    let pages = (len as usize).div_ceil(0x1000);
    let pte_flags = match pte_flags_for_prot(prot) {
        Ok(flags) => flags,
        Err(errno) => return errno,
    };

    if crate::address_space_contract::AddressSpaceContract::protect_user_range(
        addr, pages, pte_flags,
    )
    .is_err()
    {
        return EFAULT;
    }
    0
}

pub fn sys_mremap(old_addr: u64, old_len: u64, new_len: u64, _flags: u64) -> i64 {
    let new_addr = sys_mmap(0, new_len, 3, 0x22, !0u64, 0);
    if new_addr < 0 {
        return new_addr;
    }
    let copy_len = old_len.min(new_len) as usize;
    unsafe {
        core::ptr::copy_nonoverlapping(old_addr as *const u8, new_addr as *mut u8, copy_len);
    }
    sys_munmap(old_addr, old_len);
    new_addr
}

pub fn sys_madvise(_addr: u64, _len: u64, _advice: u64) -> i64 {
    ENOSYS
}

pub fn sys_brk(new_brk: u64) -> i64 {
    let cur_brk = match process::with_current_process(|p| p.brk) {
        Some(brk) => brk,
        None => return ENOMEM,
    };

    if new_brk == 0 || new_brk <= cur_brk {
        return cur_brk as i64;
    }

    let map_from = (cur_brk + 0xFFF) & !0xFFF;
    let map_to = (new_brk + 0xFFF) & !0xFFF;
    let pages = ((map_to - map_from) / 0x1000) as usize;

    if pages > 0 {
        let phys = match crate::memory_contract::MemoryContract::alloc_user_frames(pages, "brk") {
            Some(p) => p,
            None => return ENOMEM,
        };
        if crate::address_space_contract::AddressSpaceContract::map_user_frames(
            map_from, phys, pages,
        )
        .is_err()
        {
            crate::memory_contract::MemoryContract::free_frames(phys, pages, "brk_failed");
            return ENOMEM;
        }
    }

    let _ = process::with_current_process_mut(|p| p.brk = new_brk);
    new_brk as i64
}
