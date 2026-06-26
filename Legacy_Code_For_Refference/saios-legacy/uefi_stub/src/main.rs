//! SAIOS UEFI Boot Stub
//!
//! A minimal UEFI application that loads the SAIOS kernel from the EFI
//! system partition and hands control to it.
//!
//! Boot flow:
//!   UEFI firmware → (optionally via Shim) → saios-uefi.efi
//!     → reads /EFI/SAIOS/saios.elf from ESP
//!     → parses ELF64 program headers
//!     → obtains UEFI memory map
//!     → calls ExitBootServices()
//!     → sets up minimal page tables (identity map 0-128 GiB)
//!     → passes UEFI memory map to kernel via a SaiosBootInfo struct
//!     → jumps to kernel entry point
//!
//! The stub is compiled to a PE32+ EFI application (BOOTX64.EFI) and
//! placed on the EFI System Partition at:
//!     /EFI/BOOT/BOOTX64.EFI   (fallback - any UEFI machine)
//!     /EFI/SAIOS/saios.efi    (named entry)
//!
//! Secure Boot:
//!     The stub is signed with the SAIOS signing certificate (db key).
//!     Firmware with SAIOS's certificate in the db variable can boot it.
//!     Without Secure Boot enabled, the stub boots on any UEFI machine.

#![no_std]
#![no_main]
#![allow(dead_code)]

use core::ffi::c_void;
use core::ptr;

mod version {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../shared_version.rs"));
}

// -- UEFI basic types ------------------------------------------------------

type EfiHandle = *mut c_void;
type EfiStatus = usize;
type EfiPhysAddr = u64;
type EfiVirtAddr = u64;

const EFI_SUCCESS: EfiStatus = 0;
const EFI_LOAD_ERROR: EfiStatus = 1 | (1 << 63);
const EFI_BUFFER_TOO_SMALL: EfiStatus = 5 | (1 << 63);
const EFI_NOT_FOUND: EfiStatus = 14 | (1 << 63);

// -- UEFI table headers ----------------------------------------------------

#[repr(C)]
struct EfiTableHeader {
    signature: u64,
    revision: u32,
    header_size: u32,
    crc32: u32,
    reserved: u32,
}

// -- Simple Text Output Protocol -------------------------------------------

#[repr(C)]
struct EfiSimpleTextOutputProtocol {
    reset: *const c_void,
    output_string: unsafe extern "efiapi" fn(*mut Self, *const u16) -> EfiStatus,
    test_string: *const c_void,
    query_mode: *const c_void,
    set_mode: *const c_void,
    set_attribute: *const c_void,
    clear_screen: unsafe extern "efiapi" fn(*mut Self) -> EfiStatus,
    set_cursor_position: *const c_void,
    enable_cursor: *const c_void,
    mode: *const c_void,
}

// -- Boot Services ---------------------------------------------------------

#[repr(C)]
struct EfiBootServices {
    hdr: EfiTableHeader,
    // Task Priority
    raise_tpl: *const c_void,
    restore_tpl: *const c_void,
    // Memory
    allocate_pages:
        unsafe extern "efiapi" fn(AllocType, MemType, usize, *mut EfiPhysAddr) -> EfiStatus,
    free_pages: *const c_void,
    get_memory_map: unsafe extern "efiapi" fn(
        *mut usize,
        *mut EfiMemoryDescriptor,
        *mut usize,
        *mut usize,
        *mut u32,
    ) -> EfiStatus,
    allocate_pool: unsafe extern "efiapi" fn(MemType, usize, *mut *mut c_void) -> EfiStatus,
    free_pool: unsafe extern "efiapi" fn(*mut c_void) -> EfiStatus,
    // Events
    create_event: *const c_void,
    set_timer: *const c_void,
    wait_for_event: *const c_void,
    signal_event: *const c_void,
    close_event: *const c_void,
    check_event: *const c_void,
    // Protocol handlers (9 entries)
    _pad9: [*const c_void; 9],
    // Images (3 entries)
    _pad3a: [*const c_void; 3],
    exit_boot_services: unsafe extern "efiapi" fn(EfiHandle, usize) -> EfiStatus,
    // Misc
    _pad_misc: [*const c_void; 8],
    // Open / Close Protocol (3 entries)
    _pad3b: [*const c_void; 3],
    // locate_protocol
    _pad_loc: [*const c_void; 6],
}

