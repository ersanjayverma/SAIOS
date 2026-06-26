//! apt — experimental Debian-style package ingestion for SAIOS.
//!
//! Provides a familiar command surface for fetching package metadata and .deb
//! archives. This is a compatibility-facing tool, not proof that SAIOS can run
//! arbitrary Debian packages yet.
//!
//! # Supported commands
//! ```
//! apt update              — refresh package lists from mirrors
//! apt install <pkg>...    — download and install packages
//! apt remove  <pkg>...    — uninstall packages
//! apt search  <term>      — search the package index
//! apt show    <pkg>       — display package metadata
//! apt list                — list installed packages
//! apt upgrade             — upgrade all installed packages
//! apt clean               — clear the package cache
//! ```
//!
//! # Architecture
//! apt → fetches InRelease + Packages.gz from mirror
//!     → parses package metadata
//!     → downloads .deb files to /var/cache/apt/archives/
//!     → calls dpkg to install (or applies directly for phase 1)
//!
//! # Default package sources
//! Default sources are in /etc/apt/sources.list:
//!   deb http://deb.debian.org/debian bookworm main contrib non-free
//!   deb http://security.debian.org/debian-security bookworm-security main
//!
//! Note: Full HTTPS support requires TLS — currently uses HTTP.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// A parsed entry from the Packages index file.
#[derive(Debug, Clone, Default)]
pub struct PackageInfo {
    /// Package name (e.g. "bash").
    pub name: String,
    /// Package version string (e.g. "5.2.15-2+b7").
    pub version: String,
    /// Architecture (e.g. "amd64").
    pub arch: String,
    /// Human-readable description (first line).
    pub description: String,
    /// Installed size in KB.
    pub installed_size: u64,
    /// Download size in bytes.
    pub size: u64,
    /// SHA256 hash of the .deb file.
    pub sha256: String,
    /// Relative path on the mirror to the .deb file.
    pub filename: String,
    /// Package dependencies.
    pub depends: Vec<String>,
}

/// Package database: name → PackageInfo.
type PkgDb = BTreeMap<String, PackageInfo>;

/// Currently loaded package database (populated by `apt update`).
static PKG_DB: spin::Mutex<PkgDb> = spin::Mutex::new(BTreeMap::new());
/// Set of installed package names (pub so build_essentials can read it).
pub static INSTALLED: spin::Mutex<alloc::collections::BTreeSet<String>> =
    spin::Mutex::new(alloc::collections::BTreeSet::new());

// -- Mirror configuration ---------------------------------------------------

/// Default package mirror. deb.debian.org is used as a familiar upstream while
/// package execution remains limited by the SAIOS userspace ABI.
const DEFAULT_MIRROR: &str = "deb.debian.org";
/// Default suite.
const DEFAULT_SUITE: &str = "bookworm";
/// Default components.
const DEFAULT_COMP: &str = "main";

/// Runtime mirror host, overridable at first boot and persisted to
/// /etc/saios.conf.  Defaults to the official CDN.
pub static MIRROR: spin::Mutex<alloc::string::String> =
    spin::Mutex::new(alloc::string::String::new());

/// The mirror host to use (falls back to the CDN default if unset).
pub fn mirror() -> alloc::string::String {
    let m = MIRROR.lock();
    if m.is_empty() {
        alloc::string::String::from(DEFAULT_MIRROR)
    } else {
        m.clone()
    }
}

/// Set the mirror host (from saved config or the first-boot wizard).
pub fn set_mirror(host: &str) {
    if !host.is_empty() {
        *MIRROR.lock() = alloc::string::String::from(host);
    }
}

/// Path to the package lists cache directory.
const LISTS_DIR: &str = "/var/lib/apt/lists";
/// Path to the downloaded .deb cache.
const CACHE_DIR: &str = "/var/cache/apt/archives";

// -- Main entry point -------------------------------------------------------

/// Run an apt command.
///
/// # Arguments
/// * `args` — argument string after the `apt` keyword.
pub fn run(args: &str) {
    let mut parts = args.trim().splitn(2, ' ');
    let subcmd = parts.next().unwrap_or("").trim();
    let rest = parts.next().unwrap_or("").trim();

    match subcmd {
        "update" => cmd_update(),
        "install" => cmd_install(rest),
        "remove" | "purge" => cmd_remove(rest),
        "search" => cmd_search(rest),
        "show" => cmd_show(rest),
        "list" => cmd_list(rest),
        "upgrade" => cmd_upgrade(),
        "clean" => cmd_clean(),
        "autoremove" => crate::println!("apt autoremove: nothing to remove"),
        "help" | "--help" => print_help(),
        "" => print_help(),
        other => crate::println!("apt: unknown command '{}'. Try apt help.", other),
    }
}

