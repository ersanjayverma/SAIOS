use alloc::vec::Vec;
use uefi::boot::MemoryType as UefiMemoryType;
use uefi::mem::memory_map::MemoryMap;
use uefi::println;
#[repr(C)]
pub struct MemoryMapInfo {
    pub entries: *const MemoryRegion,
    pub entry_count: usize,
}
#[repr(C)]
pub struct MemoryRegion {
    pub base: u64,
    pub length: u64,
    pub region_type: MemoryType,
    pub attributes: u64,
}
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u32)]
pub enum MemoryType {
    Reserved,
    Usable,
    Reclaimable,
    ACPI,
    ACPINvs,
    MMIO,
    MMIOPort,
    Persistent,
    Bad,
    Loader,
    Seed,
    Framebuffer,
}
pub fn initialize() -> uefi::Result<MemoryMapInfo> {
    let memorymap = uefi::boot::memory_map(UefiMemoryType::LOADER_DATA)?;
    let entries = memorymap.entries();
    let _entry_count = entries.len();

    println!("Memory Map:");
    for entry in entries {
        println!(
            "Base: {:#x}, Length: {:#x}, Type: {:?}, Attributes: {:#x}",
            entry.phys_start,
            entry.page_count * 4096,
            convert_memory_type(entry.ty),
            entry.att.bits()
        );
    }
    let mut regions: Vec<MemoryRegion> = Vec::new();

    for d in memorymap.entries() {
        regions.push(MemoryRegion {
            base: d.phys_start,
            length: d.page_count * 4096,
            region_type: convert_memory_type(d.ty),
            attributes: d.att.bits(),
        });
    }
    let regions = regions.leak();

    Ok(MemoryMapInfo {
        entries: regions.as_ptr(),
        entry_count: regions.len(),
    })
}
fn convert_memory_type(ty: UefiMemoryType) -> MemoryType {
    match ty {
        UefiMemoryType::CONVENTIONAL => MemoryType::Usable,
        UefiMemoryType::LOADER_CODE | UefiMemoryType::LOADER_DATA => MemoryType::Loader,
        UefiMemoryType::BOOT_SERVICES_CODE | UefiMemoryType::BOOT_SERVICES_DATA => {
            MemoryType::Reclaimable
        }
        UefiMemoryType::ACPI_RECLAIM => MemoryType::ACPI,
        UefiMemoryType::ACPI_NON_VOLATILE => MemoryType::ACPINvs,
        _ => MemoryType::Reserved,
    }
}
