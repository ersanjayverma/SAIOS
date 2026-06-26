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

static long long syscall0(long nr) {
    long long ret;
    __asm__ volatile("syscall" : "=a"(ret) : "a"(nr) : "rcx", "r11", "memory");
    return ret;
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

static void sys_exit(int code) {
    syscall1(60, code);
    for (;;) { }
}

static int expect_eq(const char *name, long long actual, long long expected) {
    puts_raw("[syscallabi] case ");
    puts_raw(name);
    puts_raw("\r\n");
    if (actual == expected) {
        return 1;
    }
    puts_raw("[syscallabi] FAIL: unexpected return\r\n");
    return 0;
}

static int expect_positive(const char *name, long long actual) {
    puts_raw("[syscallabi] case ");
    puts_raw(name);
    puts_raw("\r\n");
    if (actual > 0) {
        return 1;
    }
    puts_raw("[syscallabi] FAIL: expected positive return\r\n");
    return 0;
}

void _start(void) {
    int ok = 1;
    int pipefd[2] = { -1, -1 };
    puts_raw("[syscallabi] start\r\n");

    ok &= expect_positive("getpid-positive", syscall0(39));
    ok &= expect_eq("close-ebadf", syscall1(3, -1), -9);
    ok &= expect_eq("pipe2-null-efault", syscall2(293, 0, 0), -14);
    ok &= expect_eq("pipe2-unsupported-flag-einval", syscall2(293, (long)pipefd, 0x800), -22);
    ok &= expect_eq("select-unsupported-enosys", syscall0(23), -38);
    ok &= expect_eq("unknown-unsupported-enosys", syscall0(999), -38);

    if (ok) {
        puts_raw("[syscallabi] PASS: syscall ABI return and errno behavior verified\r\n");
        sys_exit(0);
    }
    puts_raw("[syscallabi] FAIL: syscall ABI conformance gate failed\r\n");
    sys_exit(1);
}