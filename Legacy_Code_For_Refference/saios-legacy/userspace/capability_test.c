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

static long long syscall3(long nr, long a0, long a1, long a2) {
    long long ret;
    __asm__ volatile("syscall" : "=a"(ret) : "a"(nr), "D"(a0), "S"(a1), "d"(a2) : "rcx", "r11", "memory");
    return ret;
}

static void sys_exit(int code) {
    syscall1(60, code);
    for (;;) { }
}

static int expect_eq(const char *name, long long actual, long long expected) {
    puts_raw("[capabilitytest] case ");
    puts_raw(name);
    puts_raw("\r\n");
    if (actual == expected) {
        return 1;
    }
    puts_raw("[capabilitytest] FAIL: unexpected return\r\n");
    return 0;
}

static int expect_negative(const char *name, long long actual) {
    puts_raw("[capabilitytest] case ");
    puts_raw(name);
    puts_raw("\r\n");
    if (actual < 0) {
        return 1;
    }
    puts_raw("[capabilitytest] FAIL: privileged operation was allowed\r\n");
    return 0;
}

static int expect_nonnegative(const char *name, long long actual) {
    puts_raw("[capabilitytest] case ");
    puts_raw(name);
    puts_raw("\r\n");
    if (actual >= 0) {
        return 1;
    }
    puts_raw("[capabilitytest] FAIL: expected success\r\n");
    return 0;
}

void _start(void) {
    static const char path[] = "/tmp/capability-test-file";
    const long o_creat = 0o100;
    const long o_rdwr = 2;
    const long o_trunc = 0o1000;
    int ok = 1;

    puts_raw("[capabilitytest] start\r\n");
    ok &= expect_eq("initial-euid-root", syscall0(107), 0);
    ok &= expect_eq("drop-setuid-1000", syscall1(105, 1000), 0);
    ok &= expect_eq("uid-dropped", syscall0(102), 1000);
    ok &= expect_eq("euid-dropped", syscall0(107), 1000);
    ok &= expect_negative("setuid-root-denied", syscall1(105, 0));
    ok &= expect_negative("setgid-root-denied", syscall1(106, 0));
    ok &= expect_negative("setreuid-root-denied", syscall2(113, 0, 0));

    long long fd = syscall3(2, (long)path, o_creat | o_rdwr | o_trunc, 0666);
    if (expect_nonnegative("tmpfs-create-as-user", fd)) {
        syscall1(3, fd);
    } else {
        ok = 0;
    }
    ok &= expect_eq("owner-chmod-allowed", syscall2(90, (long)path, 0600), 0);
    ok &= expect_negative("nonroot-chown-denied", syscall3(92, (long)path, 0, 0));
    syscall1(87, (long)path);

    if (ok) {
        puts_raw("[capabilitytest] PASS: capability enforcement boundaries verified\r\n");
        sys_exit(0);
    }
    puts_raw("[capabilitytest] FAIL: capability enforcement gate failed\r\n");
    sys_exit(1);
}