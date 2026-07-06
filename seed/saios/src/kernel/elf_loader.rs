use crate::kernel::constants::{
    AT_ENTRY, AT_EXECFN, AT_NULL, AT_PHDR, AT_PHENT, AT_PHNUM, AT_PAGESZ,
    ELFCLASS64, ELFDATA2LSB, EM_X86_64, ET_DYN, ET_EXEC, EV_CURRENT, ELF_MAGIC,
    DT_NULL, DT_RELA, DT_RELASZ, DT_RELAENT, DT_RELACOUNT, R_X86_64_RELATIVE,
    PF_R, PF_W, PF_X, PT_DYNAMIC, PT_INTERP, PT_LOAD,
    USER_STACK_BASE, USER_STACK_PAGES,
};
use alloc::format;
use alloc::vec::Vec;

use crate::pmm;
use crate::saifs;
use crate::saifs::Handle;
use crate::vmm;

const ET_EXEC_ISOLATED_ADDRESS_SPACE: bool = true;
const ET_DYN_PROCESS_ADDRESS_SPACE: bool = false;
const ELF_TRACE_LOGS: bool = false;

macro_rules! elf_trace {
    ($($arg:tt)*) => {
        if ELF_TRACE_LOGS {
            crate::console::println!($($arg)*);
        }
    };
}

#[derive(Copy, Clone)]
struct ElfHeader {
    e_type: u16,
    e_entry: u64,
    e_phoff: u64,
    e_phentsize: u16,
    e_phnum: u16,
}

#[derive(Copy, Clone)]
struct ProgramHeader {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_filesz: u64,
    p_memsz: u64,
}

#[derive(Copy, Clone)]
struct DynamicInfo {
    rela_addr: u64,
    rela_sz: u64,
    rela_ent: u64,
    rela_count: u64,
}

#[derive(Copy, Clone)]
struct MapRange {
    start: u64,
    end: u64,
    flags: u64,
}

struct LoadedImage {
    entry: u64,
    mapped_starts: Vec<u64>,
    mapped_ranges: Vec<MapRange>,
}

fn read_u16_le(bytes: &[u8], off: usize) -> Option<u16> {
    let end = off.checked_add(2)?;
    let s = bytes.get(off..end)?;
    Some(u16::from_le_bytes([s[0], s[1]]))
}

fn read_u32_le(bytes: &[u8], off: usize) -> Option<u32> {
    let end = off.checked_add(4)?;
    let s = bytes.get(off..end)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn read_u64_le(bytes: &[u8], off: usize) -> Option<u64> {
    let end = off.checked_add(8)?;
    let s = bytes.get(off..end)?;
    Some(u64::from_le_bytes([
        s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7],
    ]))
}

fn read_i64_le(bytes: &[u8], off: usize) -> Option<i64> {
    read_u64_le(bytes, off).map(|v| v as i64)
}

fn align_down(v: u64, a: u64) -> u64 {
    v & !(a - 1)
}

fn align_up(v: u64, a: u64) -> u64 {
    (v + a - 1) & !(a - 1)
}

fn checked_add(a: u64, b: u64, err: &'static str) -> Result<u64, &'static str> {
    a.checked_add(b).ok_or(err)
}

fn parse_header(bytes: &[u8]) -> Result<ElfHeader, &'static str> {
    if bytes.len() < 64 {
        return Err("elf: file too small");
    }
    if bytes.get(0..4) != Some(&ELF_MAGIC) {
        return Err("elf: bad magic");
    }
    if bytes[4] != ELFCLASS64 {
        return Err("elf: unsupported class");
    }
    if bytes[5] != ELFDATA2LSB {
        return Err("elf: unsupported endianness");
    }
    if bytes[6] != EV_CURRENT {
        return Err("elf: unsupported ident version");
    }
    let machine = read_u16_le(bytes, 18).ok_or("elf: truncated header")?;
    if machine != EM_X86_64 {
        return Err("elf: unsupported machine");
    }

    let e_type = read_u16_le(bytes, 16).ok_or("elf: truncated header")?;
    if e_type != ET_EXEC && e_type != ET_DYN {
        return Err("elf: unsupported type");
    }

    Ok(ElfHeader {
        e_type,
        e_entry: read_u64_le(bytes, 24).ok_or("elf: truncated header")?,
        e_phoff: read_u64_le(bytes, 32).ok_or("elf: truncated header")?,
        e_phentsize: read_u16_le(bytes, 54).ok_or("elf: truncated header")?,
        e_phnum: read_u16_le(bytes, 56).ok_or("elf: truncated header")?,
    })
}

