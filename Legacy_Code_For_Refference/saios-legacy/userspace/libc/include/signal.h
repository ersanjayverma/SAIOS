#ifndef SIGNAL_H
#define SIGNAL_H

#define SIGINT 2
#define SIGTERM 15
#define SIGSEGV 11

int kill(int pid, int sig);

#endif
