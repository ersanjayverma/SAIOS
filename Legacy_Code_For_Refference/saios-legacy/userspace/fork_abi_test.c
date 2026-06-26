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

static void serial_puts(const char *s) {
    unsigned long long n = 0;
    const char *p = s;
    while (*p && n < 4096) { p++; n++; }
    saios_puts(s, n);
}

static void serial_put_dec(unsigned long long value) {
    char buf[32];
    unsigned long long pos = 0;
    if (value == 0) {
        char zero = '0';
        saios_puts(&zero, 1);
        return;
    }
    while (value != 0 && pos < sizeof(buf)) {
        buf[pos++] = (char)('0' + (value % 10));
        value /= 10;
    }
    while (pos > 0) {
        pos--;
        saios_puts(&buf[pos], 1);
    }
}

static void serial_put_hex(unsigned long long value) {
    static const char hex[] = "0123456789abcdef";
    char buf[18];
    int i;
    buf[0] = '0';
    buf[1] = 'x';
    for (i = 0; i < 16; i++) {
        unsigned long long shift = (unsigned long long)(15 - i) * 4;
        buf[2 + i] = hex[(value >> shift) & 0xF];
    }
    saios_puts(buf, sizeof(buf));
}

static void serial_put_kv_dec(const char *label, unsigned long long value) {
    serial_puts(label);
    serial_put_dec(value);
    serial_puts("\r\n");
}

static void serial_put_kv_hex(const char *label, unsigned long long value) {
    serial_puts(label);
    serial_put_hex(value);
    serial_puts("\r\n");
}

static long long sys_fork(void) {
    long long ret;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"((long long)57)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static long long sys_wait4(int pid, int *status, int options, void *rusage) {
    register long long r10 __asm__("r10") = (long long)(unsigned long long)rusage;
    long long ret;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"((long long)61), "D"((long long)pid), "S"((long long)status),
          "d"((long long)options), "r"(r10)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static void sys_exit(int code) {
    __asm__ volatile(
        "syscall"
        :
        : "a"((long long)60), "D"((long long)code)
        : "rcx", "r11", "memory"
    );
    for (;;) { }
}

void _start(void) {
    serial_puts("[forkabi] start\r\n");
    long long pid = sys_fork();
    serial_put_kv_dec("[forkabi] fork_ret=", (unsigned long long)pid);

    if (pid == 0) {
        serial_puts("[forkabi] child branch entered\r\n");
        sys_exit(37);
    }

    if (pid < 0) {
        serial_puts("[forkabi] FAIL: fork failed\r\n");
        sys_exit(1);
    }

    int status = 0;
    long long waited = sys_wait4((int)pid, &status, 0, (void*)0);
    serial_put_kv_dec("[forkabi] parent fork_ret=", (unsigned long long)pid);
    serial_put_kv_dec("[forkabi] waited=", (unsigned long long)waited);
    serial_put_kv_hex("[forkabi] status=", (unsigned long long)(unsigned int)status);

    if (waited != pid) {
        serial_puts("[forkabi] FAIL: wait4 pid mismatch\r\n");
        sys_exit(2);
    }
    if (status != (37 << 8)) {
        serial_puts("[forkabi] FAIL: child status mismatch\r\n");
        sys_exit(3);
    }

    serial_puts("[forkabi] PASS: parent saw child pid and child saw zero\r\n");
    sys_exit(0);
}
