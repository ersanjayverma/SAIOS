use core::sync::atomic::{AtomicBool, Ordering};

use crate::vfs;

const PROFILE: &str = "saios-base";
const MANIFEST_PATH: &str = "/boot/package.manifest";

const ROOT_DIRS: &[&str] = &["/boot", "/bin", "/etc", "/home", "/proc", "/dev", "/tmp", "/usr"];

const BIN_ENTRIES: &[&str] = &[
    "hello",
    "calc",
    "stress",
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

    ensure_file(MANIFEST_PATH)?;
    vfs::write(MANIFEST_PATH, text.as_bytes())
}

pub fn mount_default() -> Result<(), &'static str> {
    for d in ROOT_DIRS {
        ensure_dir(d)?;
    }

    for b in BIN_ENTRIES {
        let path = alloc::format!("/bin/{}", b);
        ensure_file(path.as_str())?;
    }

    ensure_file("/etc/profile")?;
    ensure_file("/etc/hostname")?;
    let _ = vfs::write("/etc/hostname", b"saios\n");

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