ENTRY(_start)

SECTIONS {
    . = 0x100000;

    _kernel_start = .;

    /* Multiboot2 header — must be within first 32 KiB of the file */
    .boot :
    {
        KEEP(*(.multiboot2))
    }

    /* Boot assembly (_start, page tables, GDT, _start64) MUST come first
       so the ELF entry point resolves to _start at 0x100020. */
    .text :
    {
        KEEP(*(.text.boot))
        *(.text .text.*)
    }

    .rodata :
    {
        *(.rodata .rodata.*)
    }

    .data :
    {
        *(.data .data.*)
    }

    /* End of file-backed data; everything past here is .bss (zero-init), which
       the boot loader must clear (boot.s does not zero BSS itself). */
    _bss_start = .;

    .bss :
    {
        *(COMMON)
        *(.bss .bss.*)
    }

    _kernel_end = .;
}