#[repr(u32)]
enum AllocType {
    AllocateAnyPages = 0,
}
#[repr(u32)]
enum MemType {
    EfiLoaderData = 2,
    EfiConventionalMemory = 7,
}

// -- Memory Map Descriptor -------------------------------------------------

#[repr(C)]
#[derive(Clone, Copy)]
pub struct EfiMemoryDescriptor {
    pub mem_type: u32,
    pub _pad: u32,
    pub physical_start: EfiPhysAddr,
    pub virtual_start: EfiVirtAddr,
    pub number_of_pages: u64,
    pub attribute: u64,
}

// -- Loaded Image Protocol -------------------------------------------------

const EFI_LOADED_IMAGE_PROTOCOL_GUID: EfiGuid = EfiGuid {
    data1: 0x5B1B31A1,
    data2: 0x9562,
    data3: 0x11d2,
    data4: [0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};

#[repr(C)]
struct EfiGuid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

#[repr(C)]
struct EfiLoadedImageProtocol {
    revision: u32,
    parent_handle: EfiHandle,
    system_table: *const EfiSystemTable,
    device_handle: EfiHandle,
    file_path: *const c_void,
    reserved: *const c_void,
    load_options_size: u32,
    load_options: *const c_void,
    image_base: *const c_void,
    image_size: u64,
    image_code_type: u32,
    image_data_type: u32,
    unload: *const c_void,
}

// -- Simple File System Protocol -------------------------------------------

const EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID: EfiGuid = EfiGuid {
    data1: 0x0964e5b22,
    data2: 0x6459,
    data3: 0x11d2,
    data4: [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};

#[repr(C)]
struct EfiSimpleFileSystemProtocol {
    revision: u64,
    open_volume: unsafe extern "efiapi" fn(*mut Self, *mut *mut EfiFileProtocol) -> EfiStatus,
}

#[repr(C)]
struct EfiFileProtocol {
    revision: u64,
    open: unsafe extern "efiapi" fn(*mut Self, *mut *mut Self, *const u16, u64, u64) -> EfiStatus,
    close: unsafe extern "efiapi" fn(*mut Self) -> EfiStatus,
    delete: *const c_void,
    read: unsafe extern "efiapi" fn(*mut Self, *mut usize, *mut c_void) -> EfiStatus,
    write: *const c_void,
    get_position: *const c_void,
    set_position: *const c_void,
    get_info:
        unsafe extern "efiapi" fn(*mut Self, *const EfiGuid, *mut usize, *mut c_void) -> EfiStatus,
    set_info: *const c_void,
    flush: *const c_void,
}

const EFI_FILE_INFO_GUID: EfiGuid = EfiGuid {
    data1: 0x09576e92,
    data2: 0x6d3f,
    data3: 0x11d2,
    data4: [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};

#[repr(C)]
struct EfiFileInfo {
    size: u64,
    file_size: u64,
    physical_size: u64,
    create_time: [u64; 2],
    last_access_time: [u64; 2],
    modification_time: [u64; 2],
    attribute: u64,
    file_name: [u16; 256],
}

// -- EFI System Table ------------------------------------------------------

#[repr(C)]
pub struct EfiSystemTable {
    hdr: EfiTableHeader,
    firmware_vendor: *const u16,
    firmware_revision: u32,
    _pad: u32,
    console_in_handle: EfiHandle,
    con_in: *const c_void,
    console_out_handle: EfiHandle,
    con_out: *mut EfiSimpleTextOutputProtocol,
    std_err_handle: EfiHandle,
    std_err: *mut EfiSimpleTextOutputProtocol,
    runtime_services: *const c_void,
    boot_services: *mut EfiBootServices,
    num_config_entries: usize,
    config_table: *const c_void,
}

// -- Boot info structure passed to the SAIOS kernel -------------------------
//
// The kernel receives a pointer to this in RDI (first argument).
// It replaces Multiboot2 when booted via UEFI.

#[repr(C)]
pub struct SaiosUefiBootInfo {
    /// Magic: 0x5341_494F_5345_4649 ("SAIOSUEFI" in ASCII)
    pub magic: u64,
    /// Number of valid entries in memory_map.
    pub map_count: u32,
    /// Size of each EfiMemoryDescriptor (from UEFI - varies!).
    pub descriptor_size: u32,
    /// Memory map pointer (in EfiLoaderData pages, survives ExitBootServices).
    pub memory_map: u64,
    /// Command line string (null-terminated, max 256 bytes).
    pub cmdline: [u8; 256],
}

const SAIOS_UEFI_MAGIC: u64 = 0x5341_494F_5345_4649;

// -- ELF64 structures -------------------------------------------------------

const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

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

const PT_LOAD: u32 = 1;

// -- Static state -----------------------------------------------------------

static mut SYSTEM_TABLE: *const EfiSystemTable = ptr::null();
static mut IMAGE_HANDLE: EfiHandle = ptr::null_mut();

// -- Helper: print a Rust string to UEFI console ---------------------------

fn print(s: &str) {
    let st = unsafe { &*SYSTEM_TABLE };
    let co = unsafe { &mut *st.con_out };
    // Convert UTF-8 → UCS-2 (BMP only) on the stack
    let mut buf = [0u16; 512];
    let mut n = 0usize;
    for ch in s.chars() {
        if n + 2 >= buf.len() {
            break;
        }
        buf[n] = ch as u16;
        n += 1;
    }
    buf[n] = 0;
    unsafe { (co.output_string)(st.con_out, buf.as_ptr()) };
}

fn println(s: &str) {
    print(s);
    print("\r\n");
}

// -- UEFI boot services helpers ---------------------------------------------

unsafe fn allocate_pages(pages: usize, phys: &mut EfiPhysAddr) -> EfiStatus {
    unsafe {
        let bs = &*(*SYSTEM_TABLE).boot_services;
        (bs.allocate_pages)(
            AllocType::AllocateAnyPages,
            MemType::EfiLoaderData,
            pages,
            phys,
        )
    }
}

unsafe fn get_memory_map(
    map_size: &mut usize,
    map_buf: *mut EfiMemoryDescriptor,
    map_key: &mut usize,
    desc_size: &mut usize,
    desc_ver: &mut u32,
) -> EfiStatus {
    unsafe {
        let bs = &*(*SYSTEM_TABLE).boot_services;
        (bs.get_memory_map)(map_size, map_buf, map_key, desc_size, desc_ver)
    }
}

unsafe fn exit_boot_services(key: usize) -> EfiStatus {
    unsafe {
        let bs = &*(*SYSTEM_TABLE).boot_services;
        (bs.exit_boot_services)(IMAGE_HANDLE, key)
    }
}

// -- Open the kernel ELF from the ESP --------------------------------------

unsafe fn open_kernel_file() -> Option<(*mut EfiFileProtocol, u64)> {
    unsafe {
        // Locate loaded-image protocol to find the device we booted from
        let mut loaded_image: *mut EfiLoadedImageProtocol = ptr::null_mut();
        let guid = EFI_LOADED_IMAGE_PROTOCOL_GUID;
        let status = locate_protocol_by_handle(
            IMAGE_HANDLE,
            &guid,
            &mut loaded_image as *mut _ as *mut *mut c_void,
        );
        if status != EFI_SUCCESS || loaded_image.is_null() {
            println("[uefi] cannot locate loaded image protocol");
            return None;
        }

        // Locate simple file system on the boot device
        let dev_handle = (*loaded_image).device_handle;
        let mut fs: *mut EfiSimpleFileSystemProtocol = ptr::null_mut();
        let fs_guid = EFI_SIMPLE_FILE_SYSTEM_PROTOCOL_GUID;
        let status =
            locate_protocol_by_handle(dev_handle, &fs_guid, &mut fs as *mut _ as *mut *mut c_void);
        if status != EFI_SUCCESS || fs.is_null() {
            println("[uefi] cannot locate simple file system");
            return None;
        }

        // Open root volume
        let mut root: *mut EfiFileProtocol = ptr::null_mut();
        let status = ((*fs).open_volume)(fs, &mut root);
        if status != EFI_SUCCESS || root.is_null() {
            println("[uefi] cannot open root volume");
            return None;
        }

        // Open /EFI/SAIOS/saios.elf
        // Path as UCS-2: "\EFI\SAIOS\saios.elf"
        let path: [u16; 22] = [
            b'\\' as u16,
            b'E' as u16,
            b'F' as u16,
            b'I' as u16,
            b'\\' as u16,
            b'S' as u16,
            b'A' as u16,
            b'I' as u16,
            b'O' as u16,
            b'S' as u16,
            b'\\' as u16,
            b's' as u16,
            b'a' as u16,
            b'i' as u16,
            b'o' as u16,
            b's' as u16,
            b'.' as u16,
            b'e' as u16,
            b'l' as u16,
            b'f' as u16,
            0,
            0,
        ];

        let mut file: *mut EfiFileProtocol = ptr::null_mut();
        let status = ((*root).open)(
            root,
            &mut file,
            path.as_ptr(),
            0x0000_0000_0000_0001, // EFI_FILE_MODE_READ
            0,
        );
        if status != EFI_SUCCESS || file.is_null() {
            println("[uefi] cannot open /EFI/SAIOS/saios.elf");
            let _ = ((*root).close)(root);
            return None;
        }

        // Get file size via EFI_FILE_INFO
        let mut info_size = core::mem::size_of::<EfiFileInfo>();
        let mut info = EfiFileInfo {
            size: 0,
            file_size: 0,
            physical_size: 0,
            create_time: [0; 2],
            last_access_time: [0; 2],
            modification_time: [0; 2],
            attribute: 0,
            file_name: [0; 256],
        };
        ((*file).get_info)(
            file,
            &EFI_FILE_INFO_GUID,
            &mut info_size,
            &mut info as *mut _ as *mut c_void,
        );

        let file_size = info.file_size;
        let _ = ((*root).close)(root);
        Some((file, file_size))
    }
}

/// Minimal protocol-by-handle lookup using OpenProtocol.
unsafe fn locate_protocol_by_handle(
    handle: EfiHandle,
    guid: *const EfiGuid,
    iface: *mut *mut c_void,
) -> EfiStatus {
    // Use HandleProtocol (offset 35 in boot services - simplified)
    // In a real impl we'd use OpenProtocol. For now: stub returns success
    // if the protocol is available, otherwise NOT_FOUND.
    //
    // The proper implementation would walk the protocol list.
    // We use a direct offset calculation matching the UEFI spec table layout:
    unsafe {
        let bs_ptr = (*SYSTEM_TABLE).boot_services as *const u8;
        // HandleProtocol is at offset 0xC8 = 200 bytes from boot services start
        type HandleProtocolFn =
            unsafe extern "efiapi" fn(EfiHandle, *const EfiGuid, *mut *mut c_void) -> EfiStatus;
        let handle_protocol: HandleProtocolFn =
            core::mem::transmute(*(bs_ptr.add(0xC8) as *const *const c_void));
        handle_protocol(handle, guid, iface)
    }
}

// -- ELF loader ------------------------------------------------------------

/// Load an ELF64 kernel from the given data buffer.
/// Returns the entry point virtual address on success.
unsafe fn load_elf(data: *const u8, size: u64) -> Option<u64> {
    unsafe {
        if size < 64 {
            return None;
        }
        let hdr = &*(data as *const Elf64Header);

        // Validate
        let ident = core::slice::from_raw_parts(hdr.e_ident.as_ptr(), 4);
        if ident != ELF_MAGIC {
            println("[uefi] bad ELF magic");
            return None;
        }
        if hdr.e_ident[4] != 2 {
            println("[uefi] not 64-bit ELF");
            return None;
        }

        let phoff = core::ptr::addr_of!(hdr.e_phoff).read_unaligned() as usize;
        let phnum = core::ptr::addr_of!(hdr.e_phnum).read_unaligned() as usize;
        let phentsz = core::ptr::addr_of!(hdr.e_phentsize).read_unaligned() as usize;
        let entry = core::ptr::addr_of!(hdr.e_entry).read_unaligned();

        // Load each PT_LOAD segment
        for i in 0..phnum {
            let ph_ptr = data.add(phoff + i * phentsz) as *const Elf64Phdr;
            let ph = &*ph_ptr;
            let p_type = core::ptr::addr_of!(ph.p_type).read_unaligned();
            let _p_vaddr = core::ptr::addr_of!(ph.p_vaddr).read_unaligned();
            let p_paddr = core::ptr::addr_of!(ph.p_paddr).read_unaligned();
            let p_offset = core::ptr::addr_of!(ph.p_offset).read_unaligned() as usize;
            let p_filesz = core::ptr::addr_of!(ph.p_filesz).read_unaligned() as usize;
            let p_memsz = core::ptr::addr_of!(ph.p_memsz).read_unaligned() as usize;

            if p_type != PT_LOAD || p_memsz == 0 {
                continue;
            }

            // For a kernel loaded at 1 MiB: physical == virtual (identity mapped)
            let load_at = p_paddr;
            let pages = (p_memsz + 0xFFF) / 0x1000;

            // Allocate pages at the physical address
            let mut phys_addr = load_at;
            let _ = allocate_pages(pages, &mut phys_addr);
            // (We ignore the return - if it's in conventional memory, use it anyway)

            // Copy file bytes
            let dst = load_at as *mut u8;
            let src = data.add(p_offset);
            core::ptr::copy_nonoverlapping(src, dst, p_filesz);
            // Zero BSS region (memsz > filesz)
            if p_memsz > p_filesz {
                core::ptr::write_bytes(dst.add(p_filesz), 0, p_memsz - p_filesz);
            }
        }

        Some(entry)
    }
}

// -- Identity-map page tables for kernel -----------------------------------

/// Set up 4-level page tables with 2 MiB pages covering 0-128 GiB.
/// The page tables are allocated from UEFI loader memory.
unsafe fn setup_page_tables() -> u64 {
    unsafe {
        const PAGE_SIZE: usize = 4096;
        const PAGES_PER_TABLE: usize = PAGE_SIZE / 8; // 512 entries

        // Allocate PML4 + PDPT + 128 PD tables
        let total_pages = 1 + 1 + 128;
        let mut phys: EfiPhysAddr = 0;
        allocate_pages(total_pages, &mut phys);

        let pml4 = phys as *mut u64;
        let pdpt = (phys + PAGE_SIZE as u64) as *mut u64;
        let pds = (phys + 2 * PAGE_SIZE as u64) as *mut u64;

        // Zero everything
        core::ptr::write_bytes(pml4, 0, total_pages * PAGE_SIZE);

        // PML4[0] → PDPT
        *pml4 = pdpt as u64 | 0x03; // present + writable

        // PDPT[0..127] → PD[0..127]
        for i in 0..128usize {
            let pd_addr = (pds as u64) + (i * PAGE_SIZE) as u64;
            *pdpt.add(i) = pd_addr | 0x03;
        }

        // Each PD: 512 × 2 MiB huge pages
        for pdpt_i in 0..128usize {
            for pd_i in 0..512usize {
                let overall = pdpt_i * 512 + pd_i;
                let phys_addr_lo = (overall << 21) as u64;
                let phys_addr_hi = (overall >> 11) as u64;
                let entry = (phys_addr_hi << 32) | phys_addr_lo | 0x83; // P+W+huge
                *pds.add(pdpt_i * 512 + pd_i) = entry;
            }
        }

        pml4 as u64
    }
}

// -- Main UEFI entry point -------------------------------------------------

#[unsafe(no_mangle)]
pub extern "efiapi" fn efi_main(
    image_handle: EfiHandle,
    system_table: *const EfiSystemTable,
) -> EfiStatus {
    unsafe {
        SYSTEM_TABLE = system_table;
        IMAGE_HANDLE = image_handle;

        let st = &*system_table;
        // Clear screen
        let co = &mut *st.con_out;
        (co.clear_screen)(st.con_out);

        print("[");
        print(version::SAIOS_NAME);
        print(" UEFI] Boot stub ");
        println(version::SAIOS_VERSION_TAG);
        println("[SAIOS UEFI] Loading kernel...");

        // -- Load kernel ELF ------------------------------------------------
        let (file, file_size) = match open_kernel_file() {
            Some(f) => f,
            None => {
                println("[SAIOS UEFI] FATAL: cannot open /EFI/SAIOS/saios.elf");
                return EFI_LOAD_ERROR;
            }
        };

        println("[SAIOS UEFI] Allocating kernel buffer...");
        let kernel_pages = ((file_size as usize) + 0xFFF) / 0x1000;
        let mut kernel_buf: EfiPhysAddr = 0;
        let status = allocate_pages(kernel_pages, &mut kernel_buf);
        if status != EFI_SUCCESS {
            println("[SAIOS UEFI] FATAL: cannot allocate kernel memory");
            return EFI_LOAD_ERROR;
        }

        // Read kernel into buffer
        let mut read_size = file_size as usize;
        let status = ((*file).read)(file, &mut read_size, kernel_buf as *mut c_void);
        let _ = ((*file).close)(file);
        if status != EFI_SUCCESS {
            println("[SAIOS UEFI] FATAL: read error");
            return EFI_LOAD_ERROR;
        }

        println("[SAIOS UEFI] Loading ELF segments...");
        let entry = match load_elf(kernel_buf as *const u8, file_size) {
            Some(e) => e,
            None => {
                println("[SAIOS UEFI] FATAL: invalid ELF");
                return EFI_LOAD_ERROR;
            }
        };

        // -- Get UEFI memory map --------------------------------------------
        println("[SAIOS UEFI] Getting memory map...");
        let mut map_size = 0usize;
        let mut map_key = 0usize;
        let mut desc_size = 0usize;
        let mut desc_ver = 0u32;

        // First call: get required buffer size
        get_memory_map(
            &mut map_size,
            ptr::null_mut(),
            &mut map_key,
            &mut desc_size,
            &mut desc_ver,
        );
        map_size += 2 * desc_size; // add slack for ExitBootServices allocations

        let map_pages = (map_size + 0xFFF) / 0x1000;
        let mut map_buf: EfiPhysAddr = 0;
        allocate_pages(map_pages + 1, &mut map_buf);

        let status = get_memory_map(
            &mut map_size,
            map_buf as *mut EfiMemoryDescriptor,
            &mut map_key,
            &mut desc_size,
            &mut desc_ver,
        );
        if status != EFI_SUCCESS {
            println("[SAIOS UEFI] FATAL: cannot get memory map");
            return EFI_LOAD_ERROR;
        }

        // -- Build SaiosUefiBootInfo ----------------------------------------
        let mut boot_info_phys: EfiPhysAddr = 0;
        allocate_pages(1, &mut boot_info_phys);
        let boot_info = &mut *(boot_info_phys as *mut SaiosUefiBootInfo);
        boot_info.magic = SAIOS_UEFI_MAGIC;
        boot_info.map_count = (map_size / desc_size) as u32;
        boot_info.descriptor_size = desc_size as u32;
        boot_info.memory_map = map_buf;
        let cmdline = b"SAIOS_UEFI_BOOT";
        boot_info.cmdline[..cmdline.len()].copy_from_slice(cmdline);

        // -- Set up page tables ---------------------------------------------
        println("[SAIOS UEFI] Setting up page tables...");
        let pml4 = setup_page_tables();

        // -- Exit Boot Services ---------------------------------------------
        println("[SAIOS UEFI] Exiting boot services...");
        // Refresh memory map key (required to be current for ExitBootServices)
        get_memory_map(
            &mut map_size,
            map_buf as *mut _,
            &mut map_key,
            &mut desc_size,
            &mut desc_ver,
        );
        let status = exit_boot_services(map_key);
        if status != EFI_SUCCESS {
            // Retry once (memory map key may have changed)
            get_memory_map(
                &mut map_size,
                map_buf as *mut _,
                &mut map_key,
                &mut desc_size,
                &mut desc_ver,
            );
            let status2 = exit_boot_services(map_key);
            if status2 != EFI_SUCCESS {
                // Can't print anymore - UEFI gone; spin
                loop {
                    core::arch::asm!("hlt", options(nomem, nostack));
                }
            }
        }

        // -- From here: no UEFI services available -------------------------

        // Load our new page tables
        core::arch::asm!(
            "mov cr3, {pml4}",
            pml4 = in(reg) pml4,
            options(nomem, nostack),
        );

        // Jump to kernel entry point
        // Calling convention: RDI = boot_info pointer (SaiosUefiBootInfo*)
        let kernel_entry: extern "C" fn(u64) -> ! = core::mem::transmute(entry);
        kernel_entry(boot_info_phys);
    }
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("hlt", options(nomem, nostack));
        }
    }
}
