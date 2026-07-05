use alloc::format;
use alloc::vec::Vec;

use crate::pmm;
use crate::saifs;
use crate::saifs::Handle;
use crate::vmm;

const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const EV_CURRENT: u8 = 1;
const ET_EXEC: u16 = 2;
const ET_DYN: u16 = 3;
const EM_X86_64: u16 = 62;
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PF_X: u32 = 0x1;
const PF_W: u32 = 0x2;
const PF_R: u32 = 0x4;
const DT_NULL: i64 = 0;
const DT_RELA: i64 = 7;
const DT_RELASZ: i64 = 8;
const DT_RELAENT: i64 = 9;
const DT_RELACOUNT: i64 = 0x6ffffff9;
const R_X86_64_RELATIVE: u32 = 8;

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
) -> Result<LoadedImage, &'static str> {
    let mut ranges: Vec<MapRange> = Vec::new();
    let mut mapped_starts = Vec::new();

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
        let phys = pmm::alloc_pages(pages).ok_or("elf: no physical memory for segment")?;
        let owner = format!("elf-seg@0x{:x}", r.start);
        if let Err(e) = vmm::map_owned(r.start, phys, pages, r.flags, owner.as_str()) {
            let _ = pmm::free_pages_range(phys, pages);
            for s in &mapped_starts {
                let _ = vmm::unmap(*s);
            }
            return Err(e);
        }
        mapped_starts.push(r.start);

        unsafe {
            core::ptr::write_bytes(r.start as *mut u8, 0, usize::try_from(size).unwrap_or(0));
        }
    }

    for ph in phs {
        if ph.p_type != PT_LOAD || ph.p_filesz == 0 {
            continue;
        }

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
        unsafe {
            core::ptr::copy_nonoverlapping(bytes[src_off..src_end].as_ptr(), dst, src_len);
        }

        if ph.p_memsz > ph.p_filesz {
            let bss_off = usize::try_from(ph.p_filesz).map_err(|_| "elf: bss offset overflow")?;
            let bss_len =
                usize::try_from(ph.p_memsz - ph.p_filesz).map_err(|_| "elf: bss size overflow")?;
            unsafe {
                core::ptr::write_bytes(dst.add(bss_off), 0, bss_len);
            }
        }
    }

    let entry = runtime_entry(h, base);
    Ok(LoadedImage {
        entry,
        mapped_starts,
    })
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

pub fn load_and_run(path: &str, image_base: u64) -> Result<i32, &'static str> {
    let handle = saifs::open(path).map_err(|_| "elf: open failed")?;
    let bytes = handle.read().map_err(|_| "elf: read failed")?;

    let header = parse_header(bytes.as_slice())?;
    let phs = parse_program_headers(bytes.as_slice(), &header)?;
    let base = runtime_base(&header, image_base);

    let img = map_and_load(bytes.as_slice(), &header, phs.as_slice(), base)?;

    if let Some(dyn_info) = parse_dynamic(bytes.as_slice(), phs.as_slice(), base)? {
        if let Err(e) = apply_relocations(dyn_info, base) {
            unmap_loaded(&img);
            return Err(e);
        }
    }

    let code = jump_to_entry(img.entry);
    unmap_loaded(&img);
    Ok(code)
}
