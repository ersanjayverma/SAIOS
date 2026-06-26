//! ELF shared-object loader â€” maps PT_LOAD segments, builds symbol table,
//! applies relocations.

use super::{LOADED, SharedObject, load};
use crate::memory::{FRAME_ALLOCATOR, alloc_frames, paging};
/// Read a packed struct field safely without creating a reference.
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;

// ELF64 types
const PT_LOAD: u32 = 1;
const PT_DYNAMIC: u32 = 2;
const PT_INTERP: u32 = 3;

// Dynamic section tags
const DT_NULL: i64 = 0;
const DT_NEEDED: i64 = 1;
const DT_PLTRELSZ: i64 = 2;
const DT_STRTAB: i64 = 5;
const DT_SYMTAB: i64 = 6;
const DT_RELA: i64 = 7;
const DT_RELASZ: i64 = 8;
const DT_JMPREL: i64 = 23;
const DT_PLTREL: i64 = 20;
const DT_STRSZ: i64 = 10;
const DT_SYMENT: i64 = 11;
const DT_INIT: i64 = 12;
const DT_FINI: i64 = 13;
const DT_REL: i64 = 17;
const DT_RELSZ: i64 = 18;
const DT_SONAME: i64 = 14;

// Relocation types (x86_64)
const R_X86_64_64: u32 = 1;
const R_X86_64_GLOB_DAT: u32 = 6;
const R_X86_64_JUMP_SLOT: u32 = 7;
const R_X86_64_RELATIVE: u32 = 8;
const R_X86_64_DTPMOD64: u32 = 11;
const R_X86_64_DTPOFF64: u32 = 12;
const R_X86_64_TPOFF64: u32 = 13;

#[repr(C, packed)]
struct Elf64Hdr {
    e_ident: [u8; 16],
    e_type: u16,
    e_machine: u16,
    e_version: u32,
    e_entry: u64,
    e_phoff: u64,
    e_shoff: u64,
    e_flags: u32,
    e_ehsize: u16,
    e_phentsize: u16,
    e_phnum: u16,
    e_shentsize: u16,
    e_shnum: u16,
    e_shstrndx: u16,
}

#[repr(C, packed)]
struct Elf64Phdr {
    p_type: u32,
    p_flags: u32,
    p_offset: u64,
    p_vaddr: u64,
    p_paddr: u64,
    p_filesz: u64,
    p_memsz: u64,
    p_align: u64,
}

#[repr(C, packed)]
struct Elf64Dyn {
    d_tag: i64,
    d_val: u64,
}

#[repr(C, packed)]
struct Elf64Sym {
    st_name: u32,
    st_info: u8,
    st_other: u8,
    st_shndx: u16,
    st_value: u64,
    st_size: u64,
}

#[repr(C, packed)]
struct Elf64Rela {
    r_offset: u64,
    r_info: u64,
    r_addend: i64,
}

