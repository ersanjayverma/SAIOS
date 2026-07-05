ENTRY(_start)

KERNEL_PHYS_BASE = 0x100000;
KERNEL_VIRT_OFFSET = 0xffffffff80000000;

SECTIONS {
    . = KERNEL_PHYS_BASE;

    /* Low-half trampoline: runs before CR3 switch, VMA == LMA. */
    .boot ALIGN(4K) : {
        _boot_phys_start = .;
        KEEP(*(.boot))
    }

    /* Static boot page tables, low-half so physical == virtual. */
    .boot.data ALIGN(4K) : {
        KEEP(*(.boot.data))
    }

    . = ALIGN(4K);
    _boot_phys_end = .;

    /* From here on, VMA lives in the higher half while LMA stays contiguous. */
    . += KERNEL_VIRT_OFFSET;

    _kernel_start = .;

    .text ALIGN(4K) : AT(ADDR(.text) - KERNEL_VIRT_OFFSET) {
        KEEP(*(.text.boot))
        *(.text .text.*)
    }

    .rodata ALIGN(4K) : AT(ADDR(.rodata) - KERNEL_VIRT_OFFSET) {
        *(.rodata .rodata.*)
    }

    .data.rel.ro ALIGN(4K) : AT(ADDR(.data.rel.ro) - KERNEL_VIRT_OFFSET) {
        *(.data.rel.ro .data.rel.ro.*)
    }

    .got ALIGN(4K) : AT(ADDR(.got) - KERNEL_VIRT_OFFSET) {
        *(.got .got.*)
    }

    .data ALIGN(4K) : AT(ADDR(.data) - KERNEL_VIRT_OFFSET) {
        *(.data .data.*)
    }

    _bss_start = .;

    .bss ALIGN(4K) : AT(ADDR(.bss) - KERNEL_VIRT_OFFSET) {
        *(COMMON)
        *(.bss .bss.*)
    }

    . = ALIGN(4K);
    _kernel_end = .;
}