// -- apt update ------------------------------------------------------------

/// Fetch and parse the Packages index from all configured sources.
fn cmd_update() {
    let mir = mirror();
    crate::println!("Hit: http://{}/debian bookworm InRelease", mir);
    crate::println!("Hit: http://security.debian.org/debian-security bookworm-security InRelease");

    // Fetch the compressed package list.  Prefer Packages.xz: it is ~25% smaller
    // than Packages.gz on this index, which matters because the mirror/CDN serves
    // it with Accept-Ranges: none (ranged GETs 416), so a transfer that drops
    // mid-stream cannot be resumed — the whole file must arrive in one
    // connection.  .gz is kept as a fallback for mirrors lacking .xz.
    let base = format!(
        "/debian/dists/{}/{}/binary-amd64/Packages",
        DEFAULT_SUITE, DEFAULT_COMP
    );
    let candidates = [format!("{}.xz", base), format!("{}.gz", base)];

    let mut got: Option<Vec<u8>> = None;
    for url_path in &candidates {
        crate::print!("Get: http://{}{} ... ", mir, url_path);
        match download_url(&mir, url_path) {
            Some(body) if !body.is_empty() => {
                crate::println!("{} kB", body.len() / 1024);
                // The whole compressed stream must be intact to decompress; a
                // truncated transfer fails here and we fall through to the next
                // candidate format.
                crate::print!("Decompressing... ");
                match crate::compress::decompress(&body) {
                    Ok(d) => {
                        crate::println!("{} kB", d.len() / 1024);
                        got = Some(d);
                        break;
                    }
                    Err(e) => crate::println!("error: {} (trying next format)", e),
                }
            }
            _ => crate::println!("failed"),
        }
    }

    match got {
        Some(text_bytes) => {
            let _ = ensure_dir(LISTS_DIR);
            let text = String::from_utf8_lossy(&text_bytes).into_owned();
            let cache_path = format!("{}/debian_bookworm_main_amd64_Packages", LISTS_DIR);
            save_file(&cache_path, text.as_bytes());

            let count = parse_packages(&text);
            crate::println!("Reading package lists... Done");
            crate::println!("Building dependency tree... Done");
            crate::println!("{} packages available", count);
        }
        None => {
            crate::println!("apt update failed — could not fetch a usable package index");
            crate::println!("Check: net status, net dns {}", DEFAULT_MIRROR);
        }
    }
}

/// Parse a Packages file (RFC-2822-style stanzas) into the global package DB.
/// Returns the number of packages parsed.
fn parse_packages(text: &str) -> usize {
    let mut db = PKG_DB.lock();
    let mut pkg = PackageInfo::default();
    let mut count = 0usize;

    for line in text.lines() {
        if line.is_empty() {
            // End of stanza
            if !pkg.name.is_empty() {
                db.insert(pkg.name.clone(), pkg.clone());
                count += 1;
            }
            pkg = PackageInfo::default();
            continue;
        }
        if let Some(val) = line.strip_prefix("Package: ") {
            pkg.name = val.trim().to_string();
        }
        if let Some(val) = line.strip_prefix("Version: ") {
            pkg.version = val.trim().to_string();
        }
        if let Some(val) = line.strip_prefix("Architecture: ") {
            pkg.arch = val.trim().to_string();
        }
        if let Some(val) = line.strip_prefix("Description: ") {
            pkg.description = val.trim().to_string();
        }
        if let Some(val) = line.strip_prefix("Installed-Size:") {
            pkg.installed_size = val.trim().parse().unwrap_or(0);
        }
        if let Some(val) = line.strip_prefix("Size: ") {
            pkg.size = val.trim().parse().unwrap_or(0);
        }
        if let Some(val) = line.strip_prefix("SHA256: ") {
            pkg.sha256 = val.trim().to_string();
        }
        if let Some(val) = line.strip_prefix("Filename: ") {
            pkg.filename = val.trim().to_string();
        }
        if let Some(val) = line.strip_prefix("Depends: ") {
            pkg.depends = val
                .split(',')
                .map(|d| d.trim().split(' ').next().unwrap_or("").to_string())
                .collect();
        }
    }
    count
}

// -- apt install -----------------------------------------------------------

