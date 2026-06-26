#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv, char **envp) {
    (void)envp;
    printf("hello from SAIOS libc argc=%d\n", argc);
    if (argc > 0) {
        printf("argv0=%s\n", argv[0]);
    }
    char *buf = malloc(32);
    if (!buf) {
        puts("malloc failed");
        return 2;
    }
    memcpy(buf, "malloc works", 13);
    puts(buf);
    write(1, "write works\n", 12);
    return 0;
}
