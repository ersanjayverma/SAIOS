//! execve() — replace the current process image with a new ELF binary.
//!
//! Sets up the user stack according to the System V AMD64 ABI:
//!
//!   RSP →  argc
//!          argv[0] ptr
//!          argv[1] ptr
//!          ...
//!          NULL
//!          envp[0] ptr
//!          ...
//!          NULL
//!          AT_PHDR / AT_ENTRY / AT_PAGESZ / AT_NULL auxv pairs
//!          string data (argv + envp null-terminated strings)
//!          padding to 16-byte alignment

use alloc::string::String;
use alloc::vec::Vec;

use super::{Process, USER_STACK_TOP};
use crate::dynlink;
use crate::vfs;

const ELF_TYPE_DYN: u16 = 3;
const PT_LOAD: u32 = 1;

#[repr(C, packed)]
struct Elf64Header {
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

struct AuxvInfo {
    phdr: u64,
    phent: u64,
    phnum: u64,
    base: u64,
}

/// Execute a new program in the context of the current process.
/// Returns the entry point RIP and the new RSP on success.
pub fn do_exec(
    proc: &mut Process,
    elf_data: &[u8],
    argv: &[String],
    envp: &[String],
) -> Result<(u64, u64), &'static str> {
    if crate::windows::pe_loader::is_pe_file(elf_data) {
        crate::serial_println!("[execve] Detected PE Executable, passing to Windows compat layer");
        let pe_proc = crate::windows::pe_loader::load_pe(proc, elf_data)?;

        // Build a dummy user stack for Windows process
        const WINDOWS_STUB_USER_STACK: u64 = 0x8000_0000;
        let rsp = WINDOWS_STUB_USER_STACK;
        proc.rip = pe_proc.entry_point;
        proc.rsp = rsp;
        crate::vfs_contract::VfsContract::close_on_exec_for_process(proc);

        return Ok((pe_proc.entry_point, rsp));
    }

    proc.is_windows_process = false;
    proc.namespace_view = crate::vfs::namespace::NamespaceView::Linux;

    // Load ELF — replaces address space segments
    let entry = crate::process::elf::load(elf_data, proc)?;

    // Set up TLS if PT_TLS was present in the ELF
    if proc.tls_info.is_some() {
        dynlink::tls::setup_tls_for_process(proc);
    }

    let auxv_info = parse_auxv_info(
        elf_data,
        proc.interpreter.as_ref().map(|info| info.base).unwrap_or(0),
    )?;

    // Build user stack with argc / argv / envp / auxv
    let rsp = setup_user_stack(proc, proc.program_entry, &auxv_info, argv, envp)?;

    // Close FDs with O_CLOEXEC
    crate::vfs_contract::VfsContract::close_on_exec_for_process(proc);
    proc.rip = entry;
    proc.rsp = rsp;

    Ok((entry, rsp))
}

/// Read a null-terminated string array from user space.
/// Returns Vec<String> of the arguments.
pub fn read_user_argv(ptr: u64) -> Vec<String> {
    let mut result = Vec::new();
    if ptr == 0 {
        return result;
    }
    let mut p = ptr;
    loop {
        let str_ptr = unsafe { core::ptr::read_volatile(p as *const u64) };
        if str_ptr == 0 {
            break;
        }
        if let Some(s) = unsafe { read_user_cstr(str_ptr, 4096) } {
            result.push(s);
        }
        p += 8;
        if result.len() > 256 {
            break;
        }
    }
    result
}

unsafe fn read_user_cstr(ptr: u64, max: usize) -> Option<String> {
    unsafe {
        if ptr == 0 {
            return None;
        }
        let mut v = Vec::new();
        let mut p = ptr as *const u8;
        for _ in 0..max {
            let c = core::ptr::read_volatile(p);
            if c == 0 {
                break;
            }
            v.push(c);
            p = p.add(1);
        }
        String::from_utf8(v).ok()
    }
}