fn parse_program_headers(bytes: &[u8], h: &ElfHeader) -> Result<Vec<ProgramHeader>, &'static str> {
    if h.e_phnum == 0 {
        return Err("elf: no program headers");
    }
    if h.e_phentsize < 56 {
        return Err("elf: invalid phentsize");
    }

    let mut out = Vec::new();
    let phoff = usize::try_from(h.e_phoff).map_err(|_| "elf: phoff out of range")?;
    let phentsize = h.e_phentsize as usize;

    for i in 0..h.e_phnum as usize {
        let off = phoff
            .checked_add(i.checked_mul(phentsize).ok_or("elf: ph overflow")?)
            .ok_or("elf: ph overflow")?;
        let end = off.checked_add(56).ok_or("elf: ph overflow")?;
        if end > bytes.len() {
            return Err("elf: program headers exceed file");
        }

        let ph = ProgramHeader {
            p_type: read_u32_le(bytes, off).ok_or("elf: truncated ph")?,
            p_flags: read_u32_le(bytes, off + 4).ok_or("elf: truncated ph")?,
            p_offset: read_u64_le(bytes, off + 8).ok_or("elf: truncated ph")?,
            p_vaddr: read_u64_le(bytes, off + 16).ok_or("elf: truncated ph")?,
            p_filesz: read_u64_le(bytes, off + 32).ok_or("elf: truncated ph")?,
            p_memsz: read_u64_le(bytes, off + 40).ok_or("elf: truncated ph")?,
        };
        if ph.p_type == PT_LOAD {
            if ph.p_filesz > ph.p_memsz {
                return Err("elf: PT_LOAD filesz > memsz");
            }
            let file_end = checked_add(ph.p_offset, ph.p_filesz, "elf: PT_LOAD range overflow")?;
            if file_end > bytes.len() as u64 {
                return Err("elf: PT_LOAD exceeds file");
            }
        }
        out.push(ph);
    }

    Ok(out)
}

fn segment_flags(ph_flags: u32) -> u64 {
    let mut flags = vmm::FLAG_USER;
    if (ph_flags & PF_R) != 0 {
        flags |= vmm::FLAG_READ;
    }
    if (ph_flags & PF_W) != 0 {
        flags |= vmm::FLAG_WRITE;
    }
    if (ph_flags & PF_X) != 0 {
        flags |= vmm::FLAG_EXEC;
    }
    flags
}

fn add_map_range(ranges: &mut Vec<MapRange>, mut newr: MapRange) {
    let mut i = 0usize;
    while i < ranges.len() {
        let r = ranges[i];
        let overlap = !(newr.end < r.start || r.end < newr.start);
        let adjacent = newr.end == r.start || r.end == newr.start;
        if overlap || adjacent {
            newr.start = newr.start.min(r.start);
            newr.end = newr.end.max(r.end);
            newr.flags |= r.flags;
            ranges.remove(i);
            i = 0;
            continue;
        }
        i += 1;
    }
    ranges.push(newr);
}

fn runtime_base(h: &ElfHeader, image_base: u64) -> u64 {
    if h.e_type == ET_DYN { image_base } else { 0 }
}

fn runtime_entry(h: &ElfHeader, base: u64) -> u64 {
    h.e_entry.saturating_add(base)
}

fn parse_dynamic(
    bytes: &[u8],
    phs: &[ProgramHeader],
    base: u64,
) -> Result<Option<DynamicInfo>, &'static str> {
    let Some(dyn_ph) = phs.iter().find(|p| p.p_type == PT_DYNAMIC).copied() else {
        return Ok(None);
    };

    let dyn_off = usize::try_from(dyn_ph.p_offset).map_err(|_| "elf: PT_DYNAMIC offset invalid")?;
    let dyn_sz = usize::try_from(dyn_ph.p_filesz).map_err(|_| "elf: PT_DYNAMIC size invalid")?;
    let dyn_end = dyn_off
        .checked_add(dyn_sz)
        .ok_or("elf: PT_DYNAMIC range overflow")?;
    if dyn_end > bytes.len() {
        return Err("elf: PT_DYNAMIC exceeds file");
    }

    let mut info = DynamicInfo {
        rela_addr: 0,
        rela_sz: 0,
        rela_ent: 0,
        rela_count: 0,
    };

    let mut at = dyn_off;
    while at + 16 <= dyn_end {
        let tag = read_i64_le(bytes, at).ok_or("elf: truncated dynamic")?;
        let val = read_u64_le(bytes, at + 8).ok_or("elf: truncated dynamic")?;
        at += 16;

        if tag == DT_NULL {
            break;
        }
        if tag == DT_RELA {
            info.rela_addr = val.saturating_add(base);
        } else if tag == DT_RELASZ {
            info.rela_sz = val;
        } else if tag == DT_RELAENT {
            info.rela_ent = val;
        } else if tag == DT_RELACOUNT {
            info.rela_count = val;
        }
    }

    if info.rela_addr == 0 || info.rela_sz == 0 {
        return Ok(None);
    }
    if info.rela_ent == 0 {
        info.rela_ent = 24;
    }
    Ok(Some(info))
}

