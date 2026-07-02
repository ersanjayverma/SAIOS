use core::sync::atomic::{AtomicBool, Ordering};

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
    let mut text = alloc::string::String::new();
    text.push_str("SAIOS_BIN_V1\n");
    text.push_str("entry=");
    text.push_str(entry);
    text.push('\n');
    text.push_str("type=pie\n");
    text.push_str("preferred_base=0x00400000\n");
    text.push_str("dynamic=true\n");
    text.push_str("interp=/lib/ld-saios.so\n");

    let needed = if entry == "calc" {
        "libc.so,libm.so"
    } else if entry == "shell" {
        "libc.so,libshell.so"
    } else if entry == "editor" {
        "libc.so,libui.so"
    } else {
        "libc.so"
    };
    text.push_str("needed=");
    text.push_str(needed);
    text.push('\n');

    let required = if entry == "calc" {
        "malloc,free,printf,sin,cos"
    } else if entry == "shell" {
        "malloc,free,printf,shell_init,shell_run"
    } else if entry == "editor" {
        "malloc,free,printf,ui_init,text_edit"
    } else {
        "malloc,free,printf"
    };
    text.push_str("required=");
    text.push_str(required);
    text.push('\n');
    vfs::write_path(path, text.as_bytes())
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