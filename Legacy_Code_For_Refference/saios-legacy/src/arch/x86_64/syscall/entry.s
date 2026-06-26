# syscall/entry.s — SYSCALL/SYSRETQ trampoline (AT&T syntax)
#
# On SYSCALL entry:
#   %rax = syscall number
#   %rdi, %rsi, %rdx, %r10, %r8, %r9 = arguments
#   %rcx = saved user RIP  (by CPU)
#   %r11 = saved user RFLAGS (by CPU)
#   %rsp = user stack

.section .text.syscall, "ax"
.code64

.global syscall_entry

syscall_entry:
    swapgs
    # Save the CPU-provided user return image before using any scratch regs.
    # Do not push here: %rsp is still the user stack until we load %gs:0.
    movq  %rsp, %gs:8
    movq  %rcx, %gs:16
    movq  %r11, %gs:24
    movq  %gs:0, %r11
    movq  %r11, %gs:128
    movq  %gs:8, %r11
    movq  %r11, %gs:136
    movq  %gs:16, %r11
    movq  %r11, %gs:144
    # --- switch to kernel stack ---
    movq  %gs:0, %rsp

    # --- snapshot user registers for fork/clone child setup ---
    # SYSCALL itself clobbers RCX/R11.  Linux-compatible fork children must
    # resume with the rest of the register image preserved and RAX = 0.
    movq  %rdi, %gs:32
    movq  %rsi, %gs:40
    movq  %rdx, %gs:48
    movq  %r8,  %gs:56
    movq  %r9,  %gs:64
    movq  %r10, %gs:72
    movq  %rbx, %gs:80
    movq  %rbp, %gs:88
    movq  %r12, %gs:96
    movq  %r13, %gs:104
    movq  %r14, %gs:112
    movq  %r15, %gs:120

    # --- build context frame on kernel stack ---
    # Keep the syscall's user RSP in this process's kernel frame. Blocking
    # syscalls can schedule another process, which may overwrite the per-CPU
    # scratch slot before this syscall returns.
    movq  %gs:8, %r10
    movq  %gs:16, %rcx
    movq  %gs:24, %r11
    pushq %r10
    pushq %rcx           # user RIP
    pushq %r11           # user RFLAGS
    pushq %rbp
    pushq %rbx
    pushq %r12
    pushq %r13
    pushq %r14
    pushq %r15

    # --- set up syscall_dispatch(num, a1, a2, a3, a4, a5, a6) ---
    # Linux  in : rax=num, rdi=a1, rsi=a2, rdx=a3, r10=a4, r8=a5, r9=a6
    # SysV  call: rdi=num, rsi=a1, rdx=a2, rcx=a3, r8=a4, r9=a5, [stack]=a6
    # Order matters to avoid clobbering a source before it is read.
    movq  %r9,  %r11          # stash a6 (r11 was already saved on the stack)
    movq  %r8,  %r9           # r9 = a5  (Linux r8)
    movq  %r10, %r8           # r8 = a4  (Linux r10)
    movq  %rdx, %rcx          # rcx = a3
    movq  %rsi, %rdx          # rdx = a2
    movq  %rdi, %rsi          # rsi = a1
    movq  %rax, %rdi          # rdi = syscall number
    # a6 goes on the stack (7th SysV arg).  The 9 saved-frame pushes above leave
    # rsp 8-byte misaligned; pushing a6 brings it back to the SysV call alignment
    # and the callee sees a6 at [rsp+8].
    pushq %r11                # a6
    callq syscall_dispatch
    addq  $8,   %rsp          # drop a6

    # --- restore saved registers ---
    popq  %r15
    popq  %r14
    popq  %r13
    popq  %r12
    popq  %rbx
    popq  %rbp
    popq  %r11           # user RFLAGS (needed by sysretq)
    popq  %rcx           # user RIP    (needed by sysretq)
    popq  %r10           # user RSP saved in this syscall frame

    # --- restore user stack and return ---
    movq  %r10, %rsp
    swapgs
    sysretq
