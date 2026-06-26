# boot.s — Multiboot2 header + 32-bit bootstrap → 64-bit long mode entry

.set MB2_MAGIC,        0xe85250d6
.set MB2_ARCH_I386,    0
.set MB2_HEADER_LEN,   (mb2_header_end - mb2_header_start)
.set MB2_CHECKSUM,     -(MB2_MAGIC + MB2_ARCH_I386 + MB2_HEADER_LEN)

.section .multiboot2, "a"
.align 8
mb2_header_start:
    .long  MB2_MAGIC
    .long  MB2_ARCH_I386
    .long  MB2_HEADER_LEN
    .long  MB2_CHECKSUM
    # End tag
    .short 0
    .short 0
    .long  8
mb2_header_end:

# -- 32-bit entry (GRUB lands here) -----------------------------------------

.section .text.boot, "ax"
.code32
.global _start
_start:
    cli
    movl $stack_top, %esp
    # Save MBI pointer from %ebx before we clobber registers
    movl %ebx, mbi_ptr_32

    # Check for CPUID support
    pushfl
    popl  %eax
    movl  %eax, %ecx
    xorl  $(1 << 21), %eax
    pushl %eax
    popfl
    pushfl
    popl  %eax
    pushl %ecx
    popfl
    cmpl  %ecx, %eax
    je    .no_cpuid

    # Check for long mode (extended CPUID)
    movl  $0x80000000, %eax
    cpuid
    cmpl  $0x80000001, %eax
    jb    .no_long_mode
    movl  $0x80000001, %eax
    cpuid
    testl $(1 << 29), %edx
    jz    .no_long_mode

    # Set up minimal page tables (identity + higher-half)
    call  setup_page_tables
    call  enable_paging

    # Load 64-bit GDT
    lgdt  gdt64_ptr

    # Far-jump to 64-bit code segment
    ljmp  $0x08, $_start64

.no_cpuid:
.no_long_mode:
    hlt
    jmp   .no_long_mode

# -- Page table setup — identity map up to 128 GiB ---------------------------
#
# We use 4-level paging with 2 MiB huge pages:
#   PML4[0] → PDPT
#   PDPT[0..127] → pd_tables[0..127]   (one PD per GiB)
#   pd_tables[n][0..511] = n*1GiB + m*2MiB | 0x83
#
# Physical address of a given entry = (pdpt_idx * 512 + pd_idx) * 2 MiB
# In 32-bit: low32 = overall_idx << 21 (mod 2^32)
#            high32 = overall_idx >> 11
#
# 128 PDPT entries × 512 PD entries × 2 MiB = 128 GiB total coverage.
# Page table storage: 128 × 4096 = 512 KiB in .bss (pd_tables).

setup_page_tables:
    # -- PML4[0] → PDPT ----------------------------------------------------
    movl  $pdpt, %eax
    orl   $0x3, %eax
    movl  %eax, pml4
    movl  $0, pml4 + 4

    # -- PDPT[0..127] → pd_tables[0..127] ----------------------------------
    movl  $0, %ecx
.fill_pdpt_loop:
    # Address of pd_tables[ecx] = pd_tables + ecx * 4096
    movl  %ecx, %eax
    shll  $12, %eax            # eax = ecx * 4096
    addl  $pd_tables, %eax     # eax = &pd_tables[ecx]
    orl   $0x3, %eax           # present + writable
    movl  %eax, pdpt(, %ecx, 8)
    movl  $0, pdpt + 4(, %ecx, 8)
    incl  %ecx
    cmpl  $128, %ecx
    jne   .fill_pdpt_loop

    # -- pd_tables: each entry maps a 2 MiB huge page ----------------------
    # Overall entry index: 0 .. 128*512-1 = 0 .. 65535
    # Physical address = overall_idx * 2 MiB
    #   low32  = (overall_idx << 21) & 0xFFFFFFFF
    #   high32 = overall_idx >> 11
    movl  $0, %ecx             # %ecx = overall 2 MiB index
.fill_all_pd:
    movl  %ecx, %eax
    shll  $21, %eax            # low 32 bits of physical address
    movl  %ecx, %edx
    shrl  $11, %edx            # high 32 bits (handles > 4 GiB)
    orl   $0x83, %eax          # present | writable | huge (2 MiB)
    # Slot address: pd_tables + ecx * 8  (8 bytes per 64-bit entry)
    movl  %ecx, %esi
    shll  $3, %esi
    addl  $pd_tables, %esi
    movl  %eax, (%esi)         # low 32 bits
    movl  %edx, 4(%esi)        # high 32 bits
    incl  %ecx
    cmpl  $65536, %ecx         # 128 GiB / 2 MiB = 65536 entries
    jne   .fill_all_pd
    ret

enable_paging:
    # Point CR3 at PML4
    movl  $pml4, %eax
    movl  %eax, %cr3

    # Enable PAE
    movl  %cr4, %eax
    orl   $(1 << 5), %eax
    movl  %eax, %cr4

    # Set LME in EFER
    movl  $0xC0000080, %ecx
    rdmsr
    orl   $(1 << 8), %eax
    wrmsr

    # Enable paging
    movl  %cr0, %eax
    orl   $(1 << 31), %eax
    movl  %eax, %cr0
    ret

# -- Minimal GDT for 64-bit mode --------------------------------------------

.align 8
gdt64:
    .quad 0                                # null
    .quad 0x00af9a000000ffff               # 64-bit code (DPL0)
    .quad 0x00af92000000ffff               # 64-bit data (DPL0)
gdt64_end:

gdt64_ptr:
    .short gdt64_end - gdt64 - 1
    .long  gdt64

# -- 64-bit entry -----------------------------------------------------------

.code64
_start64:
    movw  $0x10, %ax
    movw  %ax,   %ds
    movw  %ax,   %es
    movw  %ax,   %fs
    movw  %ax,   %gs
    movw  %ax,   %ss

    movabsq $stack_top, %rsp

    # Pass MBI pointer as first argument (rdi = System V AMD64 ABI arg1)
    movl  mbi_ptr_32(%rip), %edi

    movabsq $kernel_main, %rax
    call  *%rax

.halt:
    hlt
    jmp   .halt

# -- BSS: page tables + stack -----------------------------------------------

.section .data
mbi_ptr_32: .long 0

.section .bss
.align 4096
pml4:      .skip 4096           # PML4 (one page)
pdpt:      .skip 4096           # PDPT covering 0–512 GiB (one page)
pd_tables: .skip 4096 * 128     # 128 PD tables → 512 KiB (128 GiB coverage)

.align 16
stack_bottom:
    .skip 65536         # 64 KiB kernel stack
.global stack_top
stack_top:
