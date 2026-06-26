#ifndef SAIOS_UNISTD_H
#define SAIOS_UNISTD_H

#include <stddef.h>
#include <sys/types.h>

ssize_t read(int fd, void *buf, size_t len);
ssize_t write(int fd, const void *buf, size_t len);
int close(int fd);
pid_t fork(void);
int execve(const char *path, char *const argv[], char *const envp[]);
pid_t getpid(void);
pid_t wait4(pid_t pid, int *status, int options, void *rusage);
void _exit(int code) __attribute__((noreturn));
void *sbrk(long increment);
int brk(void *addr);

#endif
