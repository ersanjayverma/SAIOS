#ifndef SAIOS_SYSCALL_H
#define SAIOS_SYSCALL_H

#include <stdint.h>

#define SYS_read 0
#define SYS_write 1
#define SYS_open 2
#define SYS_close 3
#define SYS_mmap 9
#define SYS_mprotect 10
#define SYS_munmap 11
#define SYS_brk 12
#define SYS_getpid 39
#define SYS_fork 57
#define SYS_execve 59
#define SYS_exit 60
#define SYS_wait4 61
#define SYS_kill 62

#define SAIOS_SYS_PUTS 0x80000001UL
#define SAIOS_SYS_PUTC 0x80000002UL

long __saios_syscall0(long nr);
long __saios_syscall1(long nr, long a0);
long __saios_syscall2(long nr, long a0, long a1);
long __saios_syscall3(long nr, long a0, long a1, long a2);
long __saios_syscall4(long nr, long a0, long a1, long a2, long a3);
long __saios_syscall5(long nr, long a0, long a1, long a2, long a3, long a4);
long __saios_syscall6(long nr, long a0, long a1, long a2, long a3, long a4, long a5);

long __saios_ret(long ret);

#endif