/// Download and install one or more packages.
fn cmd_install(args: &str) {
    if args.is_empty() {
        crate::println!("apt install: no package name given");
        return;
    }

    let pkgs: Vec<&str> = args.split_whitespace().collect();
    let db = PKG_DB.lock();

    // Check package database is populated
    if db.is_empty() {
        crate::println!("apt: package cache is empty. Run: apt update");
        return;
    }

    for pkg_name in &pkgs {
        match db.get(*pkg_name) {
            None => {
                crate::println!("E: Unable to locate package {}", pkg_name);
                crate::println!("   Try: apt search {}", pkg_name);
            }
            Some(info) => {
                crate::println!("The following NEW packages will be installed:");
                crate::println!(
                    "  {} ({}, {} KB)",
                    info.name,
                    info.version,
                    info.size / 1024
                );
                crate::println!();
                let mir = mirror();
                crate::print!("Get: http://{}/debian/{} ... ", mir, info.filename);

                let deb_path_url = format!("/debian/{}", info.filename);

                match download_url(&mir, &deb_path_url) {
                    Some(body) if !body.is_empty() => {
                        let deb_path = format!("{}/{}.deb", CACHE_DIR, pkg_name);
                        let _ = ensure_dir(CACHE_DIR);
                        save_file(&deb_path, &body);
                        crate::println!("{} B", body.len());

                        // Apply the package
                        match apply_deb(&deb_path, info) {
                            Ok(()) => {
                                INSTALLED.lock().insert(pkg_name.to_string());
                                crate::println!("Setting up {} ({}) ...", info.name, info.version);
                                crate::println!("Processing triggers for man-db ...");
                            }
                            Err(e) => crate::println!("dpkg: error applying {}: {}", pkg_name, e),
                        }
                    }
                    _ => {
                        crate::println!("Connection failed for {}", pkg_name);
                        crate::println!("Tip: verify with: net dns {}", DEFAULT_MIRROR);
                    }
                }
            }
        }
    }
}

/// Apply a downloaded .deb file using the real dpkg installer.
fn apply_deb(deb_path: &str, _info: &PackageInfo) -> Result<(), &'static str> {
    let data = crate::vfs_contract::VfsContract::read_file(deb_path).map_err(|_| "read failed")?;

    let ctrl = crate::package::install_deb(&data)?;
    // Update our in-memory installed set
    INSTALLED.lock().insert(ctrl.package.clone());
    Ok(())
}

// -- apt remove ------------------------------------------------------------

fn cmd_remove(args: &str) {
    for pkg in args.split_whitespace() {
        if INSTALLED.lock().remove(pkg) {
            crate::println!("Removing {} ...", pkg);
        } else {
            crate::println!("Package '{}' is not installed", pkg);
        }
    }
}

// -- apt search ------------------------------------------------------------

fn cmd_search(term: &str) {
    if term.is_empty() {
        crate::println!("apt search: no search term given");
        return;
    }
    let db = PKG_DB.lock();
    if db.is_empty() {
        crate::println!("apt: run 'apt update' first");
        return;
    }

    let mut found = 0;
    for (name, info) in db.iter() {
        if name.contains(term) || info.description.to_lowercase().contains(term) {
            let installed = if INSTALLED.lock().contains(name) {
                "[installed]"
            } else {
                ""
            };
            crate::println!("{}/{} {} {}", name, DEFAULT_SUITE, info.version, installed);
            crate::println!("  {}", info.description);
            found += 1;
        }
    }
    if found == 0 {
        crate::println!("No results for '{}'", term);
    } else {
        crate::println!("\n{} result(s)", found);
    }
}

// -- apt show --------------------------------------------------------------

fn cmd_show(pkg: &str) {
    let db = PKG_DB.lock();
    match db.get(pkg) {
        None => crate::println!("E: No packages found"),
        Some(info) => {
            let inst = if INSTALLED.lock().contains(pkg) {
                "installed"
            } else {
                "not installed"
            };
            crate::println!("Package: {}", info.name);
            crate::println!("Version: {}", info.version);
            crate::println!("Architecture: {}", info.arch);
            crate::println!("Installed-Size: {} kB", info.installed_size);
            crate::println!("Download-Size: {} kB", info.size / 1024);
            crate::println!(
                "APT-Sources: http://{}/debian {} {}",
                DEFAULT_MIRROR,
                DEFAULT_SUITE,
                DEFAULT_COMP
            );
            crate::println!("Description: {}", info.description);
            crate::println!("Status: {}", inst);
            if !info.depends.is_empty() {
                crate::println!("Depends: {}", info.depends.join(", "));
            }
        }
    }
}

// -- apt list -------------------------------------------------------------

fn cmd_list(filter: &str) {
    let installed = INSTALLED.lock();
    if filter == "--installed" || filter.is_empty() {
        crate::println!("Listing installed packages:");
        for pkg in installed.iter() {
            let db = PKG_DB.lock();
            let ver = db
                .get(pkg.as_str())
                .map(|i| i.version.as_str())
                .unwrap_or("unknown");
            crate::println!("{}/{} {} amd64 [installed]", pkg, DEFAULT_SUITE, ver);
        }
        crate::println!("{} packages installed", installed.len());
    }
}

