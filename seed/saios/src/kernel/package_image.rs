use core::sync::atomic::{AtomicBool, Ordering};

use alloc::vec;
use alloc::vec::Vec;

use crate::vfs;

const PROFILE: &str = "saios-base";
const MANIFEST_PATH: &str = "/boot/package.manifest";

const ROOT_DIRS: &[&str] = &[
    "/boot", "/bin", "/etc", "/home", "/proc", "/dev", "/tmp", "/usr", "/lib", "/system",
];

const SHARED_LIB_ENTRIES: &[&str] = &[
    "ld-saios.so",
    "libc.so",
    "libm.so",
    "libshell.so",
    "libui.so",
];

const BIN_ENTRIES: &[&str] = &[
    "hello", "calc", "editor", "shell", "ls", "cat", "cp", "mv", "rm", "mkdir", "ps", "kill",
    "top", "uname", "stress", "cc", "taskman", "diskpart", "busybox", "r3probe",
];

const EMBEDDED_BUSYBOX: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../busybox"));
const USER_MODE_PROBE_PATH: &str = "/bin/r3probe";

static MOUNTED: AtomicBool = AtomicBool::new(false);

const ELF_PT_LOAD: u32 = 1;
const ELF_PT_DYNAMIC: u32 = 2;
const ELF_PT_INTERP: u32 = 3;

#[derive(Copy, Clone, Debug)]
pub struct PackageImageStatus {
    pub mounted: bool,
    pub profile: &'static str,
    pub manifest: &'static str,
    pub roots: usize,
    pub bins: usize,
}

fn ensure_dir(path: &str) -> Result<(), &'static str> {
    match vfs::mkdir(path) {
        Ok(()) => Ok(()),
        Err("already exists") => Ok(()),
        Err(e) => Err(e),
    }
}

fn ensure_file(path: &str) -> Result<(), &'static str> {
    match vfs::touch(path) {
        Ok(()) => Ok(()),
        Err("already exists") => Ok(()),
        Err(e) => Err(e),
    }
}

fn write_manifest() -> Result<(), &'static str> {
    let mut text = alloc::string::String::new();
    text.push_str("profile=");
    text.push_str(PROFILE);
    text.push('\n');

    for d in ROOT_DIRS {
        text.push_str("dir=");
        text.push_str(d);
        text.push('\n');
    }

    for b in BIN_ENTRIES {
        text.push_str("bin=/bin/");
        text.push_str(b);
        text.push('\n');
    }

    for so in SHARED_LIB_ENTRIES {
        text.push_str("lib=/lib/");
        text.push_str(so);
        text.push('\n');
    }

    ensure_file(MANIFEST_PATH)?;
    vfs::write_path(MANIFEST_PATH, text.as_bytes())
}

fn write_binary(path: &str, entry: &str) -> Result<(), &'static str> {
    let _ = entry;
    const ELF_HEADER_SIZE: usize = 64;
    const PROGRAM_HEADER_SIZE: usize = 56;
    const IMAGE_SIZE: usize = ELF_HEADER_SIZE + PROGRAM_HEADER_SIZE;

    let mut elf = vec![0u8; IMAGE_SIZE];

    // e_ident
    elf[0] = 0x7F;
    elf[1] = b'E';
    elf[2] = b'L';
    elf[3] = b'F';
    elf[4] = 2; // ELFCLASS64
    elf[5] = 1; // little-endian
    elf[6] = 1; // current version

    fn put16(buf: &mut [u8], off: usize, value: u16) {
        let b = value.to_le_bytes();
        buf[off..off + 2].copy_from_slice(&b);
    }
    fn put32(buf: &mut [u8], off: usize, value: u32) {
        let b = value.to_le_bytes();
        buf[off..off + 4].copy_from_slice(&b);
    }
    fn put64(buf: &mut [u8], off: usize, value: u64) {
        let b = value.to_le_bytes();
        buf[off..off + 8].copy_from_slice(&b);
    }

    // ELF header fields.
    put16(&mut elf, 16, 2); // ET_EXEC
    put16(&mut elf, 18, 62); // EM_X86_64
    put32(&mut elf, 20, 1); // EV_CURRENT
    put64(&mut elf, 24, 0x0040_1000); // e_entry
    put64(&mut elf, 32, ELF_HEADER_SIZE as u64); // e_phoff
    put64(&mut elf, 40, 0); // e_shoff
    put32(&mut elf, 48, 0); // e_flags
    put16(&mut elf, 52, ELF_HEADER_SIZE as u16); // e_ehsize
    put16(&mut elf, 54, PROGRAM_HEADER_SIZE as u16); // e_phentsize
    put16(&mut elf, 56, 1); // e_phnum
    put16(&mut elf, 58, 0); // e_shentsize
    put16(&mut elf, 60, 0); // e_shnum
    put16(&mut elf, 62, 0); // e_shstrndx

    let ph = ELF_HEADER_SIZE;
    put32(&mut elf, ph, 1); // PT_LOAD
    put32(&mut elf, ph + 4, 0x5); // PF_R | PF_X
    put64(&mut elf, ph + 8, 0); // p_offset
    put64(&mut elf, ph + 16, 0x0040_0000); // p_vaddr
    put64(&mut elf, ph + 24, 0x0040_0000); // p_paddr
    put64(&mut elf, ph + 32, IMAGE_SIZE as u64); // p_filesz
    put64(&mut elf, ph + 40, IMAGE_SIZE as u64); // p_memsz
    put64(&mut elf, ph + 48, 0x1000); // p_align

    vfs::write_path(path, elf.as_slice())
}