fn apply_relocations(info: DynamicInfo, base: u64) -> Result<(), &'static str> {
    if info.rela_ent < 24 {
        return Err("elf: RELA entry size too small");
    }

    let total = if info.rela_count > 0 {
        info.rela_count
    } else {
        info.rela_sz / info.rela_ent
    };

    let mut idx = 0u64;
    while idx < total {
        let rel_addr = checked_add(
            info.rela_addr,
            idx.checked_mul(info.rela_ent).ok_or("elf: RELA overflow")?,
            "elf: RELA overflow",
        )?;
        let rel_ptr = rel_addr as *const u8;
        let r_offset = unsafe { core::ptr::read_unaligned(rel_ptr as *const u64) };
        let r_info = unsafe { core::ptr::read_unaligned(rel_ptr.add(8) as *const u64) };
        let r_addend = unsafe { core::ptr::read_unaligned(rel_ptr.add(16) as *const i64) };
        let r_type = (r_info & 0xFFFF_FFFF) as u32;

        if r_type == R_X86_64_RELATIVE {
            let target = checked_add(base, r_offset, "elf: relocation target overflow")?;
            let value = checked_add(base, r_addend as u64, "elf: relocation addend overflow")?;
            unsafe {
                core::ptr::write_unaligned(target as *mut u64, value);
            }
        }

        idx += 1;
    }

    Ok(())
}

