//! UEFI memory map capture and handoff structures.

extern crate alloc;
use core::cell::UnsafeCell;
use uefi::boot::MemoryType as UefiMemoryType;
use uefi::mem::memory_map::MemoryMap;

/// Maximum number of UEFI memory descriptors supported during boot.
const MEMORY_REGION_CAPACITY: usize = 1024;
const UEFI_PAGE_SIZE: u64 = 4096;

struct MemoryRegionBuffer(UnsafeCell<[MemoryRegion; MEMORY_REGION_CAPACITY]>);

unsafe impl Sync for MemoryRegionBuffer {}

static MEMORY_REGIONS: MemoryRegionBuffer = MemoryRegionBuffer(UnsafeCell::new(
    [MemoryRegion {
        base: 0,
        length: 0,
        region_type: MemoryType::Reserved,
        attributes: 0,
    }; MEMORY_REGION_CAPACITY],
));

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct MemoryMapInfo {
    pub entries: *const MemoryRegion,
    pub entry_count: usize,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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
    // Retrieve the UEFI memory map.  This must happen AFTER all
    // LOADER_DATA allocations (kernel, stack, boot-info, entries
    // storage) so those regions appear in the map.
    let memorymap = uefi::boot::memory_map(UefiMemoryType::LOADER_DATA)?;

    // SAFETY: single-threaded boot context.
    let regions = unsafe { &mut *MEMORY_REGIONS.0.get() };
    let mut count = 0;

    for entry in memorymap.entries() {
        if count >= MEMORY_REGION_CAPACITY {
            return Err(uefi::Error::from(uefi::Status::BUFFER_TOO_SMALL));
        }

        regions[count] = MemoryRegion {
            base: entry.phys_start,
            length: entry.page_count * UEFI_PAGE_SIZE,
            region_type: convert_memory_type(entry.ty),
            attributes: entry.att.bits(),
        };
        count += 1;
    }

    Ok(MemoryMapInfo {
        entries: regions.as_ptr(),
        entry_count: count,
    })
}

pub fn convert_memory_type(ty: UefiMemoryType) -> MemoryType {
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
