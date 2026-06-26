//! dpkg — Debian package installer for SAIOS.
//!
//! Installs a .deb package:
//!   1. Parse the ar archive (debian-binary, control.tar.*, data.tar.*)
//!   2. Decompress control.tar.* → parse the `control` file (Package, Version, Depends...)
//!   3. Run `preinst` maintainer script if present
//!   4. Decompress data.tar.* → extract all files to /
//!   5. Run `postinst` maintainer script if present
//!   6. Update /var/lib/dpkg/status
//!   7. Write /var/lib/dpkg/info/<package>.list (installed file list)
//!
//! Package states tracked in /var/lib/dpkg/status:
//!   install ok installed   — fully installed
//!   install ok half-installed — interrupted mid-install
//!   deinstall ok config-files — removed but config files remain

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Parsed fields from a `control` file.
#[derive(Debug, Clone, Default)]
pub struct ControlInfo {
    pub package: String,
    pub version: String,
    pub arch: String,
    pub description: String,
    pub installed_size: u64,
    pub depends: Vec<String>,
    pub pre_depends: Vec<String>,
    pub recommends: Vec<String>,
    pub suggests: Vec<String>,
    pub maintainer: String,
    pub section: String,
    pub priority: String,
}

impl ControlInfo {
    /// Parse a `control` file (RFC-2822-like key: value stanzas).
    pub fn parse(text: &str) -> Self {
        let mut info = ControlInfo::default();
        let mut cur_key = String::new();
        let mut cur_val = String::new();

        let flush = |key: &str, val: &str, info: &mut ControlInfo| {
            let v = val.trim().to_string();
            match key {
                "Package" => info.package = v,
                "Version" => info.version = v,
                "Architecture" => info.arch = v,
                "Description" => info.description = v.lines().next().unwrap_or("").to_string(),
                "Installed-Size" => info.installed_size = v.parse().unwrap_or(0),
                "Depends" => info.depends = parse_deps(&v),
                "Pre-Depends" => info.pre_depends = parse_deps(&v),
                "Recommends" => info.recommends = parse_deps(&v),
                "Suggests" => info.suggests = parse_deps(&v),
                "Maintainer" => info.maintainer = v,
                "Section" => info.section = v,
                "Priority" => info.priority = v,
                _ => {}
            }
        };

        for line in text.lines() {
            if line.starts_with(' ') || line.starts_with('\t') {
                // Continuation
                cur_val.push('\n');
                cur_val.push_str(line.trim());
            } else if let Some(colon) = line.find(':') {
                // New field — flush previous
                if !cur_key.is_empty() {
                    flush(&cur_key.clone(), &cur_val.clone(), &mut info);
                }
                cur_key = line[..colon].trim().to_string();
                cur_val = line[colon + 1..].trim().to_string();
            }
        }
        if !cur_key.is_empty() {
            flush(&cur_key, &cur_val, &mut info);
        }
        info
    }
}