fn write_user_mode_probe_binary(path: &str) -> Result<(), &'static str> {
    const ELF_HEADER_SIZE: usize = 64;
    const PROGRAM_HEADER_SIZE: usize = 56;
    const CODE_OFFSET: usize = 0x80;
    const IMAGE_SIZE: usize = CODE_OFFSET + 9;
    const ENTRY_VADDR: u64 = 0x0040_0000 + CODE_OFFSET as u64;
    const LINUX_EXIT_SYSCALL: u32 = 60;
    const CODE: [u8; 9] = [
        0xB8,
        LINUX_EXIT_SYSCALL as u8,
        0x00,
        0x00,
        0x00,
        0x31,
        0xFF,
        0x0F,
        0x05,
    ];

    let mut elf = vec![0u8; IMAGE_SIZE];

    elf[0] = 0x7F;
    elf[1] = b'E';
    elf[2] = b'L';
    elf[3] = b'F';
    elf[4] = 2;
    elf[5] = 1;
    elf[6] = 1;

    fn put16(buf: &mut [u8], off: usize, value: u16) {
        let b = value.to_le_bytes();
        buf[off..off + 2].copy_from_slice(&b);
    }
    fn put32(buf: &mut [u8], off: usize, value: u32) {
        let b = value.to_le_bytes();
        buf[off..off + 4].copy_from_slice(&b);
    }
    fn put64(buf: &mut [u8], off: usize, value: u64) {
        let b = value.to_le_bytes();
        buf[off..off + 8].copy_from_slice(&b);
    }

    put16(&mut elf, 16, 2);
    put16(&mut elf, 18, 62);
    put32(&mut elf, 20, 1);
    put64(&mut elf, 24, ENTRY_VADDR);
    put64(&mut elf, 32, ELF_HEADER_SIZE as u64);
    put32(&mut elf, 48, 0);
    put16(&mut elf, 52, ELF_HEADER_SIZE as u16);
    put16(&mut elf, 54, PROGRAM_HEADER_SIZE as u16);
    put16(&mut elf, 56, 1);

    let ph = ELF_HEADER_SIZE;
    put32(&mut elf, ph, 1);
    put32(&mut elf, ph + 4, 0x5);
    put64(&mut elf, ph + 8, 0);
    put64(&mut elf, ph + 16, 0x0040_0000);
    put64(&mut elf, ph + 24, 0x0040_0000);
    put64(&mut elf, ph + 32, IMAGE_SIZE as u64);
    put64(&mut elf, ph + 40, IMAGE_SIZE as u64);
    put64(&mut elf, ph + 48, 0x1000);

    elf[CODE_OFFSET..CODE_OFFSET + CODE.len()].copy_from_slice(&CODE);
    vfs::write_path(path, elf.as_slice())
}

fn write_shared_library(path: &str, soname: &str, exports: &str) -> Result<(), &'static str> {
    let mut text = alloc::string::String::new();
    text.push_str("SAIOS_SO_V1\n");
    text.push_str("soname=");
    text.push_str(soname);
    text.push('\n');
    text.push_str("exports=");
    text.push_str(exports);
    text.push('\n');
    vfs::write_path(path, text.as_bytes())
}

fn seed_binaries() -> Result<(), &'static str> {
    for b in BIN_ENTRIES {
        let path = alloc::format!("/bin/{}", b);
        ensure_file(path.as_str())?;
        if *b == "r3probe" {
            write_user_mode_probe_binary(path.as_str())?;
        } else {
            write_binary(path.as_str(), b)?;
        }
    }

    Ok(())
}

fn seed_shared_libraries() -> Result<(), &'static str> {
    for so in SHARED_LIB_ENTRIES {
        let path = alloc::format!("/lib/{}", so);
        ensure_file(path.as_str())?;

        if *so == "ld-saios.so" {
            write_shared_library(path.as_str(), so, "dl_open,dl_sym,dl_close")?;
        } else if *so == "libc.so" {
            write_shared_library(path.as_str(), so, "malloc,free,printf,puts,exit")?;
        } else if *so == "libm.so" {
            write_shared_library(path.as_str(), so, "sin,cos,tan,sqrt")?;
        } else if *so == "libshell.so" {
            write_shared_library(path.as_str(), so, "shell_init,shell_run,shell_prompt")?;
        } else if *so == "libui.so" {
            write_shared_library(path.as_str(), so, "ui_init,ui_draw,text_edit")?;
        }
    }

    Ok(())
}