fn map_and_load(
    bytes: &[u8],
    h: &ElfHeader,
    phs: &[ProgramHeader],
    base: u64,
    clear_existing_mappings: bool,
) -> Result<LoadedImage, &'static str> {
    let mut ranges: Vec<MapRange> = Vec::new();
    let mut mapped_starts = Vec::new();

    for ph in phs {
        if ph.p_type != PT_LOAD {
            continue;
        }
        elf_trace!(
            "elf: PT_LOAD vaddr=0x{:x} off=0x{:x} filesz=0x{:x} memsz=0x{:x} flags=0x{:x}",
            ph.p_vaddr,
            ph.p_offset,
            ph.p_filesz,
            ph.p_memsz,
            ph.p_flags
        );
    }

    for ph in phs {
        if ph.p_type != PT_LOAD || ph.p_memsz == 0 {
            continue;
        }
        let start = checked_add(base, ph.p_vaddr, "elf: segment start overflow")?;
        let end = checked_add(start, ph.p_memsz, "elf: segment end overflow")?;
        let map_start = align_down(start, vmm::PAGE_SIZE);
        let map_end = align_up(end, vmm::PAGE_SIZE);
        let range = MapRange {
            start: map_start,
            end: map_end,
            flags: segment_flags(ph.p_flags),
        };
        add_map_range(&mut ranges, range);
    }

    for r in &ranges {
        let size = r.end.saturating_sub(r.start);
        let pages =
            usize::try_from(size / vmm::PAGE_SIZE).map_err(|_| "elf: segment pages overflow")?;

        elf_trace!(
            "elf: map 0x{:x}-0x{:x} {}{}{}",
            r.start,
            r.end,
            if (r.flags & vmm::FLAG_READ) != 0 { "R" } else { "-" },
            if (r.flags & vmm::FLAG_WRITE) != 0 { "W" } else { "-" },
            if (r.flags & vmm::FLAG_EXEC) != 0 { "X" } else { "-" },
        );

        if clear_existing_mappings {
            // Remove stale tracked mappings from prior attempts. Do not force an
            // untracked teardown here: dropping a shared low-half huge mapping can
            // invalidate unrelated kernel source bytes used by the loader copy path.
            // `map_owned`/`map_page_hw` handles huge-PDE split at map time.
            let _ = vmm::unmap(r.start);
        }

        let phys = pmm::alloc_pages(pages).ok_or("elf: no physical memory for segment")?;
        let owner = format!("elf-seg@0x{:x}", r.start);
        let load_flags = r.flags | vmm::FLAG_WRITE;
        if let Err(e) = vmm::map_owned(r.start, phys, pages, load_flags, owner.as_str()) {
            let _ = pmm::free_pages_range(phys, pages);
            for s in &mapped_starts {
                let _ = vmm::unmap(*s);
            }
            if h.e_type == ET_EXEC && e == "vmm: page already mapped" {
                return Err(
                    "elf: ET_EXEC load address overlaps existing kernel mappings; rebuild binary as static PIE (ET_DYN)",
                );
            }
            return Err(e);
        }
        mapped_starts.push(r.start);

        let size_usize = usize::try_from(size).map_err(|_| "elf: segment size overflow")?;
        if !range_mapped_current(r.start, size_usize) {
            for s in &mapped_starts {
                let _ = vmm::unmap(*s);
            }
            return Err("elf: segment mapping incomplete before zero");
        }

        unsafe {
            core::ptr::write_bytes(r.start as *mut u8, 0, size_usize);
        }
    }

    for ph in phs {
        if ph.p_type != PT_LOAD || ph.p_filesz == 0 {
            continue;
        }

        elf_trace!(
            "elf: copy seg vaddr=0x{:x} off=0x{:x} filesz=0x{:x} memsz=0x{:x}",
            ph.p_vaddr,
            ph.p_offset,
            ph.p_filesz,
            ph.p_memsz
        );

        let src_off = usize::try_from(ph.p_offset).map_err(|_| "elf: file offset overflow")?;
        let src_len = usize::try_from(ph.p_filesz).map_err(|_| "elf: file size overflow")?;
        let src_end = src_off
            .checked_add(src_len)
            .ok_or("elf: file range overflow")?;
        if src_end > bytes.len() {
            for s in &mapped_starts {
                let _ = vmm::unmap(*s);
            }
            return Err("elf: PT_LOAD file range exceeds image");
        }

        let dst = checked_add(base, ph.p_vaddr, "elf: dst overflow")? as *mut u8;
        let src_ptr = bytes[src_off..src_end].as_ptr() as u64;
        let dst_ptr = dst as u64;
        if !range_mapped_current(dst_ptr, src_len) {
            for s in &mapped_starts {
                let _ = vmm::unmap(*s);
            }
            return Err("elf: destination segment mapping incomplete before copy");
        }
        if !range_mapped_current(src_ptr, src_len) {
            for s in &mapped_starts {
                let _ = vmm::unmap(*s);
            }
            return Err("elf: source segment mapping missing before copy");
        }
        let cr3 = hal::arch::paging::read_cr3();
        elf_trace!(
            "elf: copy detail src=0x{:x}-0x{:x} dst=0x{:x}-0x{:x} len=0x{:x} cr3=0x{:x}",
            src_ptr,
            src_ptr.saturating_add(src_len as u64),
            dst_ptr,
            dst_ptr.saturating_add(src_len as u64),
            src_len,
            cr3,
        );
        if ELF_TRACE_LOGS {
            log_mapping("seg-src-begin", src_ptr);
            log_mapping("seg-src-end", src_ptr.saturating_add(src_len as u64).saturating_sub(1));
            log_mapping("seg-dst-begin", dst_ptr);
            log_mapping("seg-dst-end", dst_ptr.saturating_add(src_len as u64).saturating_sub(1));
            log_mapping("seg-kernel-rsp", current_rsp());
        }
        unsafe {
            core::ptr::copy_nonoverlapping(bytes[src_off..src_end].as_ptr(), dst, src_len);
        }

        elf_trace!(
            "elf: copied seg vaddr=0x{:x} len=0x{:x}",
            ph.p_vaddr,
            ph.p_filesz
        );

        if ph.p_memsz > ph.p_filesz {
            let bss_off = usize::try_from(ph.p_filesz).map_err(|_| "elf: bss offset overflow")?;
            let bss_len =
                usize::try_from(ph.p_memsz - ph.p_filesz).map_err(|_| "elf: bss size overflow")?;
            if !range_mapped_current(dst_ptr.saturating_add(bss_off as u64), bss_len) {
                for s in &mapped_starts {
                    let _ = vmm::unmap(*s);
                }
                return Err("elf: destination bss mapping incomplete before zero");
            }
            unsafe {
                core::ptr::write_bytes(dst.add(bss_off), 0, bss_len);
            }
            elf_trace!(
                "elf: zeroed bss vaddr=0x{:x} len=0x{:x}",
                ph.p_vaddr.saturating_add(ph.p_filesz),
                ph.p_memsz.saturating_sub(ph.p_filesz)
            );
        }
    }

    let entry = runtime_entry(h, base);
    Ok(LoadedImage {
        entry,
        mapped_starts,
        mapped_ranges: ranges,
    })
}

fn finalize_segment_protections(img: &LoadedImage) -> Result<(), &'static str> {
    for range in &img.mapped_ranges {
        let size = range.end.saturating_sub(range.start);
        let pages = usize::try_from(size / vmm::PAGE_SIZE)
            .map_err(|_| "elf: segment pages overflow")?;
        vmm::reprotect(range.start, pages, range.flags)?;
    }
    Ok(())
}

fn unmap_loaded(img: &LoadedImage) {
    for start in &img.mapped_starts {
        let _ = vmm::unmap(*start);
    }
}

fn jump_to_entry(entry: u64) -> i32 {
    // SAFETY: caller ensures the entry address is a valid executable function
    // in the current address space. This is a direct in-kernel jump for
    // static ELF experiments and does not switch privilege level.
    let f: extern "sysv64" fn() -> i32 = unsafe { core::mem::transmute(entry as usize) };
    f()
}

