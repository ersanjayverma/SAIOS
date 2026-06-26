//! MBR + partition table writer.

use crate::block::BlockDevice;

/// Write the MBR: GRUB boot.img (first 446 bytes) + partition table + signature.
#[allow(dead_code)]
pub fn write_mbr(
    dev: &dyn BlockDevice,
    boot_img: &[u8],
    part_start: u64,
    part_size: u64,
) -> Result<(), &'static str> {
    let mut mbr = [0u8; 512];

    // Copy GRUB boot.img code (first 446 bytes - stops before partition table)
    let copy = boot_img.len().min(446);
    mbr[..copy].copy_from_slice(&boot_img[..copy]);

    // Patch boot.img: bytes 0x44-0x47 hold the LBA of core.img (little-endian).
    // GRUB's boot.img reads core.img from this sector.
    mbr[0x44..0x48].copy_from_slice(&1u32.to_le_bytes()); // core.img starts at LBA 1

    // -- Partition table (4 entries × 16 bytes at offset 0x1BE) ------------

    // Entry 0: one big bootable partition covering the rest of the disk
    let entry_off = 0x1BE;
    let e = &mut mbr[entry_off..entry_off + 16];

    e[0] = 0x80; // bootable flag
    // CHS start (rough - LBA mode used by GRUB anyway)
    lba_to_chs(part_start, &mut e[1..4]);
    e[4] = 0x83; // partition type: Linux native (ext4)
    // CHS end
    lba_to_chs(part_start + part_size - 1, &mut e[5..8]);
    // LBA start (little-endian u32)
    e[8..12].copy_from_slice(&(part_start as u32).to_le_bytes());
    // LBA size
    e[12..16].copy_from_slice(&(part_size as u32).to_le_bytes());

    // Boot signature
    mbr[510] = 0x55;
    mbr[511] = 0xAA;

    dev.write_sectors(0, &mbr).map_err(|_| "MBR write failed")
}

/// Write an MBR whose single partition is an **EFI System Partition** (type
/// 0xEF).  UEFI firmware ignores the MBR boot code and instead scans the
/// partition table for an ESP, mounts it as FAT, and runs
/// `/EFI/BOOT/BOOTX64.EFI`.  No GRUB boot.img / core.img is involved on the
/// EFI path, so the first 446 bytes are left zero (a harmless empty bootstrap).
pub fn write_efi_mbr(
    dev: &dyn BlockDevice,
    esp_start: u64,
    esp_size: u64,
    ext4: Option<(u64, u64)>, // (start_lba, size_sectors) of the ext4 root
) -> Result<(), &'static str> {
    let mut mbr = [0u8; 512];

    // Partition entry 0 (offset 0x1BE): the EFI System Partition (type 0xEF).
    write_part(&mut mbr[0x1BE..0x1BE + 16], 0x80, 0xEF, esp_start, esp_size);

    // Partition entry 1 (offset 0x1CE): the ext4 root/data partition (type 0x83).
    if let Some((start, size)) = ext4 {
        write_part(&mut mbr[0x1CE..0x1CE + 16], 0x00, 0x83, start, size);
    }

    mbr[510] = 0x55;
    mbr[511] = 0xAA;

    dev.write_sectors(0, &mbr)
        .map_err(|_| "EFI MBR write failed")
}

/// Fill one 16-byte MBR partition entry.
fn write_part(e: &mut [u8], boot_flag: u8, ptype: u8, start: u64, size: u64) {
    e[0] = boot_flag;
    lba_to_chs(start, &mut e[1..4]);
    e[4] = ptype;
    lba_to_chs(start + size - 1, &mut e[5..8]);
    e[8..12].copy_from_slice(&(start as u32).to_le_bytes());
    e[12..16].copy_from_slice(&(size as u32).to_le_bytes());
}

/// Rough CHS encoding - modern BIOSes use LBA, but the field must be non-zero.
fn lba_to_chs(lba: u64, out: &mut [u8]) {
    let cylinders = 1023u64;
    let heads = 254u64;
    let sectors = 63u64;
    let c = (lba / (heads * sectors)).min(cylinders);
    let h = (lba / sectors % heads) as u8;
    let s = (lba % sectors + 1) as u8;
    out[0] = h;
    out[1] = ((c >> 8) as u8 & 0x03) << 6 | (s & 0x3F);
    out[2] = (c & 0xFF) as u8;
}
