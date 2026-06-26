#ifndef SAIOS_STDLIB_H
#define SAIOS_STDLIB_H

#include <stddef.h>

void *malloc(size_t size);
void free(void *ptr);
void *calloc(size_t count, size_t size);
void abort(void) __attribute__((noreturn));
void exit(int code) __attribute__((noreturn));

#endif
#ifndef STDLIB_H
#define STDLIB_H

#include <sys/types.h>

void exit(int status);
void *malloc(size_t size);
void free(void *ptr);
void abort(void);

#endif
