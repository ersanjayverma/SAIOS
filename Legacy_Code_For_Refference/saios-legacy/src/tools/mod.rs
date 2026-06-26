//! SAIOS built-in tools — standard Unix utilities implemented in Rust.
//!
//! Each tool is a module with a `run(args: &str)` entry point called by the shell.
//!
//! | Tool              | Module                | Description                          |
//! |-------------------|-----------------------|--------------------------------------|
//! | curl              | `curl`                | HTTP client (GET/POST/PUT/DELETE)     |
//! | wget              | `wget`                | File downloader                       |
//! | vi                | `vi`                  | Modal text editor (vi-compatible)     |
//! | nano              | `nano`                | Simple modeless text editor           |
//! | apt               | `apt`                 | Experimental Debian-style fetcher     |
//! | build-essential   | `build_essentials`    | Cross-compile toolchain guide + make  |

pub mod apt;
pub mod build_essentials;
pub mod curl;
pub mod nano;
pub mod openssl;
pub mod ssh;
pub mod vi;
pub mod wget;

// Re-export parse_url for use by wget
// (curl's parse_url is private — expose it via a pub wrapper)
pub use curl::parse_url_pub;

pub fn resolve_path(path: &str) -> alloc::string::String {
    if path.trim().is_empty() {
        return alloc::string::String::new();
    }
    crate::shell::commands::vfs_abs_pub(path)
}
