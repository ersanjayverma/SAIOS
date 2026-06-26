# smp_trampoline.s — Application Processor (AP) startup trampoline.
#
# Copied verbatim to physical 0x8000 by smp::init().  An AP receives a SIPI with
# vector 0x08, so it begins executing in 16-bit real mode at CS:IP = 0x0800:0x0000
# (linear 0x8000).  The trampoline walks real-mode → protected mode → long mode
# using the SAME PML4 the BSP uses (identity-maps the low 128 GiB), then jumps to
# the 64-bit Rust entry `ap_entry`.
#
# All absolute addresses are expressed as (0x8000 + (label - ap_trampoline_start)):
# the assembler's `label - start` yields the offset within the blob, which equals
# the offset from 0x8000 once copied.  Three slots are patched at runtime by Rust:
#   tramp_cr3    (u32)  — physical address of the kernel PML4
#   tramp_entry  (u64)  — absolute address of ap_entry (64-bit Rust)
#   tramp_stack  (u64)  — top of this AP's kernel stack
#
# A spin lock (tramp_lock) serialises AP bringup so APs start one at a time and
# each consumes the single stack slot before the next SIPI is sent.

.set TRAMP, 0x8000

.section .text.smptramp, "ax"
.code16
.global ap_trampoline_start
.global ap_trampoline_end
.global ap_tramp_cr3
.global ap_tramp_entry
.global ap_tramp_stack

ap_trampoline_start:
    cli
    cld
    # Real-mode data access via DS=0 (linear == offset for 0x8000+ slots).
    xorw    %ax, %ax
    movw    %ax, %ds
    movw    %ax, %es
    movw    %ax, %ss

    # Load the 32-bit GDT (base patched/absolute below).
    lgdtl   (TRAMP + (tramp_gdtptr - ap_trampoline_start))

    # Enter protected mode (CR0.PE).
    movl    %cr0, %eax
    orl     $1, %eax
    movl    %eax, %cr0

    # Far jump into 32-bit code (selector 0x08), flushing the pipeline.
    ljmpl   $0x08, $(TRAMP + (pm32 - ap_trampoline_start))

.code32
pm32:
    # Reload data segments with the 32-bit data selector (0x10).
    movw    $0x10, %ax
    movw    %ax, %ds
    movw    %ax, %es
    movw    %ax, %ss

    # Enable PAE (CR4.PAE = bit 5).
    movl    %cr4, %eax
    orl     $(1 << 5), %eax
    movl    %eax, %cr4

    # Load CR3 with the kernel PML4 physical address (patched).
    movl    (TRAMP + (tramp_cr3 - ap_trampoline_start)), %eax
    movl    %eax, %cr3

    # Set EFER.LME (long mode enable, bit 8).
    movl    $0xC0000080, %ecx
    rdmsr
    orl     $(1 << 8), %eax
    wrmsr

    # Enable paging (CR0.PG = bit 31) → activates long mode.
    movl    %cr0, %eax
    orl     $(1 << 31), %eax
    movl    %eax, %cr0

    # Far jump into 64-bit code (selector 0x18 in the trampoline GDT).
    ljmpl   $0x18, $(TRAMP + (lm64 - ap_trampoline_start))

.code64
lm64:
    # Load this AP's stack (patched by Rust before each SIPI).
    movabsq $(TRAMP + (tramp_stack - ap_trampoline_start)), %rax
    movq    (%rax), %rsp

    # Jump to the 64-bit Rust entry (patched).
    movabsq $(TRAMP + (tramp_entry - ap_trampoline_start)), %rax
    movq    (%rax), %rax
    callq   *%rax
1:  hlt
    jmp     1b

# -- Trampoline GDT (32-bit + 64-bit code/data) ------------------------------
.align 16
tramp_gdt:
    .quad 0x0000000000000000          # 0x00 null
    .quad 0x00CF9A000000FFFF          # 0x08 32-bit code
    .quad 0x00CF92000000FFFF          # 0x10 32-bit data
    .quad 0x00AF9A000000FFFF          # 0x18 64-bit code
    .quad 0x00AF92000000FFFF          # 0x20 64-bit data
tramp_gdt_end:

.align 4
tramp_gdtptr:
    .word tramp_gdt_end - tramp_gdt - 1
    .long TRAMP + (tramp_gdt - ap_trampoline_start)

# -- Runtime-patched slots ---------------------------------------------------
.align 8
ap_tramp_cr3:
tramp_cr3:    .long 0
.align 8
ap_tramp_entry:
tramp_entry:  .quad 0
.align 8
ap_tramp_stack:
tramp_stack:  .quad 0

ap_trampoline_end:
