#![no_std]
#![no_main]
extern crate alloc;
use alloc::vec;
use uefi::boot::{AllocateType, MemoryType};
use uefi::println;
use uefi::proto::loaded_image::LoadedImage;
use uefi::proto::media::file::{File, FileAttribute, FileMode, FileType};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::*;

#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();

    println!("================================");
    println!("        SAIOS Bootloader");
    println!("================================");
    let seed_path = "\\EFI\\SAIOS\\seed.elf";
    let loader = load_seed(seed_path).unwrap();
    for i in 0..loader.elf_header.phnum {
        let offset =
            loader.elf_header.phoff as usize + i as usize * loader.elf_header.phentsize as usize;
        let ph = efi_main::load_program_header(&loader.image, offset).unwrap();

        if ph.p_type == efi_main::ProgramHeaderType::Load as u32 {
            println!(
                "Loading segment {}: offset {:#x}, vaddr {:#x}, filesz {:#x}, memsz {:#x}",
                i, ph.p_offset, ph.p_vaddr, ph.p_filesz, ph.p_memsz
            );
            let addr = ph.p_paddr;
            let pages = (ph.p_memsz as usize + 4095) / 4096;
            let allocated =
                boot::allocate_pages(AllocateType::Address(addr), MemoryType::LOADER_DATA, pages)
                    .unwrap();

            let src = &loader.image[ph.p_offset as usize..(ph.p_offset + ph.p_filesz) as usize];
            let dst = allocated.as_ptr();

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
    }

    let boot_info = efi_main::initialize_boot_info();
    println!("========================================");
    println!("          SAIOS BOOT INFORMATION        ");
    println!("========================================");

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
    println!("{:#?}", boot_info.memorymap);
    println!("{:#?}", boot_info.acpi);
    println!("{:#?}", boot_info.smbios);
    println!("{:#?}", boot_info.cpu);
    println!("{:#?}", boot_info.firmware);

    println!("========================================");

    unsafe {
        let _ = uefi::boot::exit_boot_services(None);
    };

    self::jump_to_seed(boot_info, loader.entry_point);
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
pub fn jump_to_seed(boot_info: efi_main::SaiosBootInfo, entry_point: u64) -> ! {
    println!("Jump to kernel entry point");
    let seed_entry_point = entry_point as *const ();
    let seed_fn: extern "C" fn(&efi_main::SaiosBootInfo) -> ! =
        unsafe { core::mem::transmute(seed_entry_point) };

    seed_fn(&boot_info)
}
pub struct Loader {
    pub image: alloc::vec::Vec<u8>,
    pub entry_point: u64,
    pub elf_header: efi_main::Elf64Header,
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

    let mut buffer = vec![0u8; 1024 * 1024 * 10]; // 1 MB buffer
    let size = regular
        .read(&mut buffer)
        .map_err(|_| uefi::Error::from(uefi::Status::LOAD_ERROR))?;

    println!("Parse ELF header");
    Ok(Loader {
        entry_point: efi_main::parse_elf_header(&buffer[..size]),
        elf_header: efi_main::load_elf64_header(&buffer[..size]).unwrap(),
        image: buffer[..size].to_vec(),
    })
}
