#![no_std]
#![no_main]
extern crate alloc;
use alloc::vec::Vec;
use core::arch::asm;
use core::time::Duration;
use uefi::mem::memory_map::MemoryMap;
use uefi::boot::{AllocateType, EventType, MemoryType, TimerTrigger, Tpl};
use uefi::println;
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::file::{File, FileAttribute, FileMode, FileType};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::*;

#[entry]
fn main() -> Status {
    if let Err(e) = uefi::helpers::init() {
        let _ = println!("UEFI init failed: {:?}", e);
        return Status::LOAD_ERROR;
    }
    let boot_info = efi_main::initialize_boot_info();

    let seed_path = "\\SAIOS\\seed.elf";
    let mut loader = match load_seed(seed_path) {
        Ok(loader) => loader,
        Err(e) => {
            let _ = println!("load_seed failed: {:?}", e.status());
            return e.status();
        }
    };
    let dynamic =
        if let Some(segment) = efi_main::find_dynamic_segment(&loader.image.program_headers) {
            Some(efi_main::parse_dynamic(&loader.image.bytes, segment))
        } else {
            None
        };
    loader.image.dynamic = dynamic;
    if let Some(dynamic) = &loader.image.dynamic {
        println!("Dynamic Section");

        println!("DT_RELA    : {:?}", dynamic.rela);
        println!("DT_RELASZ  : {:?}", dynamic.rela_size);
        println!("DT_RELAENT : {:?}", dynamic.rela_entry_size);
    }
    let relocations = if let Some(dynamic) = &loader.image.dynamic {
        if let (Some(rela_offset), Some(rela_size), Some(rela_entry_size)) =
            (dynamic.rela, dynamic.rela_size, dynamic.rela_entry_size)
        {
            let num_relocations = (rela_size / rela_entry_size) as usize;
            let mut relocations = Vec::with_capacity(num_relocations);
            for i in 0..num_relocations {
                let rela_vaddr = rela_offset.saturating_add((i as u64).saturating_mul(rela_entry_size));
                let Some(offset) = efi_main::virtual_to_file_offset(&loader.image.program_headers, rela_vaddr) else {
                    break;
                };
                if offset + core::mem::size_of::<efi_main::Elf64Rela>() > loader.image.bytes.len() {
                    break;
                }
                let rela: efi_main::Elf64Rela = unsafe {
                    core::ptr::read_unaligned(
                        loader.image.bytes[offset..].as_ptr() as *const efi_main::Elf64Rela
                    )
                };
                relocations.push(rela);
            }
            relocations
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };
    loader.image.relocations = relocations;
    let mut image_start = u64::MAX;
    let mut image_end = 0u64;
    for i in 0..loader.image.elf_header.phnum {
        let ph = &loader.image.program_headers[i as usize];
        if ph.p_type != efi_main::ProgramHeaderType::Load as u32 {
            continue;
        }
        println!(
            "Reading segment {}: offset {:#x}, vaddr {:#x}, filesz {:#x}, memsz {:#x}",
            i, ph.p_offset, ph.p_vaddr, ph.p_filesz, ph.p_memsz
        );
        image_start = image_start.min(ph.p_vaddr);
        image_end = image_end.max(ph.p_vaddr + ph.p_memsz);
        println!(
            "Segment {}: Image start: {:#x}, Image end: {:#x}, Total size: {:#x}",
            i,
            image_start,
            image_end,
            image_end - image_start
        );
    }
    println!(
        "Final Image start: {:#x}, Image end: {:#x}, Total size: {:#x}",
        image_start,
        image_end,
        image_end - image_start
    );
    const PAGE_SIZE: u64 = 4096;
    println!(
        "Aligning image start and end to page boundaries (page size: {:#x})",
        PAGE_SIZE
    );
    let aligned_start = image_start & !(PAGE_SIZE - 1);
    let aligned_end = (image_end + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

    let total_size = aligned_end - aligned_start;
    let pages = (total_size / PAGE_SIZE) as usize;
    println!(
        "Aligned start: {:#x}, Aligned end: {:#x}, Total size: {:#x}, Pages: {}",
        aligned_start, aligned_end, total_size, pages
    );
    println!(
        "Allocating {} pages at aligned start address {:#x}",
        pages, aligned_start
    );
    let mut used_fallback = false;
    let base = match boot::allocate_pages(
        AllocateType::Address(aligned_start),
        MemoryType::LOADER_DATA,
        pages,
    ) {
        Ok(base) => base,
        Err(_) => {
            if loader.image.elf_header.elf_type != 3 {
                let _ = println!(
                    "Kernel fixed-address allocation failed at {:#x}; ELF type={} is not relocatable",
                    aligned_start,
                    loader.image.elf_header.elf_type
                );
                return Status::OUT_OF_RESOURCES;
            }

            let _ = println!(
                "Kernel fixed-address allocation failed at {:#x}; retrying with AnyPages for ET_DYN",
                aligned_start
            );
            used_fallback = true;
            match boot::allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, pages) {
                Ok(base) => base,
                Err(e) => {
                    let _ = println!("Kernel fallback allocation failed: {:?}", e.status());
                    return e.status();
                }
            }
        }
    };
    println!("Allocated at {:p}", base.as_ptr());
    println!("Copying segments to allocated memory");
    for i in 0..loader.image.elf_header.phnum {
        let ph = &loader.image.program_headers[i as usize];

        if ph.p_type != efi_main::ProgramHeaderType::Load as u32 {
            continue;
        }

        println!(
            "Copying segment {}: offset {:#x}, vaddr {:#x}, filesz {:#x}, memsz {:#x}",
            i, ph.p_offset, ph.p_vaddr, ph.p_filesz, ph.p_memsz
        );
        let src = &loader.image.bytes[ph.p_offset as usize..(ph.p_offset + ph.p_filesz) as usize];

        let dst = (base.as_ptr() as u64 + (ph.p_vaddr - aligned_start)) as *mut u8;
        unsafe {
            core::ptr::copy_nonoverlapping(src.as_ptr(), dst, ph.p_filesz as usize);
        }
        unsafe {
            core::ptr::write_bytes(
                dst.add(ph.p_filesz as usize),
                0,
                (ph.p_memsz - ph.p_filesz) as usize,
            );
        }
    }
    println!("All segments copied successfully");
    if used_fallback {
        let load_bias = (base.as_ptr() as u64).wrapping_sub(aligned_start);
        if let Err(e) = efi_main::apply_relocations(load_bias, &loader.image.relocations) {
            let _ = println!("Fallback relocation failed: {}", e);
            return Status::LOAD_ERROR;
        }
    }
    println!("All relocations applied successfully");
    println!("Initializing boot information");

    // Print metadata fields explicitly for validation
    println!(
        "Magic Check:   0x{:X} (Expected: 0x{:X})",
        boot_info.magic,
        efi_main::SAIOS_BOOT_MAGIC
    );
    println!(
        "Boot Version:  {}.{}",
        boot_info.version >> 16,
        boot_info.version & 0xFFFF
    );
    println!("Struct Size:   {} bytes", boot_info.size);
    println!("----------------------------------------");

    // Print the sub-structures using the derived Debug trait
    println!("{:#?}", boot_info.framebuffer);
    println!("{:#?}", boot_info.acpi);
    println!("{:#?}", boot_info.smbios);
    println!("{:#?}", boot_info.cpu);
    println!("{:#?}", boot_info.firmware);

    println!("========================================");

    println!("ELF entry = {:#x}", loader.entry_point);
    println!("Jump to kernel entry point");
    let stack_pages = 16;

    let stack = match boot::allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, stack_pages) {
        Ok(stack) => stack,
        Err(e) => {
            let _ = println!("Stack allocation failed: {:?}", e.status());
            return e.status();
        }
    };

    let stack_top = stack.as_ptr() as u64 + stack_pages as u64 * 4096;
    // Rust entry points expect call-compatible stack alignment (rsp % 16 == 8).
    let kernel_rsp = stack_top - 8;

    let boot_info_pages =
        (core::mem::size_of::<efi_main::SaiosBootInfo>() as u64 + 4095) / 4096;
    let boot_info_storage = boot::allocate_pages(
        AllocateType::AnyPages,
        MemoryType::LOADER_DATA,
        boot_info_pages as usize,
    )
    .expect("Failed to allocate stable boot info storage");
    let boot_info_ptr = boot_info_storage.as_ptr() as *mut efi_main::SaiosBootInfo;
    unsafe {
        boot_info_ptr.write(boot_info.clone());
    }

    // ── Pre-allocate storage for memory-map entries ──────────────────
    //
    // We must allocate the destination buffer BEFORE calling
    // memorymap::initialize() so that the allocation appears in the
    // UEFI memory map as a LOADER_DATA region.  The PMM will then
    // reserve those frames and the entries will never be overwritten.
    const MAX_ENTRIES: usize = 1024;
    let entry_size = core::mem::size_of::<efi_main::memorymap::MemoryRegion>();
    let entries_bytes = MAX_ENTRIES * entry_size; // 32 KiB
    let entries_pages = (entries_bytes as u64 + 4095) / 4096;

    let entries_storage = boot::allocate_pages(
        AllocateType::AnyPages,
        MemoryType::LOADER_DATA,
        entries_pages as usize,
    )
    .expect("Failed to allocate stable memory-map storage");

    let pre_exit_memorymap =
        efi_main::memorymap::initialize().expect("Failed to capture pre-exit memory map");
    unsafe {
        (*boot_info_ptr).memorymap = pre_exit_memorymap;
    }

    let entry = loader
        .entry_point
        .wrapping_add((base.as_ptr() as u64).wrapping_sub(aligned_start));
    drop(loader);
    let p = entry as *const u8;
    println!("===============================================",);
    // Print the STABLE copy, not the stale local `boot_info`.
    unsafe {
        println!("{:#?}", (*boot_info_ptr).memorymap);
    }
    println!("==============================================");
    unsafe {
        println!(
            "entry bits {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}",
            *p.add(0),
            *p.add(1),
            *p.add(2),
            *p.add(3),
            *p.add(4),
            *p.add(5),
            *p.add(6),
            *p.add(7),
        );
    }

    let final_uefi_map = unsafe { boot::exit_boot_services(None) };
    let final_entry_count = final_uefi_map.len();
    if final_entry_count > MAX_ENTRIES {
        loop {
            core::hint::spin_loop();
        }
    }

    if final_entry_count != 0 {
        let dst = entries_storage.as_ptr() as *mut efi_main::memorymap::MemoryRegion;
        for (idx, desc) in final_uefi_map.entries().enumerate() {
            let region = efi_main::memorymap::MemoryRegion {
                base: desc.phys_start,
                length: desc.page_count * 4096,
                region_type: efi_main::memorymap::convert_memory_type(desc.ty),
                attributes: desc.att.bits(),
            };

            unsafe {
                core::ptr::write(dst.add(idx), region);
            }
        }

        unsafe {
            (*boot_info_ptr).memorymap = efi_main::memorymap::MemoryMapInfo {
                entries: dst,
                entry_count: final_entry_count,
            };
        }
    }

    unsafe {
        asm!(
            "mov rsp, {stack}",
            "jmp {entry}",
            stack = in(reg) kernel_rsp,
            entry = in(reg) entry,
            in("rdi") boot_info_ptr,
            options(noreturn)
        );
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!(" Extract the panic message and file/line location");
    let message = info.message();
    let location = info.location();

    if let Some(loc) = location {
        let _ = println!(
            "Panic occurred at {}:{}:{}",
            loc.file(),
            loc.line(),
            message
        );
    } else {
        let _ = println!("Panic occurred: {}", message);
    }
    loop {
        core::hint::spin_loop();
    }
}
pub struct ElfImage {
    /// Raw ELF file
    pub bytes: Vec<u8>,

    /// ELF header
    pub elf_header: efi_main::Elf64Header,

    /// All PT_* program headers
    pub program_headers: Vec<efi_main::Elf64ProgramHeader>,

    /// Parsed PT_DYNAMIC
    pub dynamic: Option<efi_main::DynamicInfo>,
    /// Parsed relocations
    pub relocations: Vec<efi_main::Elf64Rela>,
}

pub struct Loader {
    pub image: ElfImage,
    pub entry_point: u64,
}
pub fn load_seed(path: &str) -> uefi::Result<Loader> {
    println!(" Get LoadedImage protocol");
    let loaded_image = boot::open_protocol_exclusive::<LoadedImage>(boot::image_handle())
        .map_err(|_| uefi::Error::from(uefi::Status::LOAD_ERROR))?;

    println!("Map the Option into a uefi::Result safely");
    let device = loaded_image
        .device()
        .ok_or(uefi::Error::from(uefi::Status::LOAD_ERROR))?;

    println!("Get SimpleFileSystem protocol");
    let mut fs = boot::open_protocol_exclusive::<SimpleFileSystem>(device)
        .map_err(|_| uefi::Error::from(uefi::Status::LOAD_ERROR))?;

    println!("Open root volume");
    let mut root = fs
        .open_volume()
        .map_err(|_| uefi::Error::from(uefi::Status::LOAD_ERROR))?;

    println!("Convert &str → CString16");
    let cstr_path =
        CString16::try_from(path).map_err(|_| uefi::Error::from(uefi::Status::LOAD_ERROR))?;

    println!("Open file");
    let file = root
        .open(&cstr_path, FileMode::Read, FileAttribute::empty())
        .map_err(|_| uefi::Error::from(uefi::Status::NOT_FOUND))?;

    println!("Match on FileType::Regular");
    let mut regular = match file
        .into_type()
        .map_err(|_| uefi::Error::from(uefi::Status::LOAD_ERROR))?
    {
        FileType::Regular(f) => f,
        _ => return Err(uefi::Error::from(uefi::Status::LOAD_ERROR)),
    };

    println!("Read file into buffer");

    let mut buffer = Vec::with_capacity(1024 * 1024);
    let mut chunk = [0u8; 4096];
    loop {
        let n = regular
            .read(&mut chunk)
            .map_err(|_| uefi::Error::from(uefi::Status::LOAD_ERROR))?;
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);
    }
    let size = buffer.len();
    if size < core::mem::size_of::<efi_main::Elf64Header>() {
        return Err(uefi::Error::from(uefi::Status::LOAD_ERROR));
    }
    println!("Parse ELF header");
    drop(regular);
    drop(root);
    drop(fs);
    drop(loaded_image);
    drop(cstr_path);
    let header =
        efi_main::load_elf64_header(&buffer[..size]).map_err(|_| uefi::Error::from(uefi::Status::LOAD_ERROR))?;

    if header.ident[0..4] != [0x7F, b'E', b'L', b'F'] {
        return Err(uefi::Error::from(uefi::Status::LOAD_ERROR));
    }
    Ok(Loader {
        entry_point: header.entry,
        image: ElfImage {
            bytes: buffer[..size].to_vec(),
            elf_header: header.clone(),
            program_headers: (0..header.phnum)
                .map(|i| {
                    let offset = header.phoff as usize + i as usize * header.phentsize as usize;
                    efi_main::load_program_header(&buffer[..size], offset)
                        .map_err(|_| uefi::Error::from(uefi::Status::LOAD_ERROR))
                })
                .collect::<uefi::Result<Vec<efi_main::Elf64ProgramHeader>>>()?,
            dynamic: None,
            relocations: Vec::new(),
        },
    })
}
pub fn sleep(duration: Duration) -> u16 {
    let timer =
        unsafe { boot::create_event(EventType::TIMER, Tpl::APPLICATION, None, None).unwrap() };

    boot::set_timer(&timer, TimerTrigger::Relative(duration)).unwrap();
    boot::wait_for_event(&mut [timer]).unwrap();

    1
}
