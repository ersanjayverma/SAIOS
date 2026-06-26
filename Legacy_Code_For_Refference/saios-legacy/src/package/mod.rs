//! SAIOS package management — experimental dpkg/apt-style tooling.
//!
//! Current milestone: fetch and unpack selected `.deb` archives while the
//! native SAIOS libc and userspace ABI mature.

pub mod ar;
pub mod dpkg;
pub mod tar;

pub use dpkg::ControlInfo;

/// Install a .deb file from a byte slice.
pub fn install_deb(data: &[u8]) -> Result<ControlInfo, &'static str> {
    dpkg::install(data)
}
