//! ELF64 loader — parses and loads an ELF binary into a process's address space.
//!
//! Supports both traditional executables (ET_EXEC) and position-independent
//! executables (ET_DYN / PIE). For PIE binaries, applies R_X86_64_RELATIVE
//! relocations from the DYNAMIC segment.
//!
//! # Address space layout (x86_64, 4-level paging)
//!
//! User space (canonical low half):
//!   0x0000_0000_0000_0000 .. 0x0000_7FFF_FFFF_FFFF  (128 TiB)
//!
//! SAIOS user region (PML4[1]):
//!   0x0000_0080_0000_0000 (512 GiB) = USER_TEXT_BASE
//!   0x0000_0080_0040_0000 (512 GiB + 4 MiB) = typical ET_EXEC text base
//!   0x0000_00FF_FFFF_F000 = USER_STACK_TOP
//!
//! Kernel space (canonical high half):
//!   0xFFFF_8000_0000_0000 .. 0xFFFF_FFFF_FFFF_FFFF
//!
//! The canonical hole (0x0000_8000_0000_0000 .. 0xFFFF_7FFF_FFFF_FFFF) is
//! **non-addressable** under 4-level paging. Any ET_EXEC linked into the hole
//! or above USER_TOP will be rejected with a diagnostic message.

use super::{InterpreterInfo, Process};
use crate::memory::{alloc_frames, paging};
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ELF64 magic
const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];
const ELF_CLASS64: u8 = 2;
const ELF_DATA_LE: u8 = 1;
const ELF_TYPE_EXEC: u16 = 2; // ET_EXEC - traditional executable
const ELF_TYPE_DYN: u16 = 3; // ET_DYN - shared object or PIE
const ELF_MACH_X86_64: u16 = 62;

// Program header types
const PT_LOAD: u32 = 1; // Loadable segment
const PT_DYNAMIC: u32 = 2; // Dynamic linking info
const PT_INTERP: u32 = 3; // Program interpreter path
const PT_TLS: u32 = 7; // Thread-local storage

// Segment flags
const PF_W: u32 = 2; // Writable
const PF_R: u32 = 4; // Readable
const PF_X: u32 = 1; // Executable

// Dynamic tags
const DT_NULL: i64 = 0; // End of dynamic array
const DT_STRTAB: i64 = 5;
const DT_SYMTAB: i64 = 6;
const DT_RELA: i64 = 7; // Rela relocation array
const DT_RELASZ: i64 = 8; // Size of Rela array
const DT_RELAENT: i64 = 9; // Size of Rela entry
const DT_STRSZ: i64 = 10;
const DT_SYMENT: i64 = 11;
const DT_JMPREL: i64 = 23;
const DT_PLTRELSZ: i64 = 2;

// Relocation types for x86_64
const R_X86_64_GLOB_DAT: u32 = 6;
const R_X86_64_JUMP_SLOT: u32 = 7;
const R_X86_64_RELATIVE: u32 = 8;

const USER_INTERP_BASE: u64 = 0x0000_00B0_0000_0000;
const INITIAL_USER_HEAP_PAGES: usize = 16;

struct DynamicInfo {
    strtab_vaddr: u64,
    symtab_vaddr: u64,
    strsz: usize,
    syment: usize,
    rela_vaddr: u64,
    relasz: usize,
    relaent: usize,
    jmprel_vaddr: u64,
    pltrelsz: usize,
}

#[derive(Default, Clone)]
struct DynamicSymbol {
    name: String,
    value: u64,
}

struct LoadedSharedObject {
    name: String,
    base: u64,
    entry: u64,
    size: usize,
    syms: BTreeMap<String, u64>,
}

#[repr(C, packed)]
struct Elf64Header {
    magic: [u8; 4],
    class: u8,
    data: u8,
    version: u8,
    os_abi: u8,
    _pad: [u8; 8],
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
struct Elf64Rela {
    r_offset: u64,
    r_info: u64,
    r_addend: i64,
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

/// Load an ELF64 executable into the process's virtual address space.
/// Returns the entry point virtual address.
///
/// Supports both ET_EXEC (fixed address) and ET_DYN/PIE (position-independent).
/// For PIE binaries, applies R_X86_64_RELATIVE relocations.
pub fn load(data: &[u8], proc: &mut Process) -> Result<u64, &'static str> {
    if data.len() < core::mem::size_of::<Elf64Header>() {
        return Err("ELF: file too small");
    }

    let hdr = unsafe { &*(data.as_ptr() as *const Elf64Header) };

