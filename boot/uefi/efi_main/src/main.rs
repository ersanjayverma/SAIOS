#![no_std]
#![no_main]
extern crate alloc;
use alloc::vec::Vec;
use core::arch::asm;
use core::cell::UnsafeCell;
use core::time::Duration;
use uefi::boot::{AllocateType, EventType, MemoryType, TimerTrigger, Tpl};
use uefi::println;
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::file::{File, FileAttribute, FileMode, FileType};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::*;

const FB_BOOT_SMOKE_TEST: bool = true;

fn boot_log(message: &str) {
    println!("[boot] {}", message);
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

#[inline(always)]
fn trace_marker(marker: u8) {
    // Emit a raw byte to COM1 for handoff-stage tracing.
    io_out8(0x3F8, marker);
}

#[inline(always)]
fn serial_write_byte(value: u8) {
    io_out8(0x3F8, value);
}

fn serial_write_str(text: &str) {
    for b in text.bytes() {
        if b == b'\n' {
            serial_write_byte(b'\r');
        }
        serial_write_byte(b);
    }
}

fn serial_write_hex_u64(value: u64) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    serial_write_str("0x");
    for shift in (0..16).rev() {
        let nibble = ((value >> (shift * 4)) & 0xF) as usize;
        serial_write_byte(HEX[nibble]);
    }
}

fn serial_write_hex_usize(value: usize) {
    serial_write_hex_u64(value as u64);
}

#[inline(always)]
fn pack_channel(value: u8, mask: u32) -> u32 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let width = mask.count_ones();
    if width == 0 {
        return 0;
    }
    let max = (1u32 << width) - 1;
    let scaled = ((value as u32) * max + 127) / 255;
    (scaled << shift) & mask
}

#[inline(always)]
fn pack_bitmask(r: u8, g: u8, b: u8, masks: (u32, u32, u32, u32)) -> u32 {
    let (red_mask, green_mask, blue_mask, reserved_mask) = masks;
    pack_channel(r, red_mask)
        | pack_channel(g, green_mask)
        | pack_channel(b, blue_mask)
        | reserved_mask
}

unsafe fn write_packed(dst: *mut u8, packed: u32, bytes_per_pixel: usize) {
    let bytes = packed.to_le_bytes();
    let count = core::cmp::min(bytes_per_pixel, 4);
    let mut i = 0;
    while i < count {
        core::ptr::write_volatile(dst.add(i), bytes[i]);
        i += 1;
    }
}

unsafe fn put_pixel_with_stride(
    fb: &efi_main::graphics::FramebufferInfo,
    stride_pixels: usize,
    x: usize,
    y: usize,
    color: u32,
) {
    if x >= fb.width || y >= fb.height {
        return;
    }
    let bytes_per_pixel = core::cmp::max((fb.bpp + 7) / 8, 1);
    let Some(offset_pixels) = y.checked_mul(stride_pixels).and_then(|v| v.checked_add(x)) else {
        return;
    };
    let Some(offset) = offset_pixels.checked_mul(bytes_per_pixel) else {
        return;
    };
    if offset + bytes_per_pixel > fb.size {
        return;
    }

    let dst = (fb.base as *mut u8).add(offset);
    let r = ((color >> 16) & 0xFF) as u8;
    let g = ((color >> 8) & 0xFF) as u8;
    let b = (color & 0xFF) as u8;
    match fb.pixel_format {
        efi_main::graphics::PixelFormat::Rgb => {
            core::ptr::write_volatile(dst, r);
            if bytes_per_pixel >= 2 {
                core::ptr::write_volatile(dst.add(1), g);
            }
            if bytes_per_pixel >= 3 {
                core::ptr::write_volatile(dst.add(2), b);
            }
            if bytes_per_pixel >= 4 {
                core::ptr::write_volatile(dst.add(3), 0xFF);
            }
        }
        efi_main::graphics::PixelFormat::Bgr => {
            core::ptr::write_volatile(dst, b);
            if bytes_per_pixel >= 2 {
                core::ptr::write_volatile(dst.add(1), g);
            }
            if bytes_per_pixel >= 3 {
                core::ptr::write_volatile(dst.add(2), r);
            }
            if bytes_per_pixel >= 4 {
                core::ptr::write_volatile(dst.add(3), 0xFF);
            }
        }
        efi_main::graphics::PixelFormat::Bitmask => {
            let packed = pack_bitmask(
                r,
                g,
                b,
                (fb.red_mask, fb.green_mask, fb.blue_mask, fb.reserved_mask),
            );
            write_packed(dst, packed, bytes_per_pixel);
        }
        efi_main::graphics::PixelFormat::BltOnly => {}
    }
}