/// Load a shared object ELF into memory, build symbol table, apply relocations.
pub fn load_so(data: &[u8], name: &str) -> Result<SharedObject, &'static str> {
    if data.len() < 64 {
        return Err("dynlink: file too small");
    }
    let hdr = unsafe { &*(data.as_ptr() as *const Elf64Hdr) };

    if &hdr.e_ident[..4] != b"\x7FELF" {
        return Err("dynlink: bad ELF magic");
    }
    if hdr.e_ident[4] != 2 {
        return Err("dynlink: not 64-bit");
    }

    let phoff = unsafe { core::ptr::addr_of!(hdr.e_phoff).read_unaligned() } as usize;
    let phnum = unsafe { core::ptr::addr_of!(hdr.e_phnum).read_unaligned() } as usize;
    let phentsz = unsafe { core::ptr::addr_of!(hdr.e_phentsize).read_unaligned() } as usize;

    // Compute total virtual extent to pick a load address
    let (virt_min, virt_max) = virt_extent(data, phoff, phnum, phentsz);
    let load_size = (virt_max - virt_min) as usize;
    let pages = load_size.div_ceil(0x1000);

    // Allocate physical frames and map them
    let phys = alloc_frames(pages).ok_or("dynlink: OOM loading .so")?;
    // Pick virtual base = phys (identity mapped) or find a free VM region
    let base = phys; // identity map for kernel-loaded libs

    // Load PT_LOAD segments
    for i in 0..phnum {
        let off = phoff + i * phentsz;
        let ph = unsafe { &*(data[off..].as_ptr() as *const Elf64Phdr) };
        let p_type = unsafe { core::ptr::addr_of!(ph.p_type).read_unaligned() };
        let p_vaddr = unsafe { core::ptr::addr_of!(ph.p_vaddr).read_unaligned() };
        let p_filesz = unsafe { core::ptr::addr_of!(ph.p_filesz).read_unaligned() };
        let p_memsz = unsafe { core::ptr::addr_of!(ph.p_memsz).read_unaligned() };
        let p_offset = unsafe { core::ptr::addr_of!(ph.p_offset).read_unaligned() };

        if p_type != PT_LOAD {
            continue;
        }

        let voff = (p_vaddr - virt_min) as usize;
        let filesz = p_filesz as usize;
        let memsz = p_memsz as usize;
        let foff = p_offset as usize;

        if foff + filesz > data.len() {
            return Err("dynlink: segment out of range");
        }

        let dst = (base + voff as u64) as *mut u8;
        unsafe {
            core::ptr::copy_nonoverlapping(data[foff..foff + filesz].as_ptr(), dst, filesz);
            if memsz > filesz {
                core::ptr::write_bytes(dst.add(filesz), 0, memsz - filesz);
            }
        }
    }

    // Parse DYNAMIC segment to build symbol table and apply relocations
    let mut syms = BTreeMap::new();
    let mut symtab = 0u64;
    let mut strtab = 0u64;
    let mut strsz = 0usize;
    let mut rela = 0u64;
    let mut relasz = 0usize;
    let mut jmprel = 0u64;
    let mut pltrsz = 0usize;
    let mut needed = Vec::new();

    for i in 0..phnum {
        let off = phoff + i * phentsz;
        let ph = unsafe { core::ptr::read_unaligned(data[off..].as_ptr() as *const Elf64Phdr) };
        if ph.p_type != PT_DYNAMIC {
            continue;
        }

        let dyn_off = ph.p_offset as usize;
        let dyn_sz = ph.p_filesz as usize;
        let mut pos = dyn_off;

        while pos + 16 <= dyn_off + dyn_sz {
            let entry =
                unsafe { core::ptr::read_unaligned(data[pos..].as_ptr() as *const Elf64Dyn) };
            let tag = entry.d_tag;
            let val = entry.d_val;
            match tag {
                DT_NULL => break,
                DT_NEEDED => {
                    // Record string table offset for later name resolution
                    needed.push(val as usize);
                }
                DT_SYMTAB => symtab = base + (val - virt_min),
                DT_STRTAB => strtab = base + (val - virt_min),
                DT_STRSZ => strsz = val as usize,
                DT_RELA => rela = base + (val - virt_min),
                DT_RELASZ => relasz = val as usize,
                DT_JMPREL => jmprel = base + (val - virt_min),
                DT_PLTRELSZ => pltrsz = val as usize,
                _ => {}
            }
            pos += 16;
        }
    }

    // Load DT_NEEDED dependencies first
    for dep_offset in needed {
        let str_ptr = (strtab + dep_offset as u64) as *const u8;
        let dep_name = unsafe {
            let mut end = str_ptr;
            while *end != 0 && (end as usize - str_ptr as usize) < 256 {
                end = end.add(1);
            }
            let len = end as usize - str_ptr as usize;
            let mut buf = alloc::vec![0u8; len];
            core::ptr::copy_nonoverlapping(str_ptr, buf.as_mut_ptr(), len);
            String::from_utf8_lossy(&buf).into_owned()
        };
        if !dep_name.is_empty() {
            // Load the dependency
            let _ = load(&dep_name);
        }
    }

    // Build symbol table
    if symtab != 0 && strtab != 0 {
        let sym_size = core::mem::size_of::<Elf64Sym>();
        let mut sym_ptr = symtab as *const Elf64Sym;
        let strtab_ptr = strtab as *const u8;
        // Walk until we hit the strtab (rough heuristic â€” use dynsym size from section header ideally)
        for _ in 0..4096 {
            unsafe {
                let sym = &*sym_ptr;
                let name_off = sym.st_name as usize;
                if name_off < strsz {
                    let cstr = read_cstr(strtab_ptr.add(name_off), strsz - name_off);
                    if !cstr.is_empty() && sym.st_value != 0 {
                        let abs_addr = base + (sym.st_value - virt_min);
                        syms.insert(cstr, abs_addr);
                    }
                }
                sym_ptr = sym_ptr.add(1);
                // Stop when we've walked past strtab
                if sym_ptr as u64 >= strtab {
                    break;
                }
            }
        }
    }

    // Apply RELA relocations
    // For TLS relocations, we need the load_base as the module base
    apply_rela(rela, relasz, base, virt_min, &syms, Some(base));
    apply_rela(jmprel, pltrsz, base, virt_min, &syms, Some(base));

    Ok(SharedObject {
        name: String::from(name),
        base,
        entry: base + unsafe { core::ptr::addr_of!(hdr.e_entry).read_unaligned() },
        size: load_size,
        syms,
    })
}

