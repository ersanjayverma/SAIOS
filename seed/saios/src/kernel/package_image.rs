use core::sync::atomic::{AtomicBool, Ordering};

use alloc::vec;

use crate::vfs;

const PROFILE: &str = "saios-base";
const MANIFEST_PATH: &str = "/boot/package.manifest";

const ROOT_DIRS: &[&str] = &[
    "/boot",
    "/bin",
    "/etc",
    "/home",
    "/proc",
    "/dev",
    "/tmp",
    "/usr",
    "/lib",
    "/system",
];

const SHARED_LIB_ENTRIES: &[&str] = &[
    "ld-saios.so",
    "libc.so",
    "libm.so",
    "libshell.so",
    "libui.so",
];

const BIN_ENTRIES: &[&str] = &[
    "hello",
    "calc",
    "editor",
    "shell",
    "ls",
    "cat",
    "cp",
    "mv",
    "rm",
    "mkdir",
    "ps",
    "kill",
    "top",
    "uname",
    "stress",
    "cc",
    "taskman",
];

static MOUNTED: AtomicBool = AtomicBool::new(false);

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
        write_binary(path.as_str(), b)?;
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

pub fn mount_default() -> Result<(), &'static str> {
    for d in ROOT_DIRS {
        ensure_dir(d)?;
    }

    seed_shared_libraries()?;
    seed_binaries()?;

    ensure_file("/etc/profile")?;
    ensure_file("/etc/hostname")?;
    let _ = vfs::write_path("/etc/hostname", b"saios\n");

    write_manifest()?;
    MOUNTED.store(true, Ordering::Release);
    Ok(())
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