fn jump_to_entry_with_rsp_recoverable(entry: u64, rsp: u64) -> bool {
    let returned = unsafe {
        let kernel_rsp0 = hal::arch::x86_64::seed_support::user_transition_kernel_rsp0();
        hal::arch::x86_64::tss::set_rsp0(kernel_rsp0);
        hal::arch::x86_64::syscall::set_kernel_rsp0(kernel_rsp0);
        hal::arch::x86_64::seed_support::enter_user_mode(entry, rsp)
    };
    if returned {
        crate::console::println!("[iretq] returned via fault-recovery path");
    }
    returned
}

fn stack_push_bytes(mut sp: u64, bytes: &[u8]) -> Result<u64, &'static str> {
    sp = sp.saturating_sub(bytes.len() as u64);
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), sp as *mut u8, bytes.len());
    }
    Ok(sp)
}

fn stack_push_u64(mut sp: u64, value: u64) -> Result<u64, &'static str> {
    sp = sp.saturating_sub(8);
    unsafe {
        core::ptr::write_unaligned(sp as *mut u64, value);
    }
    Ok(sp)
}

fn stack_align_down(sp: u64, align: u64) -> u64 {
    sp & !(align - 1)
}

fn auxv_phdr_addr(phs: &[ProgramHeader], h: &ElfHeader, base: u64) -> Option<u64> {
    let phoff = h.e_phoff;
    for ph in phs {
        if ph.p_type != PT_LOAD || ph.p_filesz == 0 {
            continue;
        }
        let seg_start = ph.p_offset;
        let seg_end = ph.p_offset.checked_add(ph.p_filesz)?;
        if phoff >= seg_start && phoff < seg_end {
            let delta = phoff.checked_sub(seg_start)?;
            return ph.p_vaddr.checked_add(base)?.checked_add(delta);
        }
    }
    None
}

fn map_initial_user_stack(
    path: &str,
    h: &ElfHeader,
    phs: &[ProgramHeader],
    entry: u64,
    base: u64,
    mapped_starts: &mut Vec<u64>,
) -> Result<u64, &'static str> {
    let stack_start = USER_STACK_BASE;
    let stack_size = (USER_STACK_PAGES as u64).saturating_mul(vmm::PAGE_SIZE);
    let stack_top = stack_start.saturating_add(stack_size);

    // Avoid untracked teardown of the low-half stack window here.
    // Demoting a shared 2 MiB huge mapping to clear 0x700000 can drop unrelated
    // low-half translations used by kernel-side loader state before user entry.
    // A direct map attempt gives us a clean overlap error without collateral loss.
    if vmm::inspect_mapping_current(stack_start).is_some() {
        return Err("elf: user stack virtual range already mapped");
    }

    let phys = pmm::alloc_pages(USER_STACK_PAGES).ok_or("elf: no physical memory for user stack")?;
    if let Err(e) = vmm::map_owned(
        stack_start,
        phys,
        USER_STACK_PAGES,
        vmm::FLAG_USER | vmm::FLAG_READ | vmm::FLAG_WRITE,
        "elf-user-stack",
    ) {
        let _ = pmm::free_pages_range(phys, USER_STACK_PAGES);
        return Err(e);
    }
    mapped_starts.push(stack_start);

    unsafe {
        core::ptr::write_bytes(stack_start as *mut u8, 0, stack_size as usize);
    }

    let mut sp = stack_top;

    let mut arg0 = Vec::new();
    arg0.extend_from_slice(path.as_bytes());
    arg0.push(0);
    sp = stack_push_bytes(sp, arg0.as_slice())?;
    let arg0_ptr = sp;

    sp = stack_align_down(sp, 16);

    // auxv: minimal Linux-compatible vector.
    sp = stack_push_u64(sp, arg0_ptr)?;
    sp = stack_push_u64(sp, AT_EXECFN)?;
    sp = stack_push_u64(sp, entry)?;
    sp = stack_push_u64(sp, AT_ENTRY)?;
    sp = stack_push_u64(sp, h.e_phnum as u64)?;
    sp = stack_push_u64(sp, AT_PHNUM)?;
    sp = stack_push_u64(sp, h.e_phentsize as u64)?;
    sp = stack_push_u64(sp, AT_PHENT)?;
    sp = stack_push_u64(
        sp,
        auxv_phdr_addr(phs, h, base).ok_or("elf: failed to compute AT_PHDR")?,
    )?;
    sp = stack_push_u64(sp, AT_PHDR)?;
    sp = stack_push_u64(sp, vmm::PAGE_SIZE)?;
    sp = stack_push_u64(sp, AT_PAGESZ)?;

    // auxv terminator (AT_NULL, 0)
    sp = stack_push_u64(sp, 0)?;
    sp = stack_push_u64(sp, AT_NULL)?;

    // envp terminator
    sp = stack_push_u64(sp, 0)?;

    // argv[1] = NULL, argv[0] = arg0
    sp = stack_push_u64(sp, 0)?;
    sp = stack_push_u64(sp, arg0_ptr)?;
    let argv_ptr = sp;

    // argc
    sp = stack_push_u64(sp, 1)?;

    elf_trace!(
        "elf: startup argc=1 argv=0x{:x} argv0=0x{:x}",
        argv_ptr,
        arg0_ptr
    );

    Ok(sp)
}

