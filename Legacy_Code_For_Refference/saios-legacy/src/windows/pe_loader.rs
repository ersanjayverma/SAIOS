//! PE/COFF loader scaffolding for experimental Windows-process detection.
//!
//! Current scope is intentionally narrow: header detection plus placeholder
//! process-state setup. This is not a complete Windows loader.

use crate::process::Process;
use alloc::vec::Vec;

#[repr(C)]
pub struct DosHeader {
    pub e_magic: u16,
    pub e_cblp: u16,
    pub e_cp: u16,
    pub e_crlc: u16,
    pub e_cparhdr: u16,
    pub e_minalloc: u16,
    pub e_maxalloc: u16,
    pub e_ss: u16,
    pub e_sp: u16,
    pub e_csum: u16,
    pub e_ip: u16,
    pub e_cs: u16,
    pub e_lfarlc: u16,
    pub e_ovno: u16,
    pub e_res: [u16; 4],
    pub e_oemid: u16,
    pub e_oeminfo: u16,
    pub e_res2: [u16; 10],
    pub e_lfanew: i32,
}

pub fn is_pe_file(data: &[u8]) -> bool {
    if data.len() < core::mem::size_of::<DosHeader>() {
        return false;
    }
    let dos_header_ptr = data.as_ptr() as *const DosHeader;
    if !dos_header_ptr.is_aligned() {
        return false;
    }
    let dos_header = unsafe { &*dos_header_ptr };
    if dos_header.e_magic != 0x5A4D {
        // 'MZ'
        return false;
    }
    let pe_offset = dos_header.e_lfanew as usize;
    if data.len() < pe_offset + 4 {
        return false;
    }
    let signature = &data[pe_offset..pe_offset + 4];
    signature == b"PE\0\0"
}

pub struct PeProcess {
    pub base_addr: u64,
    pub entry_point: u64,
}

pub const WINDOWS_PEB_STUB_ADDR: u64 = 0x7FFDF000;
pub const WINDOWS_TEB_STUB_ADDR: u64 = 0x7FFDE000;
pub const PE_DEFAULT_BASE_ADDR: u64 = 0x400000;
pub const PE_STUB_ENTRY_POINT: u64 = 0x401000;

pub fn load_pe(proc: &mut Process, data: &[u8]) -> Result<PeProcess, &'static str> {
    if !is_pe_file(data) {
        return Err("Not a valid PE file");
    }
    if crate::compatibility_contract::CompatibilityContract::require_placeholder_available(
        "windows.pe.load",
    )
    .is_err()
    {
        return Err("windows: PE loader placeholder gated until roadmap Phase 6");
    }

    // Scaffold for mapped PE loader and PEB/TEB initialization.
    crate::serial_println!("load_pe: Discovered MZ/PE headers. Parsing sections...");

    proc.is_windows_process = true;
    proc.namespace_view = crate::vfs::namespace::NamespaceView::Windows;

    // Allocate space, setup TEB/PEB, resolve IAT, etc.
    proc.peb_addr = WINDOWS_PEB_STUB_ADDR;
    proc.teb_addr = WINDOWS_TEB_STUB_ADDR;

    Ok(PeProcess {
        base_addr: PE_DEFAULT_BASE_ADDR,
        entry_point: PE_STUB_ENTRY_POINT,
    })
}
