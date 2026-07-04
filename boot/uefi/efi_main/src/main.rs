#![no_std]
#![no_main]
extern crate alloc;
use alloc::vec::Vec;
use core::arch::asm;
use core::time::Duration;
use uefi::mem::memory_map::MemoryMap;
use uefi::boot::{AllocateType, MemoryType};
use uefi::println;
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::file::{File, FileAttribute, FileMode, FileType};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::*;

const COM1_PORT: u16 = 0x3F8;

#[inline(always)]
fn io_in8(port: u16) -> u8 {
    let value: u8;
    unsafe {
        asm!(
            "in al, dx",
            in("dx") port,
            out("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

fn boot_log(message: &str) {
    println!("[boot] {}", message);
    serial_write_str("[boot] ");
    serial_write_str(message);
    serial_write_str("\n");
}

#[inline(always)]
fn io_out8(port: u16, value: u8) {
    unsafe {
        asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}

fn init_serial_com1() {
    // 16550 init: 38400 8N1, FIFO enabled.
    io_out8(COM1_PORT + 1, 0x00); // disable interrupts
    io_out8(COM1_PORT + 3, 0x80); // DLAB on
    io_out8(COM1_PORT, 0x03); // divisor low byte
    io_out8(COM1_PORT + 1, 0x00); // divisor high byte
    io_out8(COM1_PORT + 3, 0x03); // 8N1
    io_out8(COM1_PORT + 2, 0xC7); // FIFO enable, clear, 14-byte threshold
    io_out8(COM1_PORT + 4, 0x0B); // IRQs enabled, RTS/DSR set
}

#[inline(always)]
fn serial_write_byte(byte: u8) {
    while (io_in8(COM1_PORT + 5) & 0x20) == 0 {}
    io_out8(COM1_PORT, byte);
}

fn serial_write_str(s: &str) {
    for b in s.bytes() {
        if b == b'\n' {
            serial_write_byte(b'\r');
        }
        serial_write_byte(b);
    }
}

#[inline(always)]
fn trace_marker(marker: u8) {
    // Emit a raw byte to COM1 for handoff-stage tracing.
    io_out8(0x3F8, marker);
}

#[entry]
fn main() -> Status {
    init_serial_com1();
    serial_write_str("[boot] serial online\n");
    if let Err(e) = uefi::helpers::init() {
        println!("UEFI init failed: {:?}", e);
        serial_write_str("[boot] UEFI init failed\n");
        return Status::LOAD_ERROR;
    }
    boot_log("uefi init ok");
    let boot_info = efi_main::initialize_boot_info();
    println!(
        "[boot] fb info: base={:#x} size={} {}x{} stride={} bpp={} bytespp={} fmt={:?} masks=({:#x},{:#x},{:#x},{:#x})",
        boot_info.framebuffer.base,
        boot_info.framebuffer.size,
        boot_info.framebuffer.width,
        boot_info.framebuffer.height,
        boot_info.framebuffer.stride,
        boot_info.framebuffer.bpp,
        core::cmp::max((boot_info.framebuffer.bpp).div_ceil(8), 1),
        boot_info.framebuffer.pixel_format,
        boot_info.framebuffer.red_mask,
        boot_info.framebuffer.green_mask,
        boot_info.framebuffer.blue_mask,
        boot_info.framebuffer.reserved_mask,
    );
    if boot_info.framebuffer.base != 0 {
        efi_main::ui::draw_boot_splash(boot_info.framebuffer);
    }

    let seed_path = "\\SAIOS\\seed.elf";
    let loader = match load_seed(seed_path) {
        Ok(loader) => loader,
        Err(e) => {
            println!("load_seed failed: {:?}", e.status());
            return e.status();
        }
    };
    boot_log("seed loaded");
    let mut image_start = u64::MAX;
    let mut image_end = 0u64;
    for i in 0..loader.image.elf_header.phnum {
        let ph = &loader.image.program_headers[i as usize];
        if ph.p_type != efi_main::ProgramHeaderType::Load as u32 {
            continue;
        }
        image_start = image_start.min(ph.p_vaddr);
        image_end = image_end.max(ph.p_vaddr + ph.p_memsz);
    }
    const PAGE_SIZE: u64 = 4096;
    let aligned_start = image_start & !(PAGE_SIZE - 1);
    let aligned_end = (image_end + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

    let total_size = aligned_end - aligned_start;
    let pages = (total_size / PAGE_SIZE) as usize;
    let base = match boot::allocate_pages(
        AllocateType::Address(aligned_start),
        MemoryType::LOADER_DATA,
        pages,
    ) {
        Ok(base) => base,
        Err(_) => {
            println!(
                "Kernel fixed-address allocation failed at {:#x}; static kernel cannot relocate",
                aligned_start
            );
            return Status::OUT_OF_RESOURCES;
        }
    };
    boot_log("kernel memory allocated");
    for i in 0..loader.image.elf_header.phnum {
        let ph = &loader.image.program_headers[i as usize];

        if ph.p_type != efi_main::ProgramHeaderType::Load as u32 {
            continue;
        }
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
    boot_log("segments copied");
    let stack_pages = 16;

    let stack =
        match boot::allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, stack_pages) {
            Ok(stack) => stack,
            Err(e) => {
                println!("Stack allocation failed: {:?}", e.status());
                return e.status();
            }
        };
    boot_log("stack allocated");

    let stack_base = stack.as_ptr() as u64;
    let stack_top = stack_base + stack_pages as u64 * 4096;
    let stack_span = stack_top.wrapping_sub(stack_base);
    if stack_top <= stack_base || stack_span < 4096 {
        println!(
            "[boot] invalid stack: pages={} base={:#x} top={:#x} span={:#x}",
            stack_pages, stack_base, stack_top, stack_span
        );
        return Status::LOAD_ERROR;
    }
    // Rust entry points expect call-compatible stack alignment (rsp % 16 == 8).
    let kernel_rsp = stack_top - 8;

    let boot_info_pages = (core::mem::size_of::<efi_main::SaiosBootInfo>() as u64).div_ceil(4096);
    let boot_info_storage = match boot::allocate_pages(
        AllocateType::AnyPages,
        MemoryType::LOADER_DATA,
        boot_info_pages as usize,
    ) {
        Ok(storage) => storage,
        Err(e) => {
            println!("Boot info allocation failed: {:?}", e.status());
            return e.status();
        }
    };
    let boot_info_ptr = boot_info_storage.as_ptr() as *mut efi_main::SaiosBootInfo;
    unsafe {
        boot_info_ptr.write(boot_info);
    }
    boot_log("boot info copied");

    // ── Pre-allocate storage for memory-map entries ──────────────────
    //
    // We must allocate the destination buffer BEFORE calling
    // memorymap::initialize() so that the allocation appears in the
    // UEFI memory map as a LOADER_DATA region.  The PMM will then
    // reserve those frames and the entries will never be overwritten.
    const MAX_ENTRIES: usize = 1024;
    let entry_size = core::mem::size_of::<efi_main::memorymap::MemoryRegion>();
    let entries_bytes = MAX_ENTRIES * entry_size; // 32 KiB
    let entries_pages = (entries_bytes as u64).div_ceil(4096);

    let entries_storage = match boot::allocate_pages(
        AllocateType::AnyPages,
        MemoryType::LOADER_DATA,
        entries_pages as usize,
    ) {
        Ok(storage) => storage,
        Err(e) => {
            println!("Memory-map storage allocation failed: {:?}", e.status());
            return e.status();
        }
    };
    boot_log("map storage allocated");

    let pre_exit_memorymap = match efi_main::memorymap::initialize() {
        Ok(memorymap) => memorymap,
        Err(e) => {
            println!("Pre-exit memory map capture failed: {:?}", e.status());
            return e.status();
        }
    };
    unsafe {
        (*boot_info_ptr).memorymap = pre_exit_memorymap;
    }
    boot_log("pre-exit map captured");

    let raw_entry = loader.entry_point;
    let entry = raw_entry.wrapping_add((base.as_ptr() as u64).wrapping_sub(aligned_start));
    if entry < (base.as_ptr() as u64) || entry >= ((base.as_ptr() as u64) + total_size) {
        println!(
            "[boot] invalid entry: raw={:#x} resolved={:#x} load_range=[{:#x}..{:#x})",
            raw_entry,
            entry,
            base.as_ptr() as u64,
            (base.as_ptr() as u64) + total_size
        );
        return Status::LOAD_ERROR;
    }
    drop(loader);
    boot_log("entry resolved");
    println!(
        "[boot] handoff: entry={:#x} stack={:#x} boot_info={:#x}",
        entry, kernel_rsp, boot_info_ptr as u64
    );
    boot::stall(Duration::from_secs(5));
    boot_log("exit boot services begin");

    trace_marker(b'1');
    let final_map = unsafe { boot::exit_boot_services(Some(MemoryType::LOADER_DATA)) };
    trace_marker(b'5');

    let final_entry_count = final_map.len();
    if final_entry_count > MAX_ENTRIES {
        loop {
            core::hint::spin_loop();
        }
    }

    if final_entry_count != 0 {
        let dst = entries_storage.as_ptr() as *mut efi_main::memorymap::MemoryRegion;
        for (idx, desc) in final_map.entries().enumerate() {
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
        trace_marker(b'D');
        asm!(
            "cli",
            "cld",
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
        println!(
            "Panic occurred at {}:{}:{}",
            loc.file(),
            loc.line(),
            message
        );
    } else {
        println!("Panic occurred: {}", message);
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
    let loaded_image = boot::open_protocol_exclusive::<LoadedImage>(boot::image_handle())
        .map_err(|_| uefi::Error::from(uefi::Status::LOAD_ERROR))?;

    let device = loaded_image
        .device()
        .ok_or(uefi::Error::from(uefi::Status::LOAD_ERROR))?;

    let mut fs = boot::open_protocol_exclusive::<SimpleFileSystem>(device)
        .map_err(|_| uefi::Error::from(uefi::Status::LOAD_ERROR))?;

    let mut root = fs
        .open_volume()
        .map_err(|_| uefi::Error::from(uefi::Status::LOAD_ERROR))?;

    let cstr_path =
        CString16::try_from(path).map_err(|_| uefi::Error::from(uefi::Status::LOAD_ERROR))?;

    let file = root
        .open(&cstr_path, FileMode::Read, FileAttribute::empty())
        .map_err(|_| uefi::Error::from(uefi::Status::NOT_FOUND))?;

    let mut regular = match file
        .into_type()
        .map_err(|_| uefi::Error::from(uefi::Status::LOAD_ERROR))?
    {
        FileType::Regular(f) => f,
        _ => return Err(uefi::Error::from(uefi::Status::LOAD_ERROR)),
    };

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
    drop(regular);
    drop(root);
    drop(fs);
    drop(loaded_image);
    drop(cstr_path);
    let header = efi_main::load_elf64_header(&buffer[..size])
        .map_err(|_| uefi::Error::from(uefi::Status::LOAD_ERROR))?;

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
