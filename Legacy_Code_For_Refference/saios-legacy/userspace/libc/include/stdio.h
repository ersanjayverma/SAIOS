#ifndef SAIOS_STDIO_H
#define SAIOS_STDIO_H

#include <stddef.h>

#define EOF (-1)

int putchar(int ch);
int puts(const char *s);
int printf(const char *fmt, ...);

#endif
#ifndef STDIO_H
#define STDIO_H

#include <stdarg.h>
#include <sys/types.h>

#define EOF (-1)

int putchar(int ch);
int puts(const char *s);
int printf(const char *fmt, ...);
size_t fwrite(const void *ptr, size_t size, size_t nmemb, void *stream);

#endif
