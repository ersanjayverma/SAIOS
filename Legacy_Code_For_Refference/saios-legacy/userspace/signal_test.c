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

static long long syscall4(long nr, long a0, long a1, long a2, long a3) {
    long long ret;
    register long r10 __asm__("r10") = a3;
    __asm__ volatile("syscall" : "=a"(ret) : "a"(nr), "D"(a0), "S"(a1), "d"(a2), "r"(r10) : "rcx", "r11", "memory");
    return ret;
}

static void sys_exit(int code) {
    syscall1(60, code);
    for (;;) { }
}

void _start(void) {
    puts_raw("[signaltest] start\r\n");
    long long pid = syscall0(57);
    if (pid == 0) {
        unsigned long req[2] = { 5, 0 };
        puts_raw("[signaltest] child sleeping\r\n");
        syscall2(35, (long)req, 0);
        puts_raw("[signaltest] FAIL: child returned from sleep after signal\r\n");
        sys_exit(90);
    }
    if (pid < 0) {
        puts_raw("[signaltest] FAIL: fork failed\r\n");
        sys_exit(1);
    }
    for (unsigned long i = 0; i < 1000000UL; i++) {
        __asm__ volatile("pause");
    }
    if (syscall2(62, pid, 15) != 0) {
        puts_raw("[signaltest] FAIL: kill(SIGTERM) failed\r\n");
        sys_exit(2);
    }
    int status = 0;
    long long waited = syscall4(61, pid, (long)&status, 0, 0);
    if (waited != pid) {
        puts_raw("[signaltest] FAIL: wait4 failed\r\n");
        sys_exit(3);
    }
    if (status == 0 || status == (90 << 8)) {
        puts_raw("[signaltest] FAIL: signal did not terminate blocked child\r\n");
        sys_exit(4);
    }
    puts_raw("[signaltest] PASS: targeted signal terminated blocked child\r\n");
    sys_exit(0);
}