fn framebuffer_boot_smoke_test(fb: &efi_main::graphics::FramebufferInfo) {
    if fb.base == 0 || fb.width == 0 || fb.height == 0 || fb.stride == 0 {
        return;
    }

    let bytes_per_pixel = core::cmp::max((fb.bpp + 7) / 8, 1);
    unsafe {
        // Primary addressing assumption: stride is pixels-per-scanline.
        put_pixel_with_stride(fb, fb.stride, 0, 0, 0x00FF0000);
        put_pixel_with_stride(fb, fb.stride, fb.width / 2, fb.height / 2, 0x0000FF00);
        put_pixel_with_stride(
            fb,
            fb.stride,
            fb.width.saturating_sub(1),
            fb.height.saturating_sub(1),
            0x000000FF,
        );

        // Diagnostic line across top row.
        let max_x = core::cmp::min(fb.width, 512);
        for x in 0..max_x {
            let color = if x % 3 == 0 {
                0x00FFFFFF
            } else if x % 3 == 1 {
                0x00FF00FF
            } else {
                0x0000FFFF
            };
            put_pixel_with_stride(fb, fb.stride, x, 0, color);
        }

        // Secondary probe: if stride may be reported in bytes, draw a second marker.
        if bytes_per_pixel > 0 && fb.stride % bytes_per_pixel == 0 {
            let alt_stride = fb.stride / bytes_per_pixel;
            if alt_stride >= fb.width && alt_stride != fb.stride {
                put_pixel_with_stride(fb, alt_stride, 8, 8, 0x00FFFF00);
                put_pixel_with_stride(fb, alt_stride, 9, 8, 0x00FFFF00);
                put_pixel_with_stride(fb, alt_stride, 10, 8, 0x00FFFF00);
            }
        }
    }
}

struct ExitMapBuffer(UnsafeCell<[u8; 256 * 1024]>);

unsafe impl Sync for ExitMapBuffer {}

static EXIT_MAP_BUFFER: ExitMapBuffer = ExitMapBuffer(UnsafeCell::new([0u8; 256 * 1024]));