// -- AT_* auxiliary vector tags ---------------------------------------------
const AT_NULL: u64 = 0;
const AT_PHDR: u64 = 3;
const AT_PHENT: u64 = 4;
const AT_PHNUM: u64 = 5;
const AT_PAGESZ: u64 = 6;
const AT_BASE: u64 = 7;
const AT_FLAGS: u64 = 8;
const AT_ENTRY: u64 = 9;
const AT_UID: u64 = 11;
const AT_EUID: u64 = 12;
const AT_GID: u64 = 13;
const AT_EGID: u64 = 14;
const AT_SECURE: u64 = 23;
const AT_RANDOM: u64 = 25;
const AT_HWCAP: u64 = 16;

fn setup_user_stack(
    proc: &mut Process,
    entry: u64,
    auxv_info: &AuxvInfo,
    argv: &[String],
    envp: &[String],
) -> Result<u64, &'static str> {
    // We build the stack top-down in a temporary buffer, then copy to user stack.
    let mut string_data: Vec<u8> = Vec::with_capacity(4096 * 4);

    // 1. Push string data (argv + envp, null-terminated), record offsets
    let mut argv_offsets: Vec<usize> = Vec::new();
    let mut envp_offsets: Vec<usize> = Vec::new();

    for s in argv.iter().chain(envp.iter()) {
        let is_argv = argv_offsets.len() < argv.len();
        let off = string_data.len();
        string_data.extend_from_slice(s.as_bytes());
        string_data.push(0);
        if is_argv {
            argv_offsets.push(off);
        } else {
            envp_offsets.push(off);
        }
    }

    // 16-byte random bytes for AT_RANDOM
    let rand_off = string_data.len();
    string_data.extend_from_slice(&[
        0xDE, 0xAD, 0xBE, 0xEF, 0x13, 0x37, 0xC0, 0xDE, 0xFE, 0xED, 0xFA, 0xCE, 0xCA, 0xFE, 0xBA,
        0xBE,
    ]);

    while !string_data.len().is_multiple_of(8) {
        string_data.push(0);
    }

    let aux_pairs_without_random = [
        (AT_PHDR, auxv_info.phdr),
        (AT_PHENT, auxv_info.phent),
        (AT_PHNUM, auxv_info.phnum),
        (AT_PAGESZ, 4096),
        (AT_BASE, auxv_info.base),
        (AT_FLAGS, 0),
        (AT_ENTRY, entry),
        (AT_UID, 0),
        (AT_EUID, 0),
        (AT_GID, 0),
        (AT_EGID, 0),
        (AT_SECURE, 0),
        (AT_HWCAP, 0),
    ];

    let aux_count = aux_pairs_without_random.len() + 2;
    let pointer_slots = 1 + argv.len() + 1 + envp.len() + 1 + aux_count * 2;
    let pointer_bytes = pointer_slots * core::mem::size_of::<u64>();

    let unpadded_total = pointer_bytes + string_data.len();
    let tail_padding = (16 - (unpadded_total % 16)) % 16;
    let total = unpadded_total + tail_padding;
    let rsp = USER_STACK_TOP - total as u64;
    let string_base = rsp + pointer_bytes as u64;
    let to_ptr = |off: usize| string_base + off as u64;

    let mut ptrs: Vec<u64> = Vec::new();

    // argc
    ptrs.push(argv.len() as u64);

    // argv pointers
    for &off in &argv_offsets {
        ptrs.push(to_ptr(off));
    }
    ptrs.push(0); // NULL terminator

    // envp pointers
    for &off in &envp_offsets {
        ptrs.push(to_ptr(off));
    }
    ptrs.push(0); // NULL terminator

    // auxv
    let rand_ptr = to_ptr(rand_off);
    for &(tag, val) in &aux_pairs_without_random {
        ptrs.push(tag);
        ptrs.push(val);
    }
    for &(tag, val) in &[(AT_RANDOM, rand_ptr), (AT_NULL, 0)] {
        ptrs.push(tag);
        ptrs.push(val);
    }

    crate::serial_println!(
        "[exec-stack] entry={:#x} rsp={:#x} rsp_mod16={} argc={} envc={} str_base={:#x}",
        entry,
        rsp,
        rsp & 0xF,
        argv.len(),
        envp.len(),
        string_base
    );
    for (idx, arg) in argv.iter().take(4).enumerate() {
        crate::serial_println!(
            "[exec-stack] argv[{}] ptr={:#x} text='{}'",
            idx,
            to_ptr(argv_offsets[idx]),
            arg
        );
    }
    if !envp.is_empty() {
        crate::serial_println!(
            "[exec-stack] envp[0] ptr={:#x} text='{}'",
            to_ptr(envp_offsets[0]),
            envp[0]
        );
    }

    // Build final stack bytes: ptr section + string section
    let mut final_stack: Vec<u8> = Vec::new();
    for &p in &ptrs {
        final_stack.extend_from_slice(&p.to_le_bytes());
    }
    final_stack.extend_from_slice(&string_data);
    if tail_padding != 0 {
        final_stack.resize(final_stack.len() + tail_padding, 0);
    }

    // Write to the process address space.  During execve the process CR3 is
    // usually active, but shell-spawned processes are built before their CR3 is
    // loaded, so copying through the target PML4 is required.
    if rsp < 0x1000 {
        return Err("exec: stack underflow");
    }
    copy_to_user_stack(proc, rsp, &final_stack)?;

    proc.rsp = rsp;
    Ok(rsp)
}