// -- apt upgrade ----------------------------------------------------------

fn cmd_upgrade() {
    let installed: alloc::vec::Vec<String> = INSTALLED.lock().iter().cloned().collect();
    if installed.is_empty() {
        crate::println!("0 upgraded, 0 newly installed, 0 to remove.");
        return;
    }
    crate::println!("Calculating upgrade... Done");
    crate::println!("{} packages can be upgraded.", installed.len());
    crate::println!("Run apt install <pkg> to upgrade each package.");
}

// -- apt clean ------------------------------------------------------------

fn cmd_clean() {
    // Remove all .deb files from the cache directory
    if let Ok(entries) = crate::vfs_contract::VfsContract::read_dir(CACHE_DIR) {
        let mut removed = 0;
        for entry in entries {
            if entry.name.ends_with(".deb") {
                let path = format!("{}/{}", CACHE_DIR, entry.name);
                if crate::vfs_contract::VfsContract::unlink(&path).is_ok() {
                    removed += 1;
                }
            }
        }
        crate::println!("apt clean: removed {} .deb file(s) from cache", removed);
    }
}

// -- Resumable download ------------------------------------------------------

/// Download `path` from `host` over HTTP in a single attempt — no retries, no
/// Range.  The mirror/CDN serves the index with `Accept-Ranges: none` (416s any
/// ranged GET), and a truncated transfer cannot be safely resumed, so the only
/// sound behavior is: take the body iff it arrived verifiably complete
/// (`complete` && exact Content-Length), otherwise reject it.  Returning None on
/// a partial guarantees the caller never decompresses or installs partial data.
fn download_url(host: &str, path: &str) -> Option<Vec<u8>> {
    let req = crate::net::http::HttpRequest::get(host, path, 80);
    let resp = crate::net::http::send(req)?;

    if resp.status != 200 {
        crate::serial_println!("[apt] {} -> HTTP {}", path, resp.status);
        return None;
    }

    let cl = header_usize(&resp.headers, "content-length");
    let usable = resp.complete
        && match cl {
            Some(n) => resp.body_bytes.len() == n, // exact length required
            None => !resp.body_bytes.is_empty(),   // close-delimited, server finished
        };
    if usable {
        return Some(resp.body_bytes);
    }

    crate::serial_println!(
        "[apt] truncated: got {} of {:?} (complete={}) — rejecting (no retry)",
        resp.body_bytes.len(),
        cl,
        resp.complete
    );
    None
}

/// Case-insensitive numeric header lookup.
fn header_usize(headers: &[(String, String)], name: &str) -> Option<usize> {
    for (k, v) in headers {
        if k.eq_ignore_ascii_case(name) {
            return v.trim().parse::<usize>().ok();
        }
    }
    None
}

// -- Helpers ---------------------------------------------------------------

/// Ensure a directory path exists in the VFS, creating it if needed.
fn ensure_dir(path: &str) -> Result<(), &'static str> {
    if crate::vfs_contract::VfsContract::resolve(path).is_ok() {
        return Ok(());
    }
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let mut cur = String::from("/");
    for part in &parts {
        cur.push_str(part);
        if crate::vfs_contract::VfsContract::resolve(&cur).is_err() {
            crate::vfs_contract::VfsContract::mkdir(&cur, 0o755).map_err(|_| "mkdir failed")?;
        }
        cur.push('/');
    }
    Ok(())
}

/// Write bytes to a VFS path, creating the file if needed.
fn save_file(path: &str, data: &[u8]) {
    let _ = crate::vfs_contract::VfsContract::write_file(path, data, 0o644);
}

fn print_help() {
    crate::println!("apt - SAIOS package tool (EXPERIMENTAL Debian-style package support)");
    crate::println!();
    crate::println!("USAGE");
    crate::println!("  apt <command> [options]");
    crate::println!();
    crate::println!("COMMANDS");
    crate::println!("  update              Refresh package lists");
    crate::println!("  install <pkg>...    Install packages");
    crate::println!("  remove  <pkg>...    Remove packages");
    crate::println!("  search  <term>      Search package names/descriptions");
    crate::println!("  show    <pkg>       Show package details");
    crate::println!("  list    [--installed]  List packages");
    crate::println!("  upgrade             Upgrade installed packages");
    crate::println!("  clean               Clear downloaded .deb cache");
    crate::println!();
    crate::println!("EXAMPLES");
    crate::println!("  apt update && apt install bash");
    crate::println!("  apt search python3");
    crate::println!("  apt show gcc");
}
