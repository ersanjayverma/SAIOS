pub mod acpi;
pub mod cpu;
pub mod firmware;
pub mod graphics;
pub mod memorymap;
pub mod pixelformat;
pub mod smbios;
extern crate alloc;
pub use crate::acpi::*;
pub use crate::cpu::*;
pub use crate::firmware::*;
pub use crate::graphics::*;
pub use crate::memorymap::*;
pub use crate::pixelformat::{PixelFormat, convert_pixel_format};
pub use crate::smbios::*;

pub const SAIOS_BOOT_MAGIC: u64 = 0x5341_494F_5342_4F4F; // Choose your preferred value
pub const SAIOS_BOOT_VERSION: u32 = 1;

#[repr(C)]
pub struct SaiosBootInfo {
    pub magic: u64,
    pub version: u32,
    pub size: u32,

    pub framebuffer: FramebufferInfo,
    pub memorymap: MemoryMapInfo,
    pub acpi: AcpiInfo,
    pub smbios: SmbiosInfo,
    pub cpu: CpuInfo,
    pub firmware: FirmwareInfo,

    pub reserved: [u64; 16],
}
