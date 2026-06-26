# SAIOS Phase 5.0a — minimal static userspace test program.
# Pure syscalls, no libc: write(1, msg, len) then exit(0).
# Built static (ld -static -nostdlib -e _start) and run via process::spawn to
# verify the ring-3 exec path: ELF load, user stack, SYSCALL, and clean exit.

    .section .text
    .global _start
_start:
    movq $1, %rax            # SYS_write
    movq $1, %rdi            # fd = stdout
    leaq msg(%rip), %rsi     # buf
    movq $msglen, %rdx       # count
    syscall

    movq $60, %rax           # SYS_exit
    movq $0, %rdi            # status 0
    syscall

    .section .rodata
msg:
    .ascii "hello from ring 3 - SAIOS userspace works!\n"
    .equ msglen, . - msg
