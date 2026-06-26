#![no_std]
#![no_main]
extern crate alloc;
use alloc::vec;
use uefi::println;
use uefi::*;
use uefi::proto::media::file::{File,FileType, FileMode, FileAttribute};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::proto::loaded_image::LoadedImage;
#[entry]
fn main() -> Status {
    uefi::helpers::init().unwrap();

    println!("================================");
    println!("        SAIOS Bootloader"        );
    println!("================================");
    let boot_info = efi_main::initialize_boot_info();
    println!("========================================");
    println!("          SAIOS BOOT INFORMATION        ");
    println!("========================================");
    
    // Print metadata fields explicitly for validation
    println!("Magic Check:   0x{:X} (Expected: 0x{:X})", boot_info.magic, efi_main::SAIOS_BOOT_MAGIC); 
    println!("Boot Version:  {}.{}", boot_info.version >> 16, boot_info.version & 0xFFFF);
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
    

    let ptr = self::load_seed("\\SAIOS\\seed.elf").unwrap();   

    unsafe {
     let _ =  uefi::boot::exit_boot_services(None);
    };
    
    self::jump_to_seed(boot_info,ptr.entry_point);
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
   println!(" Extract the panic message and file/line location");
    let message = info.message();
    let location = info.location();

    if let Some(loc) = location {
        let _ = println!("Panic occurred at {}:{}:{}", loc.file(), loc.line(), message);
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
    let mut root = fs.open_volume().map_err(|_| uefi::Error::from(uefi::Status::LOAD_ERROR))?;

    println!("Convert &str → CString16");
    let cstr_path = CString16::try_from(path).map_err(|_| uefi::Error::from(uefi::Status::LOAD_ERROR))?;

    println!("Open file");
    let file = root.open(&cstr_path, FileMode::Read, FileAttribute::empty())
        .map_err(|_| uefi::Error::from(uefi::Status::NOT_FOUND))?;

    println!("Match on FileType::Regular");
    let mut regular = match file.into_type().map_err(|_| uefi::Error::from(uefi::Status::LOAD_ERROR))? {
        FileType::Regular(f) => f,
        _ => return Err(uefi::Error::from(uefi::Status::LOAD_ERROR)),
    };

    println!("Read file into buffer");
    // inside strict UEFI configurations. Consider heap allocation if it triple faults.
let mut buffer = vec![0u8; 1024 * 1024 * 10]; // 1 MB buffer
    let size = regular.read(&mut buffer).map_err(|_| uefi::Error::from(uefi::Status::LOAD_ERROR))?;

    println!("Parse ELF header");
    let entry_point = parse_elf_header(&buffer[..size]);

    Ok(Loader { entry_point })
}

pub fn parse_elf_header(elf_data: &[u8]) -> u64 {
    if elf_data.len() < 0x20 {
        panic!("Not enough data for ELF header");
    }
    u64::from_le_bytes(
        elf_data[0x18..0x20]
            .try_into()
            .expect("Failed to parse entry point"),
    )
}