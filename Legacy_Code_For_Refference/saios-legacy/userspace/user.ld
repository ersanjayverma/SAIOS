/* SAIOS user-space linker script
 *
 * Produces ET_EXEC (static) binaries at canonical user addresses under PML4[1]
 * (512 GiB .. 1 TiB).  The kernel identity map occupies PML4[0..255], so user
 * pages must live in the private high slots PML4[256..511] to guarantee
 * per-process isolation.
 *
 * Layout:
 *   0x0000_0080_0040_0000  .text   (512 GiB + 4 MiB — matches USER_TEXT_BASE + 4M offset)
 *   0x0000_0080_0080_0000  .rodata (512 GiB + 8 MiB)
 *   0x0000_0080_00C0_0000  .data   (512 GiB + 12 MiB)
 *   0x0000_0080_00C0_0000  .bss    (follows .data)
 *
 * NOTE: All addresses are canonical for 4-level paging (bit 47 = 0, bits 48-63 = 0).
 *       Addresses above 0x0000_7FFF_FFFF_FFFF are non-canonical and cause #GP(0).
 */

ENTRY(_start)

SECTIONS {
    . = 0x8000400000;

    _user_start = .;

    .text : {
        *(.text.boot)
        *(.text .text.*)
    }

    .rodata : {
        *(.rodata .rodata.*)
    }

    .data : {
        *(.data .data.*)
    }

    _bss_start = .;

    .bss : {
        *(COMMON)
        *(.bss .bss.*)
    }

    _user_end = .;

    /DISCARD/ : {
        *(.comment)
        *(.note.*)
        *(.eh_frame)
    }
}