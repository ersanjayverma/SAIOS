//! Architecture abstraction layer.
//!
//! Re-exports the active architecture implementation and exposes shared
//! paging helpers.

#[cfg(target_arch = "x86_64")]
pub mod x86_64;

pub mod paging;

#[cfg(target_arch = "x86_64")]
pub use x86_64::*;
