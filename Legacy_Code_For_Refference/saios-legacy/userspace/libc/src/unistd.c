#include <fcntl.h>
#include <signal.h>
#include <saios/syscall.h>
#include <sys/mman.h>
#include <unistd.h>

ssize_t read(int fd, void *buf, size_t len) {
    return (ssize_t)__saios_ret(__saios_syscall3(SYS_read, fd, (long)buf, (long)len));
}

ssize_t write(int fd, const void *buf, size_t len) {
    return (ssize_t)__saios_ret(__saios_syscall3(SYS_write, fd, (long)buf, (long)len));
}

int open(const char *path, int flags, ...) {
    return (int)__saios_ret(__saios_syscall3(SYS_open, (long)path, flags, 0644));
}

int close(int fd) {
    return (int)__saios_ret(__saios_syscall1(SYS_close, fd));
}

pid_t fork(void) {
    return (pid_t)__saios_ret(__saios_syscall0(SYS_fork));
}

int execve(const char *path, char *const argv[], char *const envp[]) {
    return (int)__saios_ret(__saios_syscall3(SYS_execve, (long)path, (long)argv, (long)envp));
}

pid_t wait4(pid_t pid, int *status, int options, void *rusage) {
    return (pid_t)__saios_ret(__saios_syscall4(SYS_wait4, pid, (long)status, options, (long)rusage));
}

pid_t getpid(void) {
    return (pid_t)__saios_ret(__saios_syscall0(SYS_getpid));
}

int kill(int pid, int sig) {
    return (int)__saios_ret(__saios_syscall2(SYS_kill, pid, sig));
}

void *mmap(void *addr, size_t len, int prot, int flags, int fd, off_t off) {
    long ret = __saios_ret(__saios_syscall6(SYS_mmap, (long)addr, (long)len, prot, flags, fd, off));
    return ret < 0 ? MAP_FAILED : (void *)ret;
}

int munmap(void *addr, size_t len) {
    return (int)__saios_ret(__saios_syscall2(SYS_munmap, (long)addr, (long)len));
}

int mprotect(void *addr, size_t len, int prot) {
    return (int)__saios_ret(__saios_syscall3(SYS_mprotect, (long)addr, (long)len, prot));
}

void _exit(int code) {
    __saios_syscall1(SYS_exit, code);
    for (;;) {
        __asm__ volatile("hlt");
    }
}

int brk(void *addr) {
    long ret = __saios_syscall1(SYS_brk, (long)addr);
    if (ret < 0) {
        return (int)__saios_ret(ret);
    }
    return ret == (long)addr ? 0 : -1;
}

void *sbrk(long increment) {
    long current = __saios_syscall1(SYS_brk, 0);
    if (current < 0) {
        __saios_ret(current);
        return (void *)-1;
    }
    if (increment == 0) {
        return (void *)current;
    }
    long next = current + increment;
    long ret = __saios_syscall1(SYS_brk, next);
    if (ret != next) {
        __saios_ret(ret < 0 ? ret : -12);
        return (void *)-1;
    }
    return (void *)current;
}
