#ifndef SAIOS_SYS_WAIT_H
#define SAIOS_SYS_WAIT_H

#include <sys/types.h>

pid_t wait4(pid_t pid, int *status, int options, void *rusage);

#define WEXITSTATUS(status) (((status) >> 8) & 0xff)
#define WIFEXITED(status) ((((status) & 0x7f) == 0))

#endif