    // Validate header
    if { hdr.magic } != ELF_MAGIC {
        return Err("ELF: bad magic");
    }
    if { hdr.class } != ELF_CLASS64 {
        return Err("ELF: not 64-bit");
    }
    if { hdr.data } != ELF_DATA_LE {
        return Err("ELF: not little-endian");
    }

    // Accept both ET_EXEC and ET_DYN (PIE)
    let e_type = { hdr.e_type };
    if e_type != ELF_TYPE_EXEC && e_type != ELF_TYPE_DYN {
        return Err("ELF: not an executable (got type {})");
    }
    if { hdr.e_machine } != ELF_MACH_X86_64 {
        return Err("ELF: not x86_64");
    }

    let phoff = { hdr.e_phoff } as usize;
    let phnum = { hdr.e_phnum } as usize;
    let phentsz = { hdr.e_phentsize } as usize;
    let interp_path = read_interp_path(data, phoff, phnum, phentsz)?;

    // Determine load bias. ET_DYN (PIE) binaries start at 0 and must be shifted
    // into the PML4[1] user window. ET_EXEC binaries use their linked addresses
    // directly (load_bias = 0), which must already be in the canonical user range.
    let load_bias = if e_type == ELF_TYPE_DYN {
        crate::process::USER_TEXT_BASE
    } else {
        0
    };

    let raw_entry = { hdr.e_entry };
    let entry = raw_entry + load_bias;
    proc.program_entry = entry;
    proc.interpreter = None;

    // Diagnostics: print e_type, e_entry, load_bias, and final entry before
    // any page-table work. A non-canonical entry is the #1 cause of #GP(0)
    // during iretq (the CPU validates RIP canonicality before the page walk).
    let type_name = if e_type == ELF_TYPE_EXEC {
        "ET_EXEC"
    } else {
        "ET_DYN"
    };
    crate::serial_println!(
        "[elf] type={} e_entry={:#x} load_bias={:#x} final_entry={:#x} canonical={}",
        type_name,
        raw_entry,
        load_bias,
        entry,
        is_canonical_4level(entry)
    );

    // Validate entry point
    if !is_canonical_4level(entry) {
        crate::serial_println!(
            "[elf] FATAL: entry point {:#x} is NON-CANONICAL for 4-level paging! \
            Valid user range: 0x0..{:#x}. PML4[1] base: {:#x}. \
            Is the binary linked at the wrong virtual base? \
            For PIE, compile with -fPIE -pie. For ET_EXEC, link below {:#x}.",
            entry,
            crate::process::USER_TOP,
            crate::process::USER_TEXT_BASE,
            crate::process::USER_TOP
        );
        return Err("ELF: entry point is non-canonical (bad link address?)");
    }
    if entry > crate::process::USER_TOP {
        crate::serial_println!(
            "[elf] FATAL: entry point {:#x} exceeds USER_TOP ({:#x})! \
            Binary is linked outside the user address range.",
            entry,
            crate::process::USER_TOP
        );
        return Err("ELF: entry point exceeds user address range");
    }
    if !is_canonical_4level(raw_entry) && load_bias == 0 {
        // ET_EXEC with a non-canonical e_entry — the binary itself is broken.
        // PIE binaries can have raw_entry near 0, which is canonical (low user space).
        crate::serial_println!(
            "[elf] FATAL: ET_EXEC e_entry {:#x} is non-canonical. \
            The binary was linked at an address that 4-level paging cannot reach. \
            Valid user range: 0x0..{:#x}. For PIE, compile with -fPIE -pie.",
            raw_entry,
            crate::process::USER_TOP
        );
        return Err("ELF: ET_EXEC entry point is non-canonical");
    }

    // Map into the process's private address space if it has one, else the
    // currently active space (kernel/boot identity map).
    let target_pml4 = if proc.address_space_pml4() != 0 {
        proc.address_space_pml4()
    } else {
        paging::active_pml4()
    };