fn current_rsp() -> u64 {
    hal::arch::x86_64::cpu::read_rsp()
}

fn range_mapped_in_root(root_phys: u64, start: u64, len: usize) -> bool {
    if len == 0 {
        return true;
    }

    let page = vmm::PAGE_SIZE;
    let mut current = align_down(start, page);
    let end = start.saturating_add(len as u64);
    let limit = align_up(end, page);
    while current < limit {
        match vmm::is_mapped_in_page_tables(root_phys, current) {
            Ok(true) => {}
            Ok(false) | Err(_) => return false,
        }
        current = current.saturating_add(page);
    }
    true
}

fn range_mapped_current(start: u64, len: usize) -> bool {
    if len == 0 {
        return true;
    }

    let page = vmm::PAGE_SIZE;
    let mut current = align_down(start, page);
    let end = start.saturating_add(len as u64);
    let limit = align_up(end, page);
    while current < limit {
        if vmm::inspect_mapping_current(current).is_none() {
            return false;
        }
        current = current.saturating_add(page);
    }
    true
}

fn log_mapping(label: &str, virt: u64) {
    if let Some(info) = vmm::inspect_mapping_current(virt) {
        crate::console::println!(
            "elf: mapchk {} virt=0x{:x} phys=0x{:x} p={} w={} u={} nx={} g={} huge={}",
            label,
            virt,
            info.phys,
            info.present as u8,
            info.writable as u8,
            info.user as u8,
            info.nx as u8,
            info.global as u8,
            info.huge as u8,
        );
    } else {
        crate::console::println!("elf: mapchk {} virt=0x{:x} unmapped", label, virt);
    }
}

fn jump_to_entry_recoverable(entry: u64, pid: u64, initial_rsp: Option<u64>) -> Result<i32, &'static str> {
    crate::kernel::fault::begin_user_exec(pid);
    crate::console::println!(
        "elf: user-enter pid={} rip=0x{:x} rsp=0x{:x}",
        pid,
        entry,
        initial_rsp.unwrap_or_else(current_rsp)
    );
    elf_trace!(
        "elf: entry=0x{:x} rsp=0x{:x} cr3=0x{:x}",
        entry,
        initial_rsp.unwrap_or_else(current_rsp),
        hal::arch::paging::read_cr3()
    );
    if let Some(sp) = initial_rsp {
        let kernel_rsp0 = hal::arch::x86_64::seed_support::user_transition_kernel_rsp0();
        let gdt = hal::arch::x86_64::cpu::read_gdt_ptr();
        let idt = hal::arch::x86_64::cpu::read_idt_ptr();
        hal::arch::x86_64::tss::set_rsp0(kernel_rsp0);
        hal::arch::x86_64::syscall::set_kernel_rsp0(kernel_rsp0);
        if ELF_TRACE_LOGS {
            log_mapping("entry", entry);
            log_mapping("user-rsp", sp);
            log_mapping("rsp0", kernel_rsp0);
            log_mapping("gdt", gdt.base);
            log_mapping("idt", idt.base);
        }
        let fault_returned = jump_to_entry_with_rsp_recoverable(entry, sp);
        let faulted = crate::kernel::fault::take_active_exec_faulted();
        crate::kernel::fault::end_user_exec();
        crate::console::println!(
            "elf: user-return pid={} returned={} faulted={}",
            pid,
            fault_returned as u8,
            faulted as u8
        );
        if fault_returned && faulted {
            return Err("elf: user process fault");
        }
        if fault_returned {
            if let Some(rec) = crate::kernel::process::record(pid)
                && let Some(code) = rec.exit_code
            {
                return Ok(code);
            }
            return Err("elf: user process returned without exit status");
        }
        return Err("elf: user process returned unexpectedly");
    }

    let code = jump_to_entry(entry);
    let faulted = crate::kernel::fault::take_active_exec_faulted();
    crate::kernel::fault::end_user_exec();
    if faulted {
        return Err("elf: user process page fault");
    }
    Ok(code)
}

fn can_use_isolated_address_space() -> bool {
    crate::heap::dynamic_mappings_available()
}

