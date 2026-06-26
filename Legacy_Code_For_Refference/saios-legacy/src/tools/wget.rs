//! wget â€” non-interactive HTTP file downloader for SAIOS.
//!
//! wget is simpler than curl: it fetches a URL and saves the result to
//! a file derived from the URL's last path component (or -O <file>).
//!
//! # Usage
//! ```
//! wget http://example.com/file.tar.gz
//! wget -O /tmp/custom.tar.gz http://example.com/file.tar.gz
//! wget -q http://example.com/   # quiet mode
//! ```
//!
//! # Network path
//! Uses SAIOS's TCP/IP stack:
//!   DNS â†’ ARP â†’ TCP SYN/ACK â†’ HTTP GET â†’ write to VFS

use alloc::format;
use alloc::string::{String, ToString};

/// Run the wget command.
///
/// # Arguments
/// * `args` â€” raw argument string after the `wget` keyword.
pub fn run(args: &str) {
    if args.trim() == "--help" || args.trim() == "-h" {
        print_help();
        return;
    }

    // Parse arguments
    let mut url = String::new();
    let mut output: Option<String> = None; // -O <file>
    let mut quiet = false; // -q
    let mut no_check_cert = false; // --no-check-certificate

    let tokens: alloc::vec::Vec<&str> = args.split_whitespace().collect();
    let mut i = 0;
    while i < tokens.len() {
        match tokens[i] {
            "-O" | "--output-document" => {
                i += 1;
                output = tokens.get(i).map(|s| s.to_string());
            }
            "-q" | "--quiet" => quiet = true,
            "--no-check-certificate" => no_check_cert = true,
            "-P" | "--directory-prefix" => {
                // Prepend directory to the auto-derived filename
                i += 1;
                // TODO: prefix handling
            }
            arg if arg.starts_with("http") => url = arg.to_string(),
            arg if !arg.starts_with('-') => url = arg.to_string(),
            _ => {}
        }
        i += 1;
    }

    if url.is_empty() {
        crate::println!("wget: missing URL\nusage: wget [options] <url>");
        return;
    }

    // Derive output filename from URL if -O not given
    let outfile = resolve_output_path(&output.unwrap_or_else(|| derive_filename(&url)));

    if !quiet {
        crate::println!("--{}--  {}", timestamp(), url);
        crate::println!("Resolving {}...", host_of(&url));
    }

    // Parse URL
    let (scheme, host, port, path) = match super::curl::parse_url_pub(&url) {
        Some(t) => t,
        None => {
            crate::println!("wget: invalid URL: {}", url);
            return;
        }
    };

    if scheme == "https" && !no_check_cert && !quiet {
        crate::println!(
            "wget: NOTE: live HTTPS cert auto-verification pending TLS1.3 flight decode."
        );
        crate::println!(
            "      X.509 chain verifier available: 'openssl x509 -in <cert> -verify <host>'."
        );
    }

    // Send the request - over TLS for https://, plain HTTP otherwise.
    let req = crate::net::http::HttpRequest::get(&host, &path, port);
    if !quiet {
        crate::print!("Connecting to {}:{}... ", host, port);
    }

    let result = if scheme == "https" {
        crate::net::http::send_https(req)
    } else {
        crate::net::http::send(req)
    };
    let resp = match result {
        Some(r) => {
            if !quiet {
                crate::println!("connected.");
            }
            r
        }
        None => {
            crate::println!("\nwget: cannot connect to {}:{}", host, port);
            return;
        }
    };

    if resp.status != 200 && !quiet {
        crate::println!("wget: server returned HTTP {}", resp.status);
        if resp.status / 100 == 3
            && let Some((_, loc)) = resp
                .headers
                .iter()
                .find(|(k, _)| k.to_lowercase() == "location")
        {
            crate::println!("wget: redirect to {} - rerun with new URL", loc);
        }
        return;
    }

    let bytes = &resp.body_bytes[..];
    let size = bytes.len();

    if !quiet {
        crate::println!("HTTP request sent, awaiting response... {}", resp.status);
        crate::println!("Length: {} bytes", size);
        crate::println!("Saving to: '{}'", outfile);
    }

    // Write to VFS
    match write_file(&outfile, bytes) {
        Ok(()) => {
            if !quiet {
                crate::println!("'{}' saved [{} bytes]", outfile, size);
            }
        }
        Err(e) => crate::println!("wget: cannot save to '{}': {}", outfile, e),
    }
}

/// Write bytes to a VFS path, creating the file if it doesn't exist.
fn write_file(path: &str, data: &[u8]) -> Result<(), &'static str> {
    crate::vfs_contract::VfsContract::write_file(path, data, 0o644).map_err(|_| "write failed")
}

fn resolve_output_path(path: &str) -> String {
    if path.starts_with('/') {
        crate::vfs::path::normalise(path)
    } else {
        crate::shell::commands::vfs_abs_pub(path)
    }
}

/// Extract the filename from the last URL path component.
/// `http://example.com/files/archive.tar.gz` â†’ `"archive.tar.gz"`
/// `http://example.com/` â†’ `"index.html"`
fn derive_filename(url: &str) -> String {
    let path_part = url.split("://").nth(1).unwrap_or(url);
    let last = path_part.rsplit('/').find(|s| !s.is_empty());
    last.map(|s| s.to_string())
        .unwrap_or_else(|| String::from("index.html"))
}

/// Extract the host from a URL for display purposes.
fn host_of(url: &str) -> &str {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    rest.split('/').next().unwrap_or(rest)
}

/// Return a simplified timestamp string (ticks converted to HH:MM:SS).
fn timestamp() -> String {
    let ticks = crate::shell::commands::boot_ticks();
    let secs = ticks / 18;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

fn print_help() {
    crate::println!("wget â€” SAIOS file downloader");
    crate::println!();
    crate::println!("USAGE");
    crate::println!("  wget [OPTIONS] <url>");
    crate::println!();
    crate::println!("OPTIONS");
    crate::println!("  -O <file>               Save to specific filename");
    crate::println!("  -q, --quiet             Quiet mode");
    crate::println!("  --no-check-certificate  Skip TLS cert check");
    crate::println!();
    crate::println!("EXAMPLES");
    crate::println!("  wget http://example.com/file.tar.gz");
    crate::println!("  wget -O /tmp/page.html http://example.com/");
}