#[entry]
fn main() -> Status {
    if let Err(e) = uefi::helpers::init() {
        let _ = println!("UEFI init failed: {:?}", e);
        return Status::LOAD_ERROR;
    }
    boot_log("uefi init ok");
    boot_log("boot_info init begin");
    let boot_info = efi_main::initialize_boot_info();
    boot_log("boot_info init done");
    let _ = println!(
        "[boot] fb info: base={:#x} size={} {}x{} stride={} bpp={} bytespp={} fmt={:?} masks=({:#x},{:#x},{:#x},{:#x})",
        boot_info.framebuffer.base,
        boot_info.framebuffer.size,
        boot_info.framebuffer.width,
        boot_info.framebuffer.height,
        boot_info.framebuffer.stride,
        boot_info.framebuffer.bpp,
        core::cmp::max((boot_info.framebuffer.bpp + 7) / 8, 1),
        boot_info.framebuffer.pixel_format,
        boot_info.framebuffer.red_mask,
        boot_info.framebuffer.green_mask,
        boot_info.framebuffer.blue_mask,
        boot_info.framebuffer.reserved_mask,
    );
    if FB_BOOT_SMOKE_TEST {
        framebuffer_boot_smoke_test(&boot_info.framebuffer);
        let _ = println!("[boot] fb smoke test pattern drawn");
    }

    let seed_path = "\\SAIOS\\seed.elf";
    let loader = match load_seed(seed_path) {
        Ok(loader) => loader,
        Err(e) => {
            let _ = println!("load_seed failed: {:?}", e.status());
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
            let _ = println!(
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
    boot_log("boot info staging");
    let stack_pages = 16;

    let stack = match boot::allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, stack_pages) {
        Ok(stack) => stack,
        Err(e) => {
            let _ = println!("Stack allocation failed: {:?}", e.status());
            return e.status();
        }
    };
    boot_log("stack allocated");

    let stack_base = stack.as_ptr() as u64;
    let stack_top = stack_base + stack_pages as u64 * 4096;
    let stack_span = stack_top.wrapping_sub(stack_base);
    if stack_top <= stack_base || stack_span < 4096 {
        let _ = println!(
            "[boot] invalid stack: pages={} base={:#x} top={:#x} span={:#x}",
            stack_pages,
            stack_base,
            stack_top,
            stack_span
        );
        return Status::LOAD_ERROR;
    }
    // Rust entry points expect call-compatible stack alignment (rsp % 16 == 8).
    let kernel_rsp = stack_top - 8;

    let boot_info_pages =
        (core::mem::size_of::<efi_main::SaiosBootInfo>() as u64 + 4095) / 4096;
    let boot_info_storage = match boot::allocate_pages(
        AllocateType::AnyPages,
        MemoryType::LOADER_DATA,
        boot_info_pages as usize,
    ) {
        Ok(storage) => storage,
        Err(e) => {
            let _ = println!("Boot info allocation failed: {:?}", e.status());
            return e.status();
        }
    };
    let boot_info_ptr = boot_info_storage.as_ptr() as *mut efi_main::SaiosBootInfo;
    unsafe {
        boot_info_ptr.write(boot_info.clone());
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
    let entries_pages = (entries_bytes as u64 + 4095) / 4096;

    let entries_storage = match boot::allocate_pages(
        AllocateType::AnyPages,
        MemoryType::LOADER_DATA,
        entries_pages as usize,
    ) {
        Ok(storage) => storage,
        Err(e) => {
            let _ = println!("Memory-map storage allocation failed: {:?}", e.status());
            return e.status();
        }
    };
    boot_log("map storage allocated");

    let pre_exit_memorymap = match efi_main::memorymap::initialize() {
        Ok(memorymap) => memorymap,
        Err(e) => {
            let _ = println!("Pre-exit memory map capture failed: {:?}", e.status());
            return e.status();
        }
    };
    unsafe {
        (*boot_info_ptr).memorymap = pre_exit_memorymap;
    }
    boot_log("pre-exit map captured");

    let raw_entry = loader.entry_point;
    let entry = raw_entry
        .wrapping_add((base.as_ptr() as u64).wrapping_sub(aligned_start));
    if entry < (base.as_ptr() as u64) || entry >= ((base.as_ptr() as u64) + total_size) {
        let _ = println!(
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
    let _ = println!(
        "[boot] diag image: start={:#x} end={:#x} aligned_start={:#x} aligned_end={:#x} pages={} base={:#x}",
        image_start,
        image_end,
        aligned_start,
        aligned_end,
        pages,
        base.as_ptr() as u64
    );
    let _ = println!(
        "[boot] diag handoff: entry={:#x} rsp={:#x} boot_info={:#x} stack_base={:#x} stack_top={:#x}",
        entry,
        kernel_rsp,
        boot_info_ptr as u64,
        stack_base,
        stack_top
    );
    let _ = println!(
        "[boot] diag stack: pages={} span={:#x}",
        stack_pages,
        stack_span
    );
    let _ = println!(
        "[boot] diag elf: raw_entry={:#x} aligned_start={:#x} aligned_end={:#x}",
        raw_entry,
        aligned_start,
        aligned_end
    );
    let _ = println!(
        "[boot] diag mmap: pre_exit_entries={} max_entries={} entries_buf={:#x} entries_bytes={}",
        pre_exit_memorymap.entry_count,
        MAX_ENTRIES,
        entries_storage.as_ptr() as u64,
        entries_bytes
    );

    boot_log("exit boot services begin");
    trace_marker(b'A');
    let st = match table::system_table_raw() {
        Some(st) => st,
        None => return Status::ABORTED,
    };
    let st = unsafe { st.as_ref() };
    if st.boot_services.is_null() {
        return Status::ABORTED;
    }
    let bt = unsafe { &*st.boot_services };
    let image = boot::image_handle().as_ptr();

    let exit_map = unsafe { &mut *EXIT_MAP_BUFFER.0.get() };
    let mut final_map_size = 0usize;
    let mut final_desc_size = 0usize;
    let mut exited = false;

    for _ in 0..2 {
        let mut map_size = exit_map.len();
        let mut map_key = 0usize;
        let mut desc_size = 0usize;
        let mut desc_version = 0u32;

        let get_map_status = unsafe {
            (bt.get_memory_map)(
                &mut map_size,
                exit_map.as_mut_ptr().cast(),
                &mut map_key,
                &mut desc_size,
                &mut desc_version,
            )
        };
        if !get_map_status.is_success() {
            return get_map_status;
        }

        let exit_status = unsafe { (bt.exit_boot_services)(image, map_key) };
        if exit_status.is_success() {
            final_map_size = map_size;
            final_desc_size = desc_size;
            exited = true;
            break;
        }
    }
    if !exited || final_desc_size == 0 || final_map_size > exit_map.len() {
        return Status::ABORTED;
    }

    // No UEFI services available after ExitBootServices; use raw COM1 writes.
    serial_write_str("\n[boot-post-exit] fb_base=");
    serial_write_hex_u64(unsafe { (*boot_info_ptr).framebuffer.base });
    serial_write_str(" stride=");
    serial_write_hex_usize(unsafe { (*boot_info_ptr).framebuffer.stride });
    serial_write_str(" bpp=");
    serial_write_hex_usize(unsafe { (*boot_info_ptr).framebuffer.bpp });
    serial_write_str(" entry=");
    serial_write_hex_u64(entry);
    serial_write_str(" boot_info=");
    serial_write_hex_u64(boot_info_ptr as u64);
    serial_write_str("\n");

    trace_marker(b'B');
    let final_entry_count = final_map_size / final_desc_size;
    if final_entry_count > MAX_ENTRIES {
        loop {
            core::hint::spin_loop();
        }
    }

    if final_entry_count != 0 {
        let dst = entries_storage.as_ptr() as *mut efi_main::memorymap::MemoryRegion;
        for idx in 0..final_entry_count {
            let offset = idx * final_desc_size;
            let desc_ptr = unsafe {
                exit_map
                    .as_ptr()
                    .add(offset)
                    .cast::<uefi::mem::memory_map::MemoryDescriptor>()
            };
            let desc = unsafe { core::ptr::read_unaligned(desc_ptr) };
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

    trace_marker(b'C');

    unsafe {
        trace_marker(b'D');
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