    // First pass: map all PT_LOAD segments and detect PT_TLS
    for i in 0..phnum {
        let off = phoff + i * phentsz;
        if off + core::mem::size_of::<Elf64Phdr>() > data.len() {
            return Err("ELF: program header out of bounds");
        }
        let ph = unsafe { &*(data[off..].as_ptr() as *const Elf64Phdr) };
        let p_type = { ph.p_type };

        // Handle PT_TLS segment
        if p_type == PT_TLS {
            let vaddr = { ph.p_vaddr };
            let filesz = { ph.p_filesz };
            let memsz = { ph.p_memsz };
            let align = { ph.p_align };

            crate::serial_println!(
                "[tls] vaddr={:#x} filesz={:#x} memsz={:#x} align={:#x}",
                vaddr,
                filesz,
                memsz,
                align
            );

            // Store TLS information in the process
            proc.tls_info = Some(crate::process::TlsInfo::new(vaddr, filesz, memsz, align));
            continue;
        }

        if p_type != PT_LOAD {
            continue;
        }

        let raw_vaddr = { ph.p_vaddr };
        let vaddr = raw_vaddr + load_bias;
        let filesz = { ph.p_filesz } as usize;
        let memsz = { ph.p_memsz } as usize;
        let file_off = { ph.p_offset } as usize;
        let flags_val = { ph.p_flags };
        let writable = flags_val & PF_W != 0;
        let readable = flags_val & PF_R != 0;
        let _executable = flags_val & PF_X != 0;

        // Per-segment diagnostics
        let rwx = |f: u32| -> &'static str {
            match (f & PF_R != 0, f & PF_W != 0, f & PF_X != 0) {
                (true, true, true) => "RWX",
                (true, true, false) => "RW-",
                (true, false, true) => "R-X",
                (true, false, false) => "R--",
                (false, true, true) => "-WX",
                (false, true, false) => "-W-",
                (false, false, true) => "--X",
                _ => "---",
            }
        };

        // Validate canonicality of final vaddr
        if !is_canonical_4level(vaddr) {
            crate::serial_println!(
                "[elf] PT_LOAD[{}] vaddr={:#x} (raw={:#x} + bias={:#x}) NON-CANONICAL — skipping",
                i,
                vaddr,
                raw_vaddr,
                load_bias
            );
            continue;
        }
        // Validate that the segment stays within user address range
        if vaddr > crate::process::USER_TOP {
            crate::serial_println!(
                "[elf] PT_LOAD[{}] vaddr={:#x} exceeds USER_TOP ({:#x}) — skipping",
                i,
                vaddr,
                crate::process::USER_TOP
            );
            continue;
        }
        let seg_end = vaddr + memsz as u64;
        if seg_end > crate::process::USER_TOP + 1 {
            crate::serial_println!(
                "[elf] PT_LOAD[{}] end={:#x} exceeds USER_TOP ({:#x}) — segment truncated",
                i,
                seg_end,
                crate::process::USER_TOP
            );
            // Don't skip entirely — we can still map the portion within user range.
            // But for safety, reject the binary if any segment overflows.
            return Err("ELF: segment extends beyond user address range");
        }

        if memsz == 0 {
            continue;
        }
        if file_off + filesz > data.len() {
            return Err("ELF: segment data out of bounds");
        }

        // Allocate physical frames for this segment (page-aligned)
        let pages = page_count(vaddr, memsz);
        let phys = alloc_frames(pages).ok_or("ELF: OOM loading segment")?;
        let vbase = align_down(vaddr, 0x1000);

        // Map pages into user address space with proper permissions
        // For PIE binaries, always map as writable so relocations can be applied
        let executable = flags_val & PF_X != 0;
        let flags = paging::PTE_PRESENT
            | paging::PTE_USER
            | if writable || e_type == ELF_TYPE_DYN {
                paging::PTE_WRITABLE
            } else {
                0
            }
            | if !executable { paging::PTE_NO_EXEC } else { 0 };

        if crate::address_space_contract::AddressSpaceContract::map_user_frames_with_flags_in(
            crate::address_space_contract::AddressSpaceHandle {
                id: target_pml4,
                pml4: target_pml4,
                owner_pid: proc.pid,
            },
            vbase,
            phys,
            pages,
            flags,
        )
        .is_err()
        {
            crate::memory_contract::MemoryContract::free_frames(phys, pages, "elf_segment_failed");
            return Err("ELF: OOM loading segment (mapping failed)");
        }

        // Copy segment bytes from ELF file to physical memory
        // (phys is identity-mapped for the kernel → virt == phys)
        let dst_base = phys + (vaddr - vbase);
        let dst = unsafe { core::slice::from_raw_parts_mut(dst_base as *mut u8, memsz) };
        let src = &data[file_off..file_off + filesz];
        dst[..filesz].copy_from_slice(src);
        // Zero BSS (memsz > filesz)
        if memsz > filesz {
            dst[filesz..].fill(0);
        }

        if readable && !writable && !executable && filesz >= 16 {
            let src0 = u64::from_le_bytes(src[0..8].try_into().unwrap());
            let src1 = u64::from_le_bytes(src[8..16].try_into().unwrap());
            let dst0 = u64::from_le_bytes(dst[0..8].try_into().unwrap());
            let dst1 = u64::from_le_bytes(dst[8..16].try_into().unwrap());
            crate::serial_println!(
                "[elf] rodata head vaddr={:#x} src={:#018x} {:#018x} dst={:#018x} {:#018x}",
                vaddr,
                src0,
                src1,
                dst0,
                dst1
            );
        }

