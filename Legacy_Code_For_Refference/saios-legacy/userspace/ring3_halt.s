# Ring 3 execution proof — HLT probe.
#
# HLT is a privileged instruction. In Ring 3 it generates #GP.
# If the #GP handler sees CPL=3 and the faulting opcode is 0xF4 (HLT),
# that PROVES the CPU entered Ring 3 and executed at least one user
# instruction — without any I/O, syscalls, stack access, or globals.
#
# Expected serial output from the #GP handler:
#   [#GP] error=0x0 rip=<user_addr> cs=0x23 cpl=3
# The CPL=3 line is the proof.

    .section .text
    .global _start
_start:
    hlt
