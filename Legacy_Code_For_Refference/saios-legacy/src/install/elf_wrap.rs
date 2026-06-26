//! Build a valid ELF64 + Multiboot2 kernel image from the currently-running kernel.
//!
//! The running kernel is identity-mapped at physical address _kernel_start.
//! We package those bytes as a single PT_LOAD ELF segment so GRUB can load it
//! exactly as it loaded the original ISO kernel.

use alloc::vec::Vec;

unsafe extern "C" {
    static _kernel_start: u8;
    static _kernel_end: u8;
    static _bss_start: u8;
}

const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
const LOAD_BASE: u64 = 0x0010_0000; // 1 MiB — must match linker.ld
const ENTRY_POINT: u64 = 0x0010_0020; // _start in .text.boot

// Pristine snapshot of the file-backed kernel image (.text+.rodata+.data),
// captured at the very start of kernel_main BEFORE .data is dirtied.  Installing
// a runtime snapshot of .data re-boots a kernel whose statics (notably the heap
// allocator's free list) already hold live state → corruption.  This buffer
// lives in .bss (after _bss_start), so it is NOT part of the snapshot itself.
const SNAP_CAP: usize = 16 * 1024 * 1024;
static mut KERNEL_SNAPSHOT: [u8; SNAP_CAP] = [0u8; SNAP_CAP];
static mut SNAP_LEN: usize = 0;

/// Capture the pristine kernel image. MUST be the first thing kernel_main does,
/// before any `.data` static is modified.
pub fn snapshot_kernel() {
    unsafe {
        let ks = &_kernel_start as *const u8 as usize;
        let bss = &_bss_start as *const u8 as usize;
        let len = bss.saturating_sub(ks);
        if len > 0 && len <= SNAP_CAP {
            core::ptr::copy_nonoverlapping(
                ks as *const u8,
                core::ptr::addr_of_mut!(KERNEL_SNAPSHOT) as *mut u8,
                len,
            );
            SNAP_LEN = len;
        }
    }
}

/// Read the running kernel image from memory and wrap it in a fresh ELF64 header.
pub fn build_elf() -> Result<Vec<u8>, &'static str> {
    let (kernel_start, kernel_end) = unsafe {
        (
            &_kernel_start as *const u8 as u64,
            &_kernel_end as *const u8 as u64,
        )
    };
    if kernel_end <= kernel_start {
        return Err("elf_wrap: invalid kernel boundaries");
    }
    // filesz = the PRISTINE .text+.rodata+.data captured at boot; memsz spans the
    // whole image (incl .bss) so GRUB zeroes [filesz..memsz] — boot.s does not
    // clear BSS itself and the kernel's page tables live there.
    let file_size = unsafe { SNAP_LEN }; // p_filesz
    let kernel_size = (kernel_end - kernel_start) as usize; // p_memsz
    if file_size == 0 || file_size > kernel_size {
        return Err("elf_wrap: kernel not snapshotted at boot");
    }
    if kernel_size > 64 * 1024 * 1024 {
        return Err("elf_wrap: kernel > 64 MiB — refusing");
    }

    crate::serial_println!(
        "[elf_wrap] kernel {:#x}–{:#x} (file {} KiB, mem {} KiB, pristine)",
        kernel_start,
        kernel_end,
        file_size / 1024,
        kernel_size / 1024
    );

    // Pristine file-backed bytes captured before .data was dirtied.
    let kernel_bytes = unsafe { &KERNEL_SNAPSHOT[..file_size] };

    // ELF layout:
    //   0x000 – 0x03F  : ELF64 header (64 bytes)
    //   0x040 – 0x077  : Program header #0 — PT_LOAD (56 bytes)
    //   0x078 – 0x08F  : (padding to 4 KiB)
    //   0x1000+         : kernel binary

    const FILE_OFFSET: usize = 0x1000; // align code to 4 KiB in file
    let total_size = FILE_OFFSET + file_size;

    let mut elf = alloc::vec![0u8; total_size];

    // -- ELF64 header ------------------------------------------------------
    let h = &mut elf[..64];
    h[0..4].copy_from_slice(&ELF_MAGIC);
    h[4] = 2; // EI_CLASS: 64-bit
    h[5] = 1; // EI_DATA: little-endian
    h[6] = 1; // EI_VERSION
    h[7] = 0; // EI_OSABI: none
    // e_type = ET_EXEC (2)
    h[16] = 2;
    h[17] = 0;
    // e_machine = EM_X86_64 (62 = 0x3E)
    h[18] = 0x3E;
    h[19] = 0;
    // e_version = 1
    h[20] = 1;
    // e_entry
    le64(&mut h[24..32], ENTRY_POINT);
    // e_phoff = 64 (program headers immediately after ELF header)
    le64(&mut h[32..40], 64);
    // e_shoff = 0 (no section headers)
    le64(&mut h[40..48], 0);
    // e_flags = 0
    le32(&mut h[48..52], 0);
    // e_ehsize = 64
    le16(&mut h[52..54], 64);
    // e_phentsize = 56
    le16(&mut h[54..56], 56);
    // e_phnum = 1
    le16(&mut h[56..58], 1);
    // e_shentsize = 64
    le16(&mut h[58..60], 64);
    // e_shnum = 0
    le16(&mut h[60..62], 0);
    // e_shstrndx = 0
    le16(&mut h[62..64], 0);

    // -- Program header (PT_LOAD) -------------------------------------------
    let ph = &mut elf[64..120];
    // p_type = PT_LOAD (1)
    le32(&mut ph[0..4], 1);
    // p_flags = PF_R | PF_W | PF_X (7)
    le32(&mut ph[4..8], 7);
    // p_offset = FILE_OFFSET
    le64(&mut ph[8..16], FILE_OFFSET as u64);
    // p_vaddr = LOAD_BASE (= physical address in identity map)
    le64(&mut ph[16..24], LOAD_BASE);
    // p_paddr = LOAD_BASE
    le64(&mut ph[24..32], LOAD_BASE);
    // p_filesz — only the file-backed part (.text+.rodata+.data)
    le64(&mut ph[32..40], file_size as u64);
    // p_memsz — full image; loader zeroes [filesz..memsz] = .bss
    le64(&mut ph[40..48], kernel_size as u64);
    // p_align = 0x1000
    le64(&mut ph[48..56], 0x1000);

    // -- Kernel data (file-backed part only; .bss is loader-zeroed) -----------
    elf[FILE_OFFSET..FILE_OFFSET + file_size].copy_from_slice(kernel_bytes);

    Ok(elf)
}

fn le16(buf: &mut [u8], v: u16) {
    buf[0] = v as u8;
    buf[1] = (v >> 8) as u8;
}
fn le32(buf: &mut [u8], v: u32) {
    buf[0] = v as u8;
    buf[1] = (v >> 8) as u8;
    buf[2] = (v >> 16) as u8;
    buf[3] = (v >> 24) as u8;
}
fn le64(buf: &mut [u8], v: u64) {
    for (i, byte) in buf.iter_mut().enumerate().take(8) {
        *byte = (v >> (i * 8)) as u8;
    }
}