fn copy_to_user_stack(proc: &Process, dst: u64, src: &[u8]) -> Result<(), &'static str> {
    let pml4 = if proc.address_space_pml4() != 0 {
        proc.address_space_pml4()
    } else {
        crate::memory::paging::active_pml4()
    };

    let mut copied = 0usize;
    while copied < src.len() {
        let virt = dst + copied as u64;
        let phys = crate::memory::paging::translate_in(pml4, virt)
            .ok_or("exec: stack destination unmapped")?;
        let page_remaining = 0x1000 - (virt as usize & 0xFFF);
        let chunk = core::cmp::min(page_remaining, src.len() - copied);
        unsafe {
            core::ptr::copy_nonoverlapping(src[copied..].as_ptr(), phys as *mut u8, chunk);
        }
        copied += chunk;
    }

    Ok(())
}

fn parse_auxv_info(elf_data: &[u8], base: u64) -> Result<AuxvInfo, &'static str> {
    if elf_data.len() < core::mem::size_of::<Elf64Header>() {
        return Err("exec: ELF too small for auxv metadata");
    }

    let hdr = unsafe { &*(elf_data.as_ptr() as *const Elf64Header) };
    let phoff = unsafe { core::ptr::addr_of!(hdr.e_phoff).read_unaligned() } as usize;
    let phentsz = unsafe { core::ptr::addr_of!(hdr.e_phentsize).read_unaligned() } as usize;
    let phnum = unsafe { core::ptr::addr_of!(hdr.e_phnum).read_unaligned() } as usize;
    let e_type = unsafe { core::ptr::addr_of!(hdr.e_type).read_unaligned() };
    let load_bias = if e_type == ELF_TYPE_DYN {
        crate::process::USER_TEXT_BASE
    } else {
        0
    };
    let phdr_size = phentsz
        .checked_mul(phnum)
        .ok_or("exec: phdr size overflow")?;

    let mut phdr_vaddr = 0u64;
    for idx in 0..phnum {
        let off = phoff + idx * phentsz;
        if off + core::mem::size_of::<Elf64Phdr>() > elf_data.len() {
            return Err("exec: program header out of bounds");
        }
        let ph = unsafe { &*(elf_data[off..].as_ptr() as *const Elf64Phdr) };
        let p_type = unsafe { core::ptr::addr_of!(ph.p_type).read_unaligned() };
        if p_type != PT_LOAD {
            continue;
        }

        let p_offset = unsafe { core::ptr::addr_of!(ph.p_offset).read_unaligned() } as usize;
        let p_vaddr = unsafe { core::ptr::addr_of!(ph.p_vaddr).read_unaligned() };
        let p_filesz = unsafe { core::ptr::addr_of!(ph.p_filesz).read_unaligned() } as usize;

        if phoff >= p_offset && phoff + phdr_size <= p_offset + p_filesz {
            phdr_vaddr = load_bias + p_vaddr + (phoff - p_offset) as u64;
            break;
        }
    }

    Ok(AuxvInfo {
        phdr: phdr_vaddr,
        phent: phentsz as u64,
        phnum: phnum as u64,
        base,
    })
}
