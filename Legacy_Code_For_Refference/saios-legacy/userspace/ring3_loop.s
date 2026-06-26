# Minimal ring-3 transition probe.
#
# This binary intentionally performs no syscalls, no memory references after
# entry, no global data access, no port I/O, and no privileged instructions.
# If SAIOS reaches this loop, the CPL0->CPL3 transition, entry RIP mapping, and
# instruction fetch path are working independently of syscall and libc code.

    .section .text
    .global _start
_start:
1:
    jmp 1b