        // Show final mapping permissions
        let perm_str = if executable && writable {
            "RWX"
        } else if executable && !writable {
            "R-X"
        } else if !executable && writable {
            "RW-"
        } else {
            "R--"
        };

        crate::serial_println!(
            "[elf] PT_LOAD[{}] vaddr={:#x} {} → {} filesz={:#x} memsz={:#x} pages={}",
            i,
            vaddr,
            rwx(flags_val),
            perm_str,
            filesz,
            memsz,
            pages
        );
    }

    if let Some(path) = interp_path.as_ref() {
        let interp = load_interpreter_into_process(path, target_pml4)?;
        crate::dynlink::register_loaded(crate::dynlink::SharedObject {
            name: interp.name.clone(),
            base: interp.base,
            entry: interp.entry,
            size: interp.size,
            syms: interp.syms.clone(),
        });
        proc.interpreter = Some(InterpreterInfo::new(
            path.clone(),
            interp.base,
            interp.entry,
        ));
    }

    if let Some(dynamic_info) = parse_dynamic_info(data, phoff, phnum, phentsz)? {
        let local_symbols =
            build_dynamic_symbols(data, phoff, phnum, phentsz, &dynamic_info, load_bias)?;
        apply_dynamic_relocations(
            data,
            phoff,
            phnum,
            phentsz,
            target_pml4,
            load_bias,
            &dynamic_info,
            &local_symbols,
            proc.interpreter.is_some(),
        )?;
    }

    finalize_load_segment_permissions(data, phoff, phnum, phentsz, target_pml4, load_bias)?;

    map_initial_user_heap(target_pml4)?;
    proc.brk = crate::process::USER_BRK_BASE + (INITIAL_USER_HEAP_PAGES as u64 * 0x1000);

    Ok(proc
        .interpreter
        .as_ref()
        .map(|info| info.entry)
        .unwrap_or(entry))
}

fn map_initial_user_heap(target_pml4: u64) -> Result<(), &'static str> {
    let pages = INITIAL_USER_HEAP_PAGES;
    let phys = alloc_frames(pages).ok_or("ELF: OOM mapping initial heap")?;
    let base = crate::process::USER_BRK_BASE;

    if crate::address_space_contract::AddressSpaceContract::map_user_frames_in(
        crate::address_space_contract::AddressSpaceHandle {
            id: target_pml4,
            pml4: target_pml4,
            owner_pid: crate::process::current_pid().unwrap_or(1),
        },
        base,
        phys,
        pages,
    )
    .is_err()
    {
        crate::memory_contract::MemoryContract::free_frames(phys, pages, "elf_heap_failed");
        return Err("ELF: OOM mapping initial heap (mapping failed)");
    }

    unsafe {
        core::ptr::write_bytes(phys as *mut u8, 0, pages * 0x1000);
    }
    Ok(())
}

fn final_segment_flags(ph_flags: u32) -> u64 {
    let executable = ph_flags & PF_X != 0;
    let writable = ph_flags & PF_W != 0;
    paging::PTE_PRESENT
        | paging::PTE_USER
        | if writable { paging::PTE_WRITABLE } else { 0 }
        | if !executable { paging::PTE_NO_EXEC } else { 0 }
}

fn finalize_load_segment_permissions(
    data: &[u8],
    phoff: usize,
    phnum: usize,
    phentsz: usize,
    target_pml4: u64,
    load_bias: u64,
) -> Result<(), &'static str> {
    for i in 0..phnum {
        let off = phoff + i * phentsz;
        if off + core::mem::size_of::<Elf64Phdr>() > data.len() {
            continue;
        }
        let ph = unsafe { &*(data[off..].as_ptr() as *const Elf64Phdr) };
        if unsafe { core::ptr::addr_of!(ph.p_type).read_unaligned() } != PT_LOAD {
            continue;
        }

        let raw_vaddr = unsafe { core::ptr::addr_of!(ph.p_vaddr).read_unaligned() };
        let memsz = unsafe { core::ptr::addr_of!(ph.p_memsz).read_unaligned() } as usize;
        if memsz == 0 {
            continue;
        }

        let flags =
            final_segment_flags(unsafe { core::ptr::addr_of!(ph.p_flags).read_unaligned() });
        let vbase = align_down(raw_vaddr + load_bias, 0x1000);
        let pages = page_count(raw_vaddr + load_bias, memsz);
        for j in 0..pages {
            paging::update_page_flags_in(target_pml4, vbase + (j * 0x1000) as u64, flags)?;
        }
    }
    Ok(())
}

