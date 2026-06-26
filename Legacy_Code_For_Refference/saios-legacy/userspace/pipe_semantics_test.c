static long long saios_puts(const char *s, unsigned long long len) {
    long long ret;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"((long long)0x80000001), "D"((long long)1), "S"((long long)s), "d"((long long)len)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static void puts_raw(const char *s) {
    unsigned long long n = 0;
    while (s[n] && n < 4096) {
        n++;
    }
    saios_puts(s, n);
}

static long long syscall1(long nr, long a0) {
    long long ret;
    __asm__ volatile("syscall" : "=a"(ret) : "a"(nr), "D"(a0) : "rcx", "r11", "memory");
    return ret;
}

static long long syscall2(long nr, long a0, long a1) {
    long long ret;
    __asm__ volatile("syscall" : "=a"(ret) : "a"(nr), "D"(a0), "S"(a1) : "rcx", "r11", "memory");
    return ret;
}

static long long syscall3(long nr, long a0, long a1, long a2) {
    long long ret;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"(nr), "D"(a0), "S"(a1), "d"(a2)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static void sys_exit(int code) {
    syscall1(60, code);
    for (;;) { }
}

void _start(void) {
    puts_raw("[pipesem] start\r\n");

    int fds[2] = { -1, -1 };
    long long ret = syscall2(293, (long)fds, 0);
    if (ret != 0 || fds[0] < 0 || fds[1] < 0) {
        puts_raw("[pipesem] FAIL: pipe2 failed\r\n");
        sys_exit(1);
    }

    char msg[4] = { 'p', 'i', 'p', 'e' };
    ret = syscall3(1, fds[1], (long)msg, 4);
    if (ret != 4) {
        puts_raw("[pipesem] FAIL: pipe write length mismatch\r\n");
        sys_exit(2);
    }

    char buf[4] = { 0, 0, 0, 0 };
    ret = syscall3(0, fds[0], (long)buf, 4);
    if (ret != 4 || buf[0] != 'p' || buf[1] != 'i' || buf[2] != 'p' || buf[3] != 'e') {
        puts_raw("[pipesem] FAIL: pipe read data mismatch\r\n");
        sys_exit(3);
    }

    ret = syscall1(3, fds[1]);
    if (ret != 0) {
        puts_raw("[pipesem] FAIL: close writer failed\r\n");
        sys_exit(4);
    }

    ret = syscall3(0, fds[0], (long)buf, 1);
    if (ret != 0) {
        puts_raw("[pipesem] FAIL: read after writer close did not return EOF\r\n");
        sys_exit(5);
    }

    ret = syscall1(3, fds[0]);
    if (ret != 0) {
        puts_raw("[pipesem] FAIL: close reader failed\r\n");
        sys_exit(6);
    }

    int broken[2] = { -1, -1 };
    ret = syscall2(293, (long)broken, 0);
    if (ret != 0) {
        puts_raw("[pipesem] FAIL: second pipe2 failed\r\n");
        sys_exit(7);
    }
    syscall1(3, broken[0]);
    ret = syscall3(1, broken[1], (long)msg, 1);
    if (ret != -32) {
        puts_raw("[pipesem] FAIL: write without readers did not return EPIPE\r\n");
        sys_exit(8);
    }

    puts_raw("[pipesem] PASS: pipe read/write EOF and EPIPE verified\r\n");
    sys_exit(0);
}