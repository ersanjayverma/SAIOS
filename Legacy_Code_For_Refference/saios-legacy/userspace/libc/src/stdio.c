#include <stdarg.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

static int print_str(const char *s) {
    size_t len = strlen(s);
    return write(1, s, len) < 0 ? EOF : (int)len;
}

static int print_unsigned(unsigned long value, unsigned int base) {
    char buf[32];
    const char digits[] = "0123456789abcdef";
    int pos = 0;
    if (value == 0) {
        return putchar('0') == EOF ? EOF : 1;
    }
    while (value && pos < (int)sizeof(buf)) {
        buf[pos++] = digits[value % base];
        value /= base;
    }
    int written = 0;
    while (pos > 0) {
        if (putchar(buf[--pos]) == EOF) {
            return EOF;
        }
        written++;
    }
    return written;
}

int putchar(int ch) {
    unsigned char byte = (unsigned char)ch;
    return write(1, &byte, 1) == 1 ? ch : EOF;
}

int puts(const char *s) {
    int ret = print_str(s);
    if (ret == EOF || putchar('\n') == EOF) {
        return EOF;
    }
    return ret + 1;
}

int printf(const char *fmt, ...) {
    va_list ap;
    int written = 0;
    va_start(ap, fmt);
    for (const char *p = fmt; *p; p++) {
        if (*p != '%') {
            if (putchar(*p) == EOF) {
                va_end(ap);
                return EOF;
            }
            written++;
            continue;
        }
        p++;
        int ret = 0;
        if (*p == 's') {
            const char *s = va_arg(ap, const char *);
            ret = print_str(s ? s : "(null)");
        } else if (*p == 'd') {
            long value = va_arg(ap, int);
            if (value < 0) {
                if (putchar('-') == EOF) {
                    va_end(ap);
                    return EOF;
                }
                written++;
                value = -value;
            }
            ret = print_unsigned((unsigned long)value, 10);
        } else if (*p == 'u') {
            ret = print_unsigned(va_arg(ap, unsigned int), 10);
        } else if (*p == 'x') {
            ret = print_unsigned(va_arg(ap, unsigned int), 16);
        } else if (*p == 'p') {
            ret = print_str("0x");
            if (ret != EOF) {
                written += ret;
                ret = print_unsigned((unsigned long)va_arg(ap, void *), 16);
            }
        } else if (*p == '%') {
            ret = putchar('%') == EOF ? EOF : 1;
        } else {
            ret = putchar(*p) == EOF ? EOF : 1;
        }
        if (ret == EOF) {
            va_end(ap);
            return EOF;
        }
        written += ret;
    }
    va_end(ap);
    return written;
}