fn apply_rela(
    rela_base: u64,
    rela_sz: usize,
    load_base: u64,
    virt_min: u64,
    syms: &BTreeMap<String, u64>,
    tls_base: Option<u64>,
) {
    if rela_base == 0 || rela_sz == 0 {
        return;
    }
    let count = rela_sz / 24;
    let ptr = rela_base as *const Elf64Rela;
    for i in 0..count {
        let rel = unsafe { core::ptr::read_unaligned(ptr.add(i)) };
        let offset = rel.r_offset;
        let info = rel.r_info;
        let addend = rel.r_addend;
        let rtype = (info & 0xFFFF_FFFF) as u32;
        let sym_idx = (info >> 32) as usize;

        let target = (load_base + (offset - virt_min)) as *mut u64;

        match rtype {
            R_X86_64_RELATIVE => unsafe {
                *target = load_base.wrapping_add_signed(addend);
            },
            R_X86_64_DTPMOD64 => {
                // Module index - for now just set to 1 (current module)
                // In a full implementation, this would track module IDs
                unsafe {
                    *target = 1;
                }
            }
            R_X86_64_DTPOFF64 => {
                // Offset within TLS block
                if let Some(tb) = tls_base {
                    unsafe {
                        *target = addend as u64;
                    }
                }
            }
            R_X86_64_TPOFF64 => {
                // TLS block offset to TP (Thread Pointer)
                // TPOFF = offset within TLS block + TLS block base
                if let Some(tb) = tls_base {
                    // The addend is the offset within the TLS block
                    // TPOFF64 gives the offset from TP (which is at TLS base)
                    unsafe {
                        *target = addend as u64;
                    }
                }
            }
            R_X86_64_GLOB_DAT | R_X86_64_JUMP_SLOT | R_X86_64_64 => {
                // Resolve the symbol - for now just write load_base + addend
                unsafe {
                    *target = load_base.wrapping_add_signed(addend);
                }
            }
            _ => {}
        }
    }
}

fn virt_extent(data: &[u8], phoff: usize, phnum: usize, phentsz: usize) -> (u64, u64) {
    let mut min = u64::MAX;
    let mut max = 0u64;
    for i in 0..phnum {
        let off = phoff + i * phentsz;
        if off + phentsz > data.len() {
            break;
        }
        let ph = unsafe { core::ptr::read_unaligned(data[off..].as_ptr() as *const Elf64Phdr) };
        if ph.p_type != PT_LOAD {
            continue;
        }
        let va = ph.p_vaddr;
        let end = va + ph.p_memsz;
        if va < min {
            min = va;
        }
        if end > max {
            max = end;
        }
    }
    if min == u64::MAX { (0, 0) } else { (min, max) }
}

unsafe fn read_cstr(ptr: *const u8, max: usize) -> String {
    unsafe {
        let mut v = alloc::vec![];
        for i in 0..max {
            let c = *ptr.add(i);
            if c == 0 {
                break;
            }
            v.push(c);
        }
        String::from_utf8_lossy(&v).into_owned()
    }
}
