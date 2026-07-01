ENTRY(_start)

SECTIONS {
    . = 0x100000;

    _kernel_start = .;

    .text :
    {
        KEEP(*(.text.boot))
        *(.text .text.*)
    }

    .rodata :
    {
        *(.rodata .rodata.*)
    }

    .data.rel.ro :
    {
        *(.data.rel.ro .data.rel.ro.*)
    }

    .got :
    {
        *(.got .got.*)
    }

    .data :
    {
        *(.data .data.*)
    }

    /* End of file-backed data; everything past here is .bss (zero-init).
       The bootloader zeroes .bss via write_bytes (memsz - filesz). */
    _bss_start = .;

    .bss :
    {
        *(COMMON)
        *(.bss .bss.*)
    }

    /* Align to page boundary for any following sections */
    . = ALIGN(4096);
    _kernel_end = .;
}