fn read_interp_path(
    data: &[u8],
    phoff: usize,
    phnum: usize,
    phentsz: usize,
) -> Result<Option<String>, &'static str> {
    for i in 0..phnum {
        let off = phoff + i * phentsz;
        if off + core::mem::size_of::<Elf64Phdr>() > data.len() {
            break;
        }
        let ph = unsafe { &*(data[off..].as_ptr() as *const Elf64Phdr) };
        if unsafe { core::ptr::addr_of!(ph.p_type).read_unaligned() } != PT_INTERP {
            continue;
        }
        let p_offset = unsafe { core::ptr::addr_of!(ph.p_offset).read_unaligned() } as usize;
        let p_filesz = unsafe { core::ptr::addr_of!(ph.p_filesz).read_unaligned() } as usize;
        if p_offset + p_filesz > data.len() || p_filesz == 0 {
            return Err("ELF: PT_INTERP out of bounds");
        }
        let bytes = &data[p_offset..p_offset + p_filesz];
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        let path =
            core::str::from_utf8(&bytes[..end]).map_err(|_| "ELF: PT_INTERP is not UTF-8")?;
        return Ok(Some(path.to_string()));
    }
    Ok(None)
}

fn parse_dynamic_info(
    data: &[u8],
    phoff: usize,
    phnum: usize,
    phentsz: usize,
) -> Result<Option<DynamicInfo>, &'static str> {
    let mut dyn_off: Option<usize> = None;
    let mut dyn_size = 0usize;

    for i in 0..phnum {
        let off = phoff + i * phentsz;
        if off + core::mem::size_of::<Elf64Phdr>() > data.len() {
            continue;
        }
        let ph = unsafe { &*(data[off..].as_ptr() as *const Elf64Phdr) };
        if { ph.p_type } == PT_DYNAMIC {
            dyn_off = Some(unsafe { core::ptr::addr_of!(ph.p_offset).read_unaligned() } as usize);
            dyn_size = unsafe { core::ptr::addr_of!(ph.p_filesz).read_unaligned() } as usize;
            break;
        }
    }

    let dyn_off = match dyn_off {
        Some(v) => v,
        None => return Ok(None),
    };
    if dyn_off + dyn_size > data.len() {
        return Err("ELF: PT_DYNAMIC out of bounds");
    }

    let mut info = DynamicInfo {
        strtab_vaddr: 0,
        symtab_vaddr: 0,
        strsz: 0,
        syment: core::mem::size_of::<Elf64Rela>(),
        rela_vaddr: 0,
        relasz: 0,
        relaent: core::mem::size_of::<Elf64Rela>(),
        jmprel_vaddr: 0,
        pltrelsz: 0,
    };

    let dyn_ptr = unsafe { data.as_ptr().add(dyn_off) as *const Elf64Dyn };
    for i in 0..(dyn_size / core::mem::size_of::<Elf64Dyn>()) {
        let dyn_entry = unsafe { &*dyn_ptr.add(i) };
        let tag = { dyn_entry.d_tag };
        let val = { dyn_entry.d_val };

        match tag {
            DT_NULL => break,
            DT_STRTAB => info.strtab_vaddr = val,
            DT_SYMTAB => info.symtab_vaddr = val,
            DT_STRSZ => info.strsz = val as usize,
            DT_SYMENT => info.syment = val as usize,
            DT_RELA => info.rela_vaddr = val,
            DT_RELASZ => info.relasz = val as usize,
            DT_RELAENT => info.relaent = val as usize,
            DT_JMPREL => info.jmprel_vaddr = val,
            DT_PLTRELSZ => info.pltrelsz = val as usize,
            _ => {}
        }
    }

    Ok(Some(info))
}

