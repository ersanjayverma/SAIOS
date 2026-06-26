#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char **argv, char **envp) {
    (void)envp;
    printf("[libctest] hello from GCC-built libc program argc=%d\n", argc);
    if (argc < 1 || !argv || !argv[0]) {
        puts("[libctest] FAIL: startup argv missing");
        return 1;
    }

    char *buf = malloc(64);
    if (!buf) {
        puts("[libctest] FAIL: malloc failed");
        return 2;
    }
    memset(buf, 0, 64);
    strcpy(buf, "malloc/string/write path works");
    if (strcmp(buf, "malloc/string/write path works") != 0) {
        puts("[libctest] FAIL: string functions failed");
        return 3;
    }
    if (write(1, "[libctest] write syscall works\n", 31) != 31) {
        puts("[libctest] FAIL: write failed");
        return 4;
    }
    free(buf);
    puts("[libctest] PASS: startup stdio malloc string write exit");
    return 0;
}