pub fn load_and_run(path: &str, image_base: u64, pid: u64) -> Result<i32, &'static str> {
    let handle = saifs::open(path).map_err(|_| "elf: open failed")?;
    let bytes = handle.read().map_err(|_| "elf: read failed")?;
    let image_ptr = bytes.as_ptr() as u64;
    elf_trace!(
        "elf: image ptr=0x{:x} len=0x{:x} cr3=0x{:x}",
        image_ptr,
        bytes.len(),
        hal::arch::paging::read_cr3()
    );
    if ELF_TRACE_LOGS {
        log_mapping("image-begin", image_ptr);
        if !bytes.is_empty() {
            log_mapping("image-end", image_ptr.saturating_add(bytes.len() as u64).saturating_sub(1));
        }
    }

    let header = parse_header(bytes.as_slice())?;
    crate::console::println!(
        "elf: load path='{}' type={} mode={}",
        path,
        if header.e_type == ET_DYN { "ET_DYN" } else { "ET_EXEC" },
        if header.e_type == ET_DYN && ET_DYN_PROCESS_ADDRESS_SPACE {
            "isolated-preferred"
        } else if header.e_type == ET_EXEC && ET_EXEC_ISOLATED_ADDRESS_SPACE {
            "isolated-preferred"
        } else {
            "shared"
        }
    );
    let phs = parse_program_headers(bytes.as_slice(), &header)?;
    let et_exec_load_count = phs
        .iter()
        .filter(|ph| ph.p_type == PT_LOAD && ph.p_memsz > 0)
        .count();
    let et_exec_total_mem = phs
        .iter()
        .filter(|ph| ph.p_type == PT_LOAD)
        .fold(0u64, |acc, ph| acc.saturating_add(ph.p_memsz));
    let et_exec_isolated_policy_ok = header.e_type != ET_EXEC
        || (et_exec_load_count <= 1 && et_exec_total_mem <= 2 * 1024 * 1024);
    if phs.iter().any(|ph| ph.p_type == PT_INTERP && ph.p_filesz != 0) {
        return Err("elf: PT_INTERP executables are not supported yet");
    }
    let base = runtime_base(&header, image_base);
    let use_isolated_exec = header.e_type == ET_EXEC
        && ET_EXEC_ISOLATED_ADDRESS_SPACE
        && can_use_isolated_address_space()
        && et_exec_isolated_policy_ok;

    if header.e_type == ET_EXEC
        && ET_EXEC_ISOLATED_ADDRESS_SPACE
        && can_use_isolated_address_space()
        && !et_exec_isolated_policy_ok
    {
        crate::console::println!(
            "elf: ET_EXEC isolated policy declined (load_segments={} total_mem=0x{:x}); using shared bring-up path",
            et_exec_load_count,
            et_exec_total_mem
        );
    }

    if header.e_type == ET_EXEC && !use_isolated_exec {
        let (ks, ke) = vmm::kernel_image_range();
        if ks != 0 && ke > ks {
            for ph in &phs {
                if ph.p_type != PT_LOAD || ph.p_memsz == 0 {
                    continue;
                }
                let seg_start = ph.p_vaddr;
                let seg_end = ph.p_vaddr.saturating_add(ph.p_memsz);
                if seg_start < vmm::KERNEL_VIRT_BASE {
                    crate::console::println!(
                        "elf: ET_EXEC shared-path load 0x{:x}-0x{:x} is unsafe below high-half; refusing non-PIE image",
                        seg_start,
                        seg_end
                    );
                    return Err(
                        "elf: ET_EXEC shared-path load below high-half is unsafe; rebuild as PIE (ET_DYN) or use built-in shell",
                    );
                }
                if seg_start < ke && ks < seg_end {
                    crate::console::println!(
                        "elf: ET_EXEC segment 0x{:x}-0x{:x} overlaps live kernel image 0x{:x}-0x{:x}",
                        seg_start,
                        seg_end,
                        ks,
                        ke
                    );
                    return Err(
                        "elf: ET_EXEC image overlaps live kernel memory; rebuild as PIE (ET_DYN) or link above kernel image",
                    );
                }
            }
        }
    }

    let use_isolated_dyn = header.e_type == ET_DYN
        && ET_DYN_PROCESS_ADDRESS_SPACE
        && can_use_isolated_address_space();

    let mut exec_root = if use_isolated_exec || use_isolated_dyn {
        Some(vmm::clone_current_address_space_root()?)
    } else {
        None
    };

    if header.e_type == ET_EXEC {
        if let Some(root_phys) = exec_root {
            let stack_probe_start = align_down(current_rsp().saturating_sub(vmm::PAGE_SIZE * 16), vmm::PAGE_SIZE);
            let stack_probe_len = usize::try_from(vmm::PAGE_SIZE * 17).unwrap_or(0);
            let image_ok = range_mapped_in_root(root_phys, bytes.as_ptr() as u64, bytes.len());
            let stack_ok = range_mapped_in_root(root_phys, stack_probe_start, stack_probe_len);
            if !image_ok || !stack_ok {
                crate::console::println!(
                    "elf: ET_EXEC isolated root missing kernel source mappings (image={} stack={}); using shared bring-up path",
                    image_ok as u8,
                    stack_ok as u8
                );
                let _ = vmm::destroy_address_space_root(root_phys);
                exec_root = None;
            }
        }
    } else if header.e_type == ET_DYN {
        if let Some(root_phys) = exec_root {
            let stack_probe_start = align_down(current_rsp().saturating_sub(vmm::PAGE_SIZE * 16), vmm::PAGE_SIZE);
            let stack_probe_len = usize::try_from(vmm::PAGE_SIZE * 17).unwrap_or(0);
            let image_ok = range_mapped_in_root(root_phys, bytes.as_ptr() as u64, bytes.len());
            let stack_ok = range_mapped_in_root(root_phys, stack_probe_start, stack_probe_len);
            if !image_ok || !stack_ok {
                crate::console::println!(
                    "elf: ET_DYN isolated root missing kernel source mappings (image={} stack={}); using shared bring-up path",
                    image_ok as u8,
                    stack_ok as u8
                );
                let _ = vmm::destroy_address_space_root(root_phys);
                exec_root = None;
            }
        }
    }

    if let Some(exec_root) = exec_root {
        if header.e_type == ET_DYN {
            crate::console::println!(
                "elf: ET_DYN using cloned address-space root with low-half cleanup"
            );
        }

        let run_result = vmm::with_address_space(exec_root, || {
            elf_trace!(
                "elf: phase=map_load et={} isolated",
                if header.e_type == ET_DYN { "ET_DYN" } else { "ET_EXEC" }
            );
            let img = map_and_load(bytes.as_slice(), &header, phs.as_slice(), base, true)?;
            let mut img = img;

            let initial_rsp = match map_initial_user_stack(
                path,
                &header,
                phs.as_slice(),
                img.entry,
                base,
                &mut img.mapped_starts,
            ) {
                Ok(rsp) => Some(rsp),
                Err(e) => { unmap_loaded(&img); return Err(e); }
            };

            let dyn_opt = match parse_dynamic(bytes.as_slice(), phs.as_slice(), base) {
                Ok(d) => d,
                Err(e) => { unmap_loaded(&img); return Err(e); }
            };
            if let Some(dyn_info) = dyn_opt {
                elf_trace!(
                    "elf: phase=reloc rela=0x{:x} sz=0x{:x} ent=0x{:x} count={}",
                    dyn_info.rela_addr,
                    dyn_info.rela_sz,
                    dyn_info.rela_ent,
                    dyn_info.rela_count
                );
                if let Err(e) = apply_relocations(dyn_info, base) {
                    unmap_loaded(&img);
                    return Err(e);
                }
                elf_trace!("elf: phase=reloc done");
            }

            if let Err(e) = finalize_segment_protections(&img) {
                unmap_loaded(&img);
                return Err(e);
            }

            elf_trace!("elf: phase=jump");
            let result = jump_to_entry_recoverable(img.entry, pid, initial_rsp);
            unmap_loaded(&img);
            result
        })?;
        let _ = vmm::destroy_address_space_root(exec_root);
        run_result
    } else {
        if (header.e_type == ET_EXEC && ET_EXEC_ISOLATED_ADDRESS_SPACE
            || header.e_type == ET_DYN && ET_DYN_PROCESS_ADDRESS_SPACE)
            && !crate::heap::dynamic_mappings_available()
        {
            crate::console::println!(
                "elf: isolated address space deferred (kernel heap identity fallback active); using shared bring-up path"
            );
        }
        if header.e_type == ET_DYN && ET_DYN_PROCESS_ADDRESS_SPACE {
            elf_trace!(
                "elf: ET_DYN using shared bring-up path"
            );
        }
        elf_trace!("elf: phase=map_load");
        let img = map_and_load(bytes.as_slice(), &header, phs.as_slice(), base, false)?;
        let mut img = img;

        let initial_rsp = match map_initial_user_stack(
            path,
            &header,
            phs.as_slice(),
            img.entry,
            base,
            &mut img.mapped_starts,
        ) {
            Ok(rsp) => Some(rsp),
            Err(e) => { unmap_loaded(&img); return Err(e); }
        };

        let dyn_opt = match parse_dynamic(bytes.as_slice(), phs.as_slice(), base) {
            Ok(d) => d,
            Err(e) => { unmap_loaded(&img); return Err(e); }
        };
        if let Some(dyn_info) = dyn_opt {
            elf_trace!(
                "elf: phase=reloc rela=0x{:x} sz=0x{:x} ent=0x{:x} count={}",
                dyn_info.rela_addr,
                dyn_info.rela_sz,
                dyn_info.rela_ent,
                dyn_info.rela_count
            );
            if let Err(e) = apply_relocations(dyn_info, base) {
                unmap_loaded(&img);
                return Err(e);
            }
            elf_trace!("elf: phase=reloc done");
        }

        if let Err(e) = finalize_segment_protections(&img) {
            unmap_loaded(&img);
            return Err(e);
        }

        elf_trace!("elf: phase=jump");
        let result = jump_to_entry_recoverable(img.entry, pid, initial_rsp);
        unmap_loaded(&img);
        result
    }
}