fn build_dynamic_symbols(
    data: &[u8],
    phoff: usize,
    phnum: usize,
    phentsz: usize,
    info: &DynamicInfo,
    load_bias: u64,
) -> Result<Vec<DynamicSymbol>, &'static str> {
    if info.symtab_vaddr == 0 || info.strtab_vaddr == 0 || info.syment == 0 {
        return Ok(Vec::new());
    }

    let mut max_index = 0usize;
    max_index = max_index.max(max_symbol_index_in_rela(
        data,
        phoff,
        phnum,
        phentsz,
        info.rela_vaddr,
        info.relasz,
        info.relaent,
    )?);
    max_index = max_index.max(max_symbol_index_in_rela(
        data,
        phoff,
        phnum,
        phentsz,
        info.jmprel_vaddr,
        info.pltrelsz,
        info.relaent,
    )?);

    let symtab_off = find_file_offset_for_vaddr(data, phoff, phnum, phentsz, info.symtab_vaddr)?;
    let strtab_off = find_file_offset_for_vaddr(data, phoff, phnum, phentsz, info.strtab_vaddr)?;
    if strtab_off + info.strsz > data.len() {
        return Err("ELF: dynamic string table out of bounds");
    }
    let strtab = &data[strtab_off..strtab_off + info.strsz];

    let mut syms = Vec::new();
    syms.resize_with(max_index.saturating_add(1), DynamicSymbol::default);
    for (idx, sym_slot) in syms.iter_mut().enumerate().take(max_index + 1) {
        let off = symtab_off + idx * info.syment;
        if off + core::mem::size_of::<Elf64Sym>() > data.len() {
            break;
        }
        let sym = unsafe { &*(data[off..].as_ptr() as *const Elf64Sym) };
        let name_off = unsafe { core::ptr::addr_of!(sym.st_name).read_unaligned() } as usize;
        let value = unsafe { core::ptr::addr_of!(sym.st_value).read_unaligned() } + load_bias;
        let name = read_dynstr(strtab, name_off)?;
        *sym_slot = DynamicSymbol { name, value };
    }
    Ok(syms)
}

#[allow(clippy::too_many_arguments)]
fn apply_dynamic_relocations(
    data: &[u8],
    phoff: usize,
    phnum: usize,
    phentsz: usize,
    target_pml4: u64,
    load_bias: u64,
    info: &DynamicInfo,
    local_symbols: &[DynamicSymbol],
    allow_unresolved: bool,
) -> Result<(), &'static str> {
    apply_rela_table(
        data,
        phoff,
        phnum,
        phentsz,
        target_pml4,
        load_bias,
        info.rela_vaddr,
        info.relasz,
        info.relaent,
        local_symbols,
        allow_unresolved,
    )?;
    apply_rela_table(
        data,
        phoff,
        phnum,
        phentsz,
        target_pml4,
        load_bias,
        info.jmprel_vaddr,
        info.pltrelsz,
        info.relaent,
        local_symbols,
        allow_unresolved,
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_rela_table(
    data: &[u8],
    phoff: usize,
    phnum: usize,
    phentsz: usize,
    target_pml4: u64,
    load_bias: u64,
    rela_vaddr: u64,
    rela_size: usize,
    rela_ent: usize,
    local_symbols: &[DynamicSymbol],
    allow_unresolved: bool,
) -> Result<(), &'static str> {
    if rela_vaddr == 0 || rela_size == 0 || rela_ent == 0 {
        return Ok(());
    }
    let rela_file_off = find_file_offset_for_vaddr(data, phoff, phnum, phentsz, rela_vaddr)?;
    let rela_count = rela_size / rela_ent;

    for i in 0..rela_count {
        let rela_ptr =
            unsafe { data.as_ptr().add(rela_file_off + i * rela_ent) as *const Elf64Rela };
        let rela = unsafe { &*rela_ptr };
        let r_type = ({ rela.r_info } & 0xFFFFFFFF) as u32;
        let sym_idx = ({ rela.r_info } >> 32) as usize;
        let r_offset = { rela.r_offset };
        let r_addend = { rela.r_addend };

        let target_vaddr = r_offset + load_bias;
        let target_phys = crate::memory::paging::translate_in(target_pml4, target_vaddr)
            .ok_or("ELF: relocation target not mapped")?;
        let target_ptr = target_phys as *mut u64;

        let value = match r_type {
            R_X86_64_RELATIVE => (load_bias as i64 + r_addend) as u64,
            R_X86_64_GLOB_DAT | R_X86_64_JUMP_SLOT => {
                match resolve_symbol_value(sym_idx, r_addend, local_symbols) {
                    Ok(v) => v,
                    Err(_) if allow_unresolved => {
                        crate::serial_println!(
                            "[dynlink] deferred reloc type={} sym_idx={} at {:#x}",
                            r_type,
                            sym_idx,
                            target_vaddr
                        );
                        continue;
                    }
                    Err(e) => return Err(e),
                }
            }
            _ => {
                if allow_unresolved {
                    crate::serial_println!(
                        "[dynlink] unsupported reloc type={} at {:#x}",
                        r_type,
                        target_vaddr
                    );
                    continue;
                }
                return Err("ELF: unsupported dynamic relocation type");
            }
        };

        unsafe {
            *target_ptr = value;
        }
    }

    Ok(())
}

