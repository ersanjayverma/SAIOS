#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static uintptr_t heap_end;

void *malloc(size_t size) {
    if (size == 0) {
        size = 1;
    }
    size = (size + 15) & ~(size_t)15;
    if (heap_end == 0) {
        void *base = sbrk(0);
        if (base == (void *)-1) {
            return NULL;
        }
        heap_end = (uintptr_t)base;
    }
    void *old = (void *)heap_end;
    if (sbrk((long)size) == (void *)-1) {
        return NULL;
    }
    heap_end += size;
    return old;
}

void free(void *ptr) {
    (void)ptr;
}

void *calloc(size_t count, size_t size) {
    size_t total = count * size;
    void *ptr = malloc(total);
    if (ptr) {
        memset(ptr, 0, total);
    }
    return ptr;
}

void abort(void) {
    _exit(134);
}

void exit(int code) {
    _exit(code);
}
