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

static long long sys_write(long fd, const char *s, unsigned long long len) {
    long long ret;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"((long long)1), "D"(fd), "S"((long long)s), "d"((long long)len)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static long long sys_exit(long code) {
    long long ret;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"((long long)60), "D"(code)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static long long saios_internal_shell(void) {
    long long ret;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"((long long)0x80000007)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static unsigned long long strlen_cap(const char *s) {
    unsigned long long n = 0;
    while (s[n] && n < 4096) {
        n++;
    }
    return n;
}

static void puts_both(const char *s) {
    unsigned long long len = strlen_cap(s);
    sys_write(1, s, len);
    saios_puts(s, len);
}

void _start(void) {
    puts_both("[userspace-shell] entered /bin/sh\r\n");
    saios_internal_shell();
    puts_both("[userspace-shell] internal shell returned\r\n");
    sys_exit(1);
    for (;;) { }
}