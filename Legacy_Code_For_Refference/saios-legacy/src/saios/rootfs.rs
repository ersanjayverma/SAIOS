//! Canonical root filesystem layout and seed content.
//!
//! This module is the single source of truth for the authoritative SAIOS
//! roots, Linux-compatibility roots, and the initial file set written by the
//! installer or the temporary recovery root used by install/update media.

use super::identity;
use crate::config;
use crate::version;
use alloc::format;
use alloc::vec;
use alloc::vec::Vec;

include!(concat!(env!("OUT_DIR"), "/saios_shell_elf.rs"));

pub const AUTHORITATIVE_ROOTS: &[&str] = &[
    "/system",
    "/users",
    "/apps",
    "/runtime",
    "/security",
    "/services",
    "/devices",
    "/network",
    "/data",
    "/logs",
];

pub const AUTHORITATIVE_DIRS: &[&str] = &[
    "/system/config",
    "/system/config/system",
    "/system/config/network",
    "/system/config/display",
    "/system/config/security",
    "/system/config/packages",
    "/system/config/ai",
    "/system/users",
    "/system/auth",
    "/system/auth/acls",
    "/system/auth/capabilities",
    "/system/auth/services",
    "/system/services",
    "/system/compat",
    "/system/compat/windows",
    "/system/compat/windows/C",
    "/system/compat/windows/C/Windows",
    "/system/compat/windows/C/Windows/System32",
    "/system/compat/macos",
    "/system/compat/macos/private",
    "/users/root",
];

pub const COMPATIBILITY_ROOTS: &[&str] = &[
    "/etc", "/bin", "/sbin", "/usr", "/lib", "/lib64", "/home", "/var", "/tmp", "/proc",
];

pub const COMPATIBILITY_DIRS: &[&str] = &[
    "/etc/apt",
    "/etc/apt/sources.list.d",
    "/etc/apt/trusted.gpg.d",
    "/etc/ld.so.conf.d",
    "/etc/ssl",
    "/etc/ssl/certs",
    "/usr/bin",
    "/usr/sbin",
    "/usr/lib",
    "/usr/lib/x86_64-linux-gnu",
    "/usr/local",
    "/usr/local/bin",
    "/usr/share",
    "/usr/share/man",
    "/usr/share/man/man1",
    "/usr/share/doc",
    "/usr/share/locale",
    "/var/lib",
    "/var/lib/dpkg",
    "/var/lib/dpkg/info",
    "/var/lib/dpkg/updates",
    "/var/lib/dpkg/alternatives",
    "/var/lib/apt",
    "/var/lib/apt/lists",
    "/var/lib/apt/lists/partial",
    "/var/cache",
    "/var/cache/apt",
    "/var/cache/apt/archives",
    "/var/cache/apt/archives/partial",
    "/var/log",
    "/var/tmp",
    "/home/root",
    "/lib/x86_64-linux-gnu",
];

pub const LEGACY_ROOTS: &[&str] = &["/dev", "/run", "/sys", "/mnt", "/media", "/opt", "/srv"];

pub const WINDOWS_COMPAT_DIRS: &[&str] = &[
    "/system/compat/windows/C/Users",
    "/system/compat/windows/C/Program Files",
    "/system/compat/windows/C/ProgramData",
    "/system/compat/windows/C/Temp",
    "/system/compat/windows/C/Devices",
];

pub const MACOS_COMPAT_DIRS: &[&str] = &[
    "/system/compat/macos/Applications",
    "/system/compat/macos/System",
    "/system/compat/macos/Users",
    "/system/compat/macos/private/tmp",
];

pub fn initial_files() -> Vec<(&'static str, Vec<u8>)> {
    let saios_config = format!(
        "# SAIOS configuration\nversion={}\nai_provider=ollama\nai_host=10.0.2.2:11434\nai_model=llama3\nollama_host=10.0.2.2\nollama_port=11434\ntogether_model=openai/gpt-oss-120b\nhostname=saios\ndns=8.8.8.8,1.1.1.1\napt_mirror=deb.debian.org\n",
        version::SAIOS_VERSION,
    )
    .into_bytes();

    let network_json =
        b"{\n  \"hostname\": \"saios\",\n  \"dns\": [\"8.8.8.8\", \"1.1.1.1\"]\n}\n".to_vec();
    let packages_json =
        b"{\n  \"mirror\": \"deb.debian.org\",\n  \"suite\": \"bookworm\",\n  \"installed\": []\n}\n".to_vec();
    let auth_json = b"{\n  \"bhb_required\": false,\n  \"primary_user\": null\n}\n".to_vec();
    let sources = b"deb http://deb.debian.org/debian bookworm main contrib non-free\n\
deb http://security.debian.org/debian-security bookworm-security main\n\
deb http://deb.debian.org/debian bookworm-updates main\n"
        .to_vec();
    let os_release = format!(
        "PRETTY_NAME=\"{} {} (based on {})\"\n\
NAME=\"{}\"\nVERSION_ID=\"{}\"\nID=saios\nID_LIKE=debian\n",
        version::SAIOS_NAME,
        version::SAIOS_VERSION,
        version::SAIOS_ABI_NAME,
        version::SAIOS_NAME,
        version::SAIOS_VERSION,
    )
    .into_bytes();
    let passwd = b"root:x:0:0:root:/users/root:/bin/bash\n\
daemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin\n"
        .to_vec();
    let group = b"root:x:0:\ndaemon:x:1:\n".to_vec();
    let shadow = b"root:*:19700:0:99999:7:::\n".to_vec();
    let shell_image = SAIOS_SHELL_ELF.to_vec();

    vec![
        (config::CANONICAL_CONFIG_PATH, saios_config.clone()),
        (config::COMPAT_CONFIG_PATH, saios_config),
        ("/system/config/network/network.json", network_json),
        ("/system/config/packages/packages.json", packages_json),
        ("/system/config/security/auth.json", auth_json),
        ("/etc/apt/sources.list", sources),
        ("/etc/hostname", b"saios\n".to_vec()),
        ("/etc/os-release", os_release),
        (identity::NATIVE_PASSWD, passwd.clone()),
        (identity::NATIVE_GROUP, group.clone()),
        (identity::NATIVE_SHADOW, shadow.clone()),
        (identity::COMPAT_PASSWD, passwd),
        (identity::COMPAT_GROUP, group),
        (identity::COMPAT_SHADOW, shadow),
        (
            "/etc/nsswitch.conf",
            b"passwd: files\ngroup: files\nhosts: files dns\nnetworks: files\n".to_vec(),
        ),
        (
            "/etc/resolv.conf",
            b"nameserver 8.8.8.8\nnameserver 1.1.1.1\n".to_vec(),
        ),
        (
            "/etc/ld.so.conf",
            b"include /etc/ld.so.conf.d/*.conf\n".to_vec(),
        ),
        ("/etc/ld.so.cache", Vec::new()),
        ("/bin/sh", shell_image.clone()),
        ("/bin/bash", shell_image),
        ("/etc/shells", b"/bin/sh\n/bin/bash\n".to_vec()),
    ]
}