fn resolve_symbol_value(
    sym_idx: usize,
    addend: i64,
    local_symbols: &[DynamicSymbol],
) -> Result<u64, &'static str> {
    let sym = local_symbols
        .get(sym_idx)
        .ok_or("ELF: relocation symbol index out of range")?;
    if sym.value != 0 {
        return Ok((sym.value as i64 + addend) as u64);
    }
    if !sym.name.is_empty()
        && let Some(addr) = crate::dynlink::resolve::lookup(&sym.name)
    {
        return Ok((addr as i64 + addend) as u64);
    }
    Err("ELF: unresolved dynamic symbol")
}

fn load_interpreter_into_process(
    path: &str,
    target_pml4: u64,
) -> Result<LoadedSharedObject, &'static str> {
    let data = crate::vfs_contract::VfsContract::read_file(path)
        .or_else(|_| {
            let name = path.rsplit('/').next().unwrap_or(path);
            crate::vfs_contract::VfsContract::read_file(
                &crate::dynlink::dlopen(name, 0).to_string(),
            )
        })
        .map_err(|_| "ELF: interpreter not found")?;
    load_shared_object_into_process(
        &data,
        path.rsplit('/').next().unwrap_or(path),
        target_pml4,
        USER_INTERP_BASE,
    )
}

fn load_shared_object_into_process(
    data: &[u8],
    name: &str,
    target_pml4: u64,
    load_bias: u64,
) -> Result<LoadedSharedObject, &'static str> {
    if data.len() < core::mem::size_of::<Elf64Header>() {
        return Err("ELF: shared object too small");
    }
    let hdr = unsafe { &*(data.as_ptr() as *const Elf64Header) };
    let phoff = unsafe { core::ptr::addr_of!(hdr.e_phoff).read_unaligned() } as usize;
    let phnum = unsafe { core::ptr::addr_of!(hdr.e_phnum).read_unaligned() } as usize;
    let phentsz = unsafe { core::ptr::addr_of!(hdr.e_phentsize).read_unaligned() } as usize;
    let entry = unsafe { core::ptr::addr_of!(hdr.e_entry).read_unaligned() } + load_bias;

    for i in 0..phnum {
        let off = phoff + i * phentsz;
        if off + core::mem::size_of::<Elf64Phdr>() > data.len() {
            return Err("ELF: interpreter phdr out of bounds");
        }
        let ph = unsafe { &*(data[off..].as_ptr() as *const Elf64Phdr) };
        let p_type = unsafe { core::ptr::addr_of!(ph.p_type).read_unaligned() };
        if p_type != PT_LOAD {
            continue;
        }

        let raw_vaddr = unsafe { core::ptr::addr_of!(ph.p_vaddr).read_unaligned() };
        let vaddr = raw_vaddr + load_bias;
        let filesz = unsafe { core::ptr::addr_of!(ph.p_filesz).read_unaligned() } as usize;
        let memsz = unsafe { core::ptr::addr_of!(ph.p_memsz).read_unaligned() } as usize;
        let file_off = unsafe { core::ptr::addr_of!(ph.p_offset).read_unaligned() } as usize;
        let flags_val = unsafe { core::ptr::addr_of!(ph.p_flags).read_unaligned() };

        if memsz == 0 {
            continue;
        }
        if file_off + filesz > data.len() {
            return Err("ELF: interpreter segment out of bounds");
        }

        let pages = page_count(vaddr, memsz);
        let phys = alloc_frames(pages).ok_or("ELF: OOM loading interpreter")?;
        let vbase = align_down(vaddr, 0x1000);
        let executable = flags_val & PF_X != 0;
        let writable = flags_val & PF_W != 0;
        let flags = paging::PTE_PRESENT
            | paging::PTE_USER
            | if writable
                || unsafe { core::ptr::addr_of!(hdr.e_type).read_unaligned() } == ELF_TYPE_DYN
            {
                paging::PTE_WRITABLE
            } else {
                0
            }
            | if !executable { paging::PTE_NO_EXEC } else { 0 };

        if crate::address_space_contract::AddressSpaceContract::map_user_frames_with_flags_in(
            crate::address_space_contract::AddressSpaceHandle {
                id: target_pml4,
                pml4: target_pml4,
                owner_pid: crate::process::current_pid().unwrap_or(1),
            },
            vbase,
            phys,
            pages,
            flags,
        )
        .is_err()
        {
            crate::memory_contract::MemoryContract::free_frames(
                phys,
                pages,
                "elf_interpreter_failed",
            );
            return Err("ELF: OOM loading interpreter (mapping failed)");
        }

        let dst_base = phys + (vaddr - vbase);
        let dst = unsafe { core::slice::from_raw_parts_mut(dst_base as *mut u8, memsz) };
        let src = &data[file_off..file_off + filesz];
        dst[..filesz].copy_from_slice(src);
        if memsz > filesz {
            dst[filesz..].fill(0);
        }
    }

    let info = parse_dynamic_info(data, phoff, phnum, phentsz)?
        .ok_or("ELF: interpreter missing PT_DYNAMIC")?;
    let local_symbols = build_dynamic_symbols(data, phoff, phnum, phentsz, &info, load_bias)?;
    apply_dynamic_relocations(
        data,
        phoff,
        phnum,
        phentsz,
        target_pml4,
        load_bias,
        &info,
        &local_symbols,
        false,
    )?;
    finalize_load_segment_permissions(data, phoff, phnum, phentsz, target_pml4, load_bias)?;

    let mut syms = BTreeMap::new();
    for sym in local_symbols {
        if !sym.name.is_empty() && sym.value != 0 {
            syms.insert(sym.name, sym.value);
        }
    }

    Ok(LoadedSharedObject {
        name: name.to_string(),
        base: load_bias,
        entry,
        size: 0,
        syms,
    })
}

