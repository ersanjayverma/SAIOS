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

static long long syscall4(long nr, long a0, long a1, long a2, long a3) {
    register long long r10 __asm__("r10") = a3;
    long long ret;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"(nr), "D"(a0), "S"(a1), "d"(a2), "r"(r10)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static void sys_exit(int code) {
    syscall1(60, code);
    for (;;) { }
}

void _start(void) {
    puts_raw("[waitreap] start\r\n");

    int status = 0x5555;
    long long before = syscall4(61, -1, (long)&status, 1, 0);
    if (before != 0) {
        puts_raw("[waitreap] FAIL: WNOHANG before child did not return 0\r\n");
        sys_exit(1);
    }

    long long pid = syscall1(57, 0);
    if (pid == 0) {
        sys_exit(42);
    }
    if (pid < 0) {
        puts_raw("[waitreap] FAIL: fork failed\r\n");
        sys_exit(2);
    }

    status = 0;
    long long waited = syscall4(61, (long)pid, (long)&status, 0, 0);
    if (waited != pid) {
        puts_raw("[waitreap] FAIL: wait4 returned wrong pid\r\n");
        sys_exit(3);
    }
    if (status != (42 << 8)) {
        puts_raw("[waitreap] FAIL: wait4 status mismatch\r\n");
        sys_exit(4);
    }

    status = 0x7777;
    long long second = syscall4(61, (long)pid, (long)&status, 1, 0);
    if (second != 0) {
        puts_raw("[waitreap] FAIL: WNOHANG second reap did not return 0\r\n");
        sys_exit(5);
    }

    puts_raw("[waitreap] PASS: wait4 WNOHANG, status, and single reap verified\r\n");
    sys_exit(0);
}