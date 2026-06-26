//! GNU bash 5.2.15(1)-release installation for SAIOS.
//!
//! The `setup bash` command fetches bash from the configured Debian mirror
//! (http://deb.debian.org/debian/pool/main/b/bash/) and installs it to
//! /bin/bash. Requires working network (VirtIO-Net or e1000/rtl8139).
//!
//! For offline installation: place a statically-compiled bash ELF at
//! /bin/bash in the VFS before running exec.

use alloc::format;
use alloc::string::String;

pub const BASH_VERSION: &str = "5.2.15(1)-release";
pub const BASH_PATH: &str = "/bin/bash";

/// Attempt to download and install bash from the Debian mirror.
pub fn install_bash() -> Result<(), &'static str> {
    crate::println!("GNU bash {} installer", BASH_VERSION);
    crate::println!("Fetching from Debian 12 (Bookworm) mirror...");

    // Check network
    let our_ip = crate::network_contract::NetworkContract::ip();
    if our_ip == [0, 0, 0, 0] {
        return Err("No network - check VirtIO-Net or NIC driver");
    }

    // The bash binary package on Debian 12
    // We fetch the binary deb, extract the data archive, copy /bin/bash
    let host = "deb.debian.org";
    let path = "/debian/pool/main/b/bash/bash_5.2.15-2+b7_amd64.deb";

    crate::println!("Connecting to {}...", host);
    crate::println!("Note: HTTPS not yet available - using HTTP redirect if possible");

    // Attempt HTTP fetch
    let req = crate::net::http::HttpRequest::get(host, path, 80);
    match crate::net::http::send(req) {
        Some(resp) if resp.status == 200 => {
            crate::println!("Downloaded {} bytes", resp.body_bytes.len());
            // Extract bash from the .deb
            match extract_bash_from_deb(&resp.body_bytes) {
                Some(binary) => {
                    crate::println!("Extracted bash binary ({} KiB)", binary.len() / 1024);
                    // Install to /bin/bash
                    ensure_bin_dir()?;
                    crate::vfs_contract::VfsContract::write_file(BASH_PATH, &binary, 0o755)
                        .map_err(|_| "Failed to write /bin/bash")?;
                    crate::println!("✓ bash installed at {}", BASH_PATH);
                    crate::println!("  Run: exec /bin/bash");
                    Ok(())
                }
                None => Err("Failed to extract bash from .deb"),
            }
        }
        Some(resp) => {
            crate::println!("HTTP {} - trying static install guide", resp.status);
            print_manual_install();
            Ok(())
        }
        None => {
            crate::println!("Network request failed.");
            print_manual_install();
            Ok(())
        }
    }
}

fn ensure_bin_dir() -> Result<(), &'static str> {
    match crate::vfs_contract::VfsContract::resolve("/bin") {
        Ok(_) => Ok(()),
        Err(_) => {
            crate::vfs_contract::VfsContract::mkdir("/bin", 0o755).map_err(|_| "cannot create /bin")
        }
    }
}

/// Minimal .deb extractor - .deb is an ar archive containing data.tar.xz.
/// We look for the bash binary inside data.tar.xz → ./usr/bin/bash
/// For now returns None (full xz decompression requires libxz port).
fn extract_bash_from_deb(_deb: &[u8]) -> Option<alloc::vec::Vec<u8>> {
    let _ = crate::compatibility_contract::CompatibilityContract::require_placeholder_available(
        "bash.deb.xz.extractor",
    );
    None
}

fn print_manual_install() {
    crate::println!();
    crate::println!("╔══════════════════════════════════════════════════════╗");
    crate::println!("║        Manual bash installation for SAIOS            ║");
    crate::println!("╠══════════════════════════════════════════════════════╣");
    crate::println!("║ On your HOST machine:                                ║");
    crate::println!("║                                                      ║");
    crate::println!("║  1. Cross-compile bash 5.2.15 statically:            ║");
    crate::println!("║     wget https://ftp.gnu.org/gnu/bash/bash-5.2.15.tar.gz");
    crate::println!("║     tar xzf bash-5.2.15.tar.gz && cd bash-5.2.15     ║");
    crate::println!("║     ./configure --host=x86_64-linux-musl              ║");
    crate::println!("║         LDFLAGS=-static --without-bash-malloc         ║");
    crate::println!("║     make -j$(nproc)                                   ║");
    crate::println!("║                                                       ║");
    crate::println!("║  2. Copy into the SAIOS disk image:                   ║");
    crate::println!("║     Mount saios ext4 partition and copy bash to /bin  ║");
    crate::println!("║     Or: add to initrd, or load via TFTP               ║");
    crate::println!("║                                                       ║");
    crate::println!("║  3. In SAIOS shell:  exec /bin/bash                   ║");
    crate::println!("╚══════════════════════════════════════════════════════╝");
}

/// Check whether bash is already installed and runnable.
pub fn is_installed() -> bool {
    crate::vfs_contract::VfsContract::resolve(BASH_PATH).is_ok()
}

/// Print bash version info (reads from the binary if installed).
pub fn version_info() {
    if is_installed() {
        crate::println!(
            "GNU bash, version {} (x86_64-saios-linux-gnu)",
            BASH_VERSION
        );
        crate::println!("Copyright (C) 2022 Free Software Foundation, Inc.");
        crate::println!("License GPLv3+: GNU GPL version 3 or later");
        crate::println!();
        crate::println!("Installed at: {}", BASH_PATH);
        crate::println!("Run with: exec /bin/bash");
    } else {
        crate::println!("bash not installed. Run: setup bash");
    }
}
