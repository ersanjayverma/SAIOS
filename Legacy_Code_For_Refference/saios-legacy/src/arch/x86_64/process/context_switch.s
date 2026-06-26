# process/switch.s — kernel-mode context switch for preemptive scheduling.
#
# Calling convention (System V AMD64):
#   switch_context(from_rsp: *mut u64, to_rsp: *const u64)
#                  rdi                  rsi
#
# Saves the current kernel context (callee-saved regs + rip via call-return)
# onto the current stack, writes the new stack pointer to *from_rsp,
# then switches to the new stack and returns into the next process's context.
#
# Each process's kernel stack looks like (from top, growing down):
#   [rip of resumed instruction]   ← pushed by call to switch_context
#   [r15]
#   [r14]
#   [r13]
#   [r12]
#   [rbx]
#   [rbp]
#   [rflags]                       ← saved IF etc.; restored per-thread
#   ← kernel_rsp points here after saving
#
# RFLAGS MUST be saved/restored: a thread may yield cooperatively (IF=1) but be
# resumed by a timer-IRQ-driven switch (IF=0).  Without restoring RFLAGS it would
# return with IF=0 and then `hlt` forever (interrupts disabled → never woken).

.section .text.switch, "ax"
.code64
.global switch_context
.global switch_context_nosave
.global switch_to_user

switch_context:
    # Save callee-saved registers onto current stack
    push  %r15
    push  %r14
    push  %r13
    push  %r12
    push  %rbx
    push  %rbp
    pushfq                # save RFLAGS (incl. IF) last → top of frame

    # Save current RSP into *from_rsp
    movq  %rsp, (%rdi)

    # Load new RSP from *to_rsp
    movq  (%rsi), %rsp

    # Restore RFLAGS and callee-saved registers from new stack
    popfq                 # restore the new thread's RFLAGS (its own IF)
    pop   %rbp
    pop   %rbx
    pop   %r12
    pop   %r13
    pop   %r14
    pop   %r15

    # Return into the new process's kernel context (rip was pushed by its call)
    ret

switch_context_nosave:
    # Fatal exception handoff: the current stack is an IDT frame, not a
    # resumable kernel scheduling frame. Load the next saved context without
    # writing anything back to the exiting process.
    movq  (%rdi), %rsp

    popfq
    pop   %rbp
    pop   %rbx
    pop   %r12
    pop   %r13
    pop   %r14
    pop   %r15
    ret

# -- switch_to_user: resume a process in ring 3 via SYSRETQ ----------------
#
# switch_to_user(rip: u64, rsp: u64, rflags: u64, rax: u64)
#                rdi        rsi         rdx          rcx
#
# Sets up registers and executes SYSRETQ to enter user mode.

# -- kthread_trampoline: first-run entry for a kernel thread ---------------
#
# A freshly-spawned kernel thread's stack is crafted so switch_context's `ret`
# lands here, with %rbx = entry fn and %r12 = arg.  First-run contexts have not
# returned to scheduler::schedule(), so they must run finish_switch bookkeeping
# before enabling interrupts and entering the thread body.
.global kthread_trampoline
kthread_trampoline:
    call  kthread_finish_switch_current
    sti
    movq  %r12, %rdi      # arg -> first parameter
    call  *%rbx           # entry_fn(arg)
    call  kthread_exit_current   # never returns
1:  hlt
    jmp   1b

# -- ksetjmp / klongjmp: kernel setjmp/longjmp -----------------------------
#
# Used to make userspace exec resumable: run_current() ksetjmp's the shell
# thread's kernel context before iretq'ing into ring 3, and terminate() (on
# sys_exit) klongjmp's back so control returns to the shell instead of halting.
#
#   ksetjmp(buf: *mut [u64; 9]) -> u64        (rdi=buf; returns 0 directly)
#   klongjmp(buf: *const [u64; 9], val: u64)  (rdi=buf, rsi=val; ksetjmp
#                                               appears to return `val`, or 1)
# buf layout: [rbx, rbp, r12, r13, r14, r15, rflags, rsp, rip]
#
# RFLAGS MUST be saved/restored: a thread may run with IF=0 (interrupts disabled)
# and we must restore it correctly when returning from userspace. Without this,
# the timer IRQ stops firing and the system freezes.
.global ksetjmp
ksetjmp:
    pushfq                    # save RFLAGS on stack
    popq  48(%rdi)            # save RFLAGS at offset 48
    movq  %rbx,  0(%rdi)
    movq  %rbp,  8(%rdi)
    movq  %r12, 16(%rdi)
    movq  %r13, 24(%rdi)
    movq  %r14, 32(%rdi)
    movq  %r15, 40(%rdi)
    leaq  8(%rsp), %rax       # caller's rsp (after our ret pops the return addr)
    movq  %rax, 56(%rdi)
    movq  (%rsp), %rax        # return address
    movq  %rax, 64(%rdi)
    xorq  %rax, %rax          # direct call returns 0
    ret

.global klongjmp
klongjmp:
    movq   0(%rdi), %rbx
    movq   8(%rdi), %rbp
    movq  16(%rdi), %r12
    movq  24(%rdi), %r13
    movq  32(%rdi), %r14
    movq  40(%rdi), %r15
    movq  48(%rdi), %rax      # load saved RFLAGS
    pushq %rax                # push it on stack
    popfq                     # restore RFLAGS
    movq  56(%rdi), %rsp
    movq  %rsi, %rax          # return value seen by ksetjmp
    testq %rax, %rax
    jnz   1f
    incq  %rax                # never return 0 from a longjmp
1:  jmpq  *64(%rdi)           # resume just after ksetjmp

# -- switch_to_user: resume a process in ring 3 via SYSRETQ ----------------
switch_to_user:
    # SYSRETQ needs: RCX = user RIP, R11 = user RFLAGS
    movq  %rdi, %rcx      # user RIP
    movq  %rdx, %r11      # user RFLAGS
    movq  %rsi, %rsp      # user RSP
    movq  %rcx, %rax      # return value (fork: 0 for child)
    # rcx already holds rip for sysretq — note: caller sets rax via 4th param
    movq  (8*3)(%rsp), %rax   # actually load rax from 4th arg... hmm
    # Simpler: rax is 4th arg (rcx in SysV) — but we clobbered rcx.
    # Use r10 as temp:
    movq  %rdi, %r10      # save user rip
    movq  %rcx, %rax      # rax = 4th arg (was in rcx before we clobbered it)
    movq  %r10, %rcx      # rcx = user rip (for sysretq)
    sysretq