fn max_symbol_index_in_rela(
    data: &[u8],
    phoff: usize,
    phnum: usize,
    phentsz: usize,
    rela_vaddr: u64,
    rela_size: usize,
    rela_ent: usize,
) -> Result<usize, &'static str> {
    if rela_vaddr == 0 || rela_size == 0 || rela_ent == 0 {
        return Ok(0);
    }
    let rela_file_off = find_file_offset_for_vaddr(data, phoff, phnum, phentsz, rela_vaddr)?;
    let count = rela_size / rela_ent;
    let mut max_idx = 0usize;
    for i in 0..count {
        let rela_ptr =
            unsafe { data.as_ptr().add(rela_file_off + i * rela_ent) as *const Elf64Rela };
        let rela = unsafe { &*rela_ptr };
        max_idx = max_idx.max(({ rela.r_info } >> 32) as usize);
    }
    Ok(max_idx)
}

fn read_dynstr(strtab: &[u8], offset: usize) -> Result<String, &'static str> {
    if offset >= strtab.len() {
        return Ok(String::new());
    }
    let bytes = &strtab[offset..];
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let s = core::str::from_utf8(&bytes[..end]).map_err(|_| "ELF: invalid dynstr")?;
    Ok(s.to_string())
}

/// Find the file offset corresponding to a virtual address by scanning PT_LOAD segments.
fn find_file_offset_for_vaddr(
    data: &[u8],
    phoff: usize,
    phnum: usize,
    phentsz: usize,
    vaddr: u64,
) -> Result<usize, &'static str> {
    for i in 0..phnum {
        let off = phoff + i * phentsz;
        if off + core::mem::size_of::<Elf64Phdr>() > data.len() {
            continue;
        }
        let ph = unsafe { &*(data[off..].as_ptr() as *const Elf64Phdr) };
        if { ph.p_type } != PT_LOAD {
            continue;
        }

        let seg_vaddr = { ph.p_vaddr };
        let seg_filesz = { ph.p_filesz };
        let seg_offset = { ph.p_offset };

        if vaddr >= seg_vaddr && vaddr < seg_vaddr + seg_filesz {
            return Ok((seg_offset + (vaddr - seg_vaddr)) as usize);
        }
    }
    Err("ELF: vaddr not found in any PT_LOAD segment")
}

fn page_count(vaddr: u64, memsz: usize) -> usize {
    let end = align_up(vaddr + memsz as u64, 0x1000);
    let start = align_down(vaddr, 0x1000);
    ((end - start) / 0x1000) as usize
}

fn align_down(addr: u64, align: u64) -> u64 {
    addr & !(align - 1)
}
fn align_up(addr: u64, align: u64) -> u64 {
    (addr + align - 1) & !(align - 1)
}

/// Four-level x86_64 canonical-address check. Bits 63:48 must all equal bit 47.
/// Non-canonical addresses cause #GP(0) during iretq before the page walk begins.
fn is_canonical_4level(addr: u64) -> bool {
    let sign = (addr >> 47) & 1;
    let high = addr >> 48;
    if sign == 0 { high == 0 } else { high == 0xFFFF }
}