fn parse_deps(s: &str) -> Vec<String> {
    s.split(',')
        .map(|d| d.split_whitespace().next().unwrap_or("").to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Install a .deb package from raw bytes.
///
/// Returns the `ControlInfo` of the installed package.
pub fn install(deb_data: &[u8]) -> Result<ControlInfo, &'static str> {
    crate::println!("[dpkg] parsing .deb archive...");

    // -- Step 1: Parse ar archive -------------------------------------------
    let entries = super::ar::parse(deb_data)?;

    // Verify debian-binary
    let db = super::ar::find(&entries, "debian-binary").ok_or("dpkg: missing debian-binary")?;
    let version_str = core::str::from_utf8(&db.data).unwrap_or("").trim();
    if !version_str.starts_with("2.") {
        return Err("dpkg: unsupported debian-binary version");
    }

    // -- Step 2: Extract and parse control ---------------------------------
    let control_entry = entries
        .iter()
        .find(|e| e.name.starts_with("control.tar"))
        .ok_or("dpkg: missing control.tar")?;

    let control_data = crate::compress::decompress(&control_entry.data)?;
    let control_tar = super::tar::parse(&control_data)?;

    let control_text = control_tar
        .iter()
        .find(|e| e.path == "control" || e.path == "./control")
        .and_then(|e| core::str::from_utf8(&e.data).ok())
        .unwrap_or("");

    let info = ControlInfo::parse(control_text);
    if info.package.is_empty() {
        return Err("dpkg: control file has no Package field");
    }

    crate::println!(
        "[dpkg] installing {} {} ({})",
        info.package,
        info.version,
        info.arch
    );

    // -- Step 3: Run preinst if present ------------------------------------
    if let Some(preinst) = control_tar.iter().find(|e| e.path.ends_with("preinst")) {
        run_maintainer_script("preinst", &preinst.data, &info.package);
    }

    // -- Step 4: Extract data.tar ------------------------------------------
    let data_entry = entries
        .iter()
        .find(|e| e.name.starts_with("data.tar"))
        .ok_or("dpkg: missing data.tar")?;

    crate::print!(
        "[dpkg] decompressing data.tar ({} KiB)...",
        data_entry.data.len() / 1024
    );
    let data_raw = crate::compress::decompress(&data_entry.data)?;
    crate::println!(" {} KiB uncompressed", data_raw.len() / 1024);

    crate::print!("[dpkg] extracting files...");
    let files = super::tar::extract_to_vfs(&data_raw, "/")?;
    crate::println!(" {} files installed", files.len());

    // -- Step 5: Run postinst if present -----------------------------------
    if let Some(postinst) = control_tar.iter().find(|e| e.path.ends_with("postinst")) {
        run_maintainer_script("postinst", &postinst.data, &info.package);
    }

    // -- Step 6: Update /var/lib/dpkg/status -------------------------------
    update_status(&info);

    // -- Step 7: Write /var/lib/dpkg/info/<package>.list -------------------
    write_file_list(&info.package, &files);

    // -- Step 8: Write maintainer scripts to /var/lib/dpkg/info/ ----------
    for entry in &control_tar {
        let name = entry.path.trim_start_matches("./");
        if matches!(
            name,
            "preinst" | "postinst" | "prerm" | "postrm" | "conffiles"
        ) {
            let path = format!("/var/lib/dpkg/info/{}.{}", info.package, name);
            write_vfs_file(&path, &entry.data);
        }
    }

    crate::println!("[dpkg] {} installed successfully", info.package);
    Ok(info)
}

/// Run a maintainer script (preinst/postinst/etc.) by executing it through
/// the process layer. Falls back to a no-op if the script isn't executable.
fn run_maintainer_script(name: &str, data: &[u8], package: &str) {
    crate::println!("[dpkg] running {} for {}", name, package);
    // Write script to /tmp and exec it
    let path = format!("/tmp/.dpkg-{}-{}", package, name);
    write_vfs_file(&path, data);

    // TODO: when userspace fork/exec is fully working, exec the script here
    // For now we just log it
    crate::serial_println!(
        "[dpkg] {} script ({} bytes) — exec pending userspace",
        name,
        data.len()
    );
}

/// Update /var/lib/dpkg/status with the installed package record.
fn update_status(info: &ControlInfo) {
    let entry = format!(
        "Package: {}\n\
         Status: install ok installed\n\
         Priority: {}\n\
         Section: {}\n\
         Installed-Size: {}\n\
         Maintainer: {}\n\
         Architecture: {}\n\
         Version: {}\n\
         Description: {}\n\n",
        info.package,
        info.priority,
        info.section,
        info.installed_size,
        info.maintainer,
        info.arch,
        info.version,
        info.description,
    );

    // Read existing status, remove old entry for this package, append new
    let status_path = "/var/lib/dpkg/status";
    let existing = read_vfs_file(status_path).unwrap_or_default();
    let existing_str = String::from_utf8_lossy(&existing);
    let filtered = filter_package_from_status(&existing_str, &info.package);
    let new_status = format!("{}{}", filtered, entry);
    write_vfs_file(status_path, new_status.as_bytes());
}

/// Remove an existing package stanza from a dpkg status file.
fn filter_package_from_status(status: &str, package: &str) -> String {
    let mut result = String::new();
    let mut in_pkg = false;
    for line in status.lines() {
        if let Some(pkg_name) = line.strip_prefix("Package: ") {
            in_pkg = pkg_name.trim() == package;
        }
        if !in_pkg {
            result.push_str(line);
            result.push('\n');
        }
        if line.is_empty() {
            in_pkg = false;
        }
    }
    result
}

/// Write the list of installed files to /var/lib/dpkg/info/<package>.list
fn write_file_list(package: &str, files: &[String]) {
    let path = format!("/var/lib/dpkg/info/{}.list", package);
    let content: String = files.iter().map(|f| format!("{}\n", f)).collect();
    write_vfs_file(&path, content.as_bytes());
}

// -- VFS helpers ------------------------------------------------------------

fn write_vfs_file(path: &str, data: &[u8]) {
    if let Some(slash) = path.rfind('/') {
        let dir = if slash == 0 { "/" } else { &path[..slash] };
        crate::mkdir_p_pub(dir);
    }
    let _ = crate::vfs_contract::VfsContract::write_file(path, data, 0o644);
}

fn read_vfs_file(path: &str) -> Option<Vec<u8>> {
    crate::vfs_contract::VfsContract::read_file(path).ok()
}