fn seed_busybox_from_kernel_image() -> Result<(), &'static str> {
    verify_busybox_static_elf(EMBEDDED_BUSYBOX)?;
    ensure_file("/bin/busybox")?;
    vfs::write_path("/bin/busybox", EMBEDDED_BUSYBOX)
}

fn read_u16_le(data: &[u8], off: usize) -> Option<u16> {
    let bytes = data.get(off..off + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn read_u32_le(data: &[u8], off: usize) -> Option<u32> {
    let bytes = data.get(off..off + 4)?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn read_u64_le(data: &[u8], off: usize) -> Option<u64> {
    let bytes = data.get(off..off + 8)?;
    Some(u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]))
}

fn verify_busybox_static_elf(image: &[u8]) -> Result<(), &'static str> {
    if image.len() < 64 {
        return Err("busybox: invalid ELF image");
    }
    if image[0] != 0x7F || image[1] != b'E' || image[2] != b'L' || image[3] != b'F' {
        return Err("busybox: not an ELF binary");
    }
    if image[4] != 2 || image[5] != 1 {
        return Err("busybox: unsupported ELF class or endianness");
    }

    let phoff = read_u64_le(image, 32).ok_or("busybox: malformed ELF header")? as usize;
    let phentsize = read_u16_le(image, 54).ok_or("busybox: malformed ELF header")? as usize;
    let phnum = read_u16_le(image, 56).ok_or("busybox: malformed ELF header")? as usize;

    if phentsize < 56 || phnum == 0 {
        return Err("busybox: malformed program headers");
    }

    let table_bytes = phentsize
        .checked_mul(phnum)
        .ok_or("busybox: malformed program headers")?;
    let ph_end = phoff
        .checked_add(table_bytes)
        .ok_or("busybox: malformed program headers")?;
    if ph_end > image.len() {
        return Err("busybox: truncated program headers");
    }

    let mut has_load_segment = false;
    for idx in 0..phnum {
        let off = phoff + idx * phentsize;
        let p_type = read_u32_le(image, off).ok_or("busybox: malformed program header")?;
        let p_filesz = read_u64_le(image, off + 32).ok_or("busybox: malformed program header")?;

        if p_type == ELF_PT_LOAD {
            has_load_segment = true;
        }
        if p_type == ELF_PT_INTERP && p_filesz > 0 {
            return Err("busybox: dynamically linked (PT_INTERP present)");
        }
        if p_type == ELF_PT_DYNAMIC && p_filesz > 0 {
            return Err("busybox: dynamically linked (PT_DYNAMIC present)");
        }
    }

    if !has_load_segment {
        return Err("busybox: no loadable segment");
    }

    Ok(())
}

fn busybox_source_candidates() -> Vec<alloc::string::String> {
    let mut out = Vec::new();
    for volume in crate::driver::storage::volumes_cached() {
        if volume.name == "tmpfs" {
            continue;
        }
        let Some(mountpoint) = volume.mounted_at else {
            continue;
        };
        let root = mountpoint.trim_end_matches('/');
        out.push(alloc::format!("{}/busybox", root));
        out.push(alloc::format!("{}/bin/busybox", root));
        out.push(alloc::format!("{}/usr/bin/busybox", root));
    }
    out
}

pub fn install_busybox_from_storage_to_tmpfs() -> Result<bool, &'static str> {
    for candidate in busybox_source_candidates() {
        if crate::driver::storage::mounted_volume_for_path_cached(candidate.as_str()).is_none() {
            continue;
        }

        let Ok(image) = vfs::read_path(candidate.as_str()) else {
            continue;
        };

        verify_busybox_static_elf(image.as_slice())?;
        ensure_file("/bin/busybox")?;
        vfs::write_path("/bin/busybox", image.as_slice())?;
        return Ok(true);
    }

    Ok(false)
}

fn mount_default_inner() -> Result<(), &'static str> {
    for d in ROOT_DIRS {
        ensure_dir(d)?;
    }

    seed_shared_libraries()?;
    seed_binaries()?;
    seed_busybox_from_kernel_image()?;
    ensure_file(USER_MODE_PROBE_PATH)?;
    write_user_mode_probe_binary(USER_MODE_PROBE_PATH)?;

    ensure_file("/etc/profile")?;
    ensure_file("/etc/hostname")?;
    let _ = vfs::write_path("/etc/hostname", b"saios\n");

    write_manifest()?;
    MOUNTED.store(true, Ordering::Release);
    Ok(())
}

pub fn mount_default() -> Result<(), &'static str> {
    let previous_logging = vfs::set_event_logging_enabled(false);
    let result = mount_default_inner();
    let _ = vfs::set_event_logging_enabled(previous_logging);
    result
}

pub fn status() -> PackageImageStatus {
    PackageImageStatus {
        mounted: MOUNTED.load(Ordering::Acquire),
        profile: PROFILE,
        manifest: MANIFEST_PATH,
        roots: ROOT_DIRS.len(),
        bins: BIN_ENTRIES.len(),
    }
}
