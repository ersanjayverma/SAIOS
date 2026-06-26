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

static long long syscall4(long nr, long a0, long a1, long a2, long a3) {
    long long ret;
    register long r10 __asm__("r10") = a3;
    __asm__ volatile("syscall" : "=a"(ret) : "a"(nr), "D"(a0), "S"(a1), "d"(a2), "r"(r10) : "rcx", "r11", "memory");
    return ret;
}

static long long syscall6(long nr, long a0, long a1, long a2, long a3, long a4, long a5) {
    long long ret;
    register long r10 __asm__("r10") = a3;
    register long r8 __asm__("r8") = a4;
    register long r9 __asm__("r9") = a5;
    __asm__ volatile("syscall" : "=a"(ret) : "a"(nr), "D"(a0), "S"(a1), "d"(a2), "r"(r10), "r"(r8), "r"(r9) : "rcx", "r11", "memory");
    return ret;
}

static void sys_exit(int code) {
    syscall1(60, code);
    for (;;) { }
}

static int expect_child_fault(void (*fn)(void), const char *name) {
    puts_raw("[memperm] case ");
    puts_raw(name);
    puts_raw("\r\n");

    long long pid = syscall0(57);
    if (pid == 0) {
        fn();
        puts_raw("[memperm] FAIL: protected access returned normally\r\n");
        sys_exit(90);
    }
    if (pid < 0) {
        puts_raw("[memperm] FAIL: fork failed\r\n");
        return 0;
    }

    int status = 0;
    long long waited = syscall4(61, pid, (long)&status, 0, 0);
    if (waited != pid) {
        puts_raw("[memperm] FAIL: wait4 failed\r\n");
        return 0;
    }
    if (status == (90 << 8) || status == 0) {
        puts_raw("[memperm] FAIL: child did not fault\r\n");
        return 0;
    }
    puts_raw("[memperm] PASS: child faulted as expected\r\n");
    return 1;
}

static void rodata_write(void) {
    static const char msg[] = "read only";
    volatile char *p = (volatile char *)msg;
    *p = 'X';
}

static void mmap_readonly_write(void) {
    char *p = (char *)syscall6(9, 0, 4096, 1, 0x22, -1, 0);
    if ((long long)p < 0) {
        puts_raw("[memperm] FAIL: mmap(PROT_READ) failed\r\n");
        sys_exit(91);
    }
    p[0] = 'X';
}

static void mprotect_readonly_write(void) {
    char *p = (char *)syscall6(9, 0, 4096, 3, 0x22, -1, 0);
    if ((long long)p < 0) {
        puts_raw("[memperm] FAIL: mmap(RW) failed\r\n");
        sys_exit(92);
    }
    p[0] = 'A';
    if (syscall3(10, (long)p, 4096, 1) != 0) {
        puts_raw("[memperm] FAIL: mprotect(PROT_READ) failed\r\n");
        sys_exit(93);
    }
    p[0] = 'B';
}

static void nx_execute(void) {
    unsigned char *p = (unsigned char *)syscall6(9, 0, 4096, 3, 0x22, -1, 0);
    if ((long long)p < 0) {
        puts_raw("[memperm] FAIL: mmap(RW) for NX failed\r\n");
        sys_exit(94);
    }
    p[0] = 0xc3;
    void (*fn)(void) = (void (*)(void))p;
    fn();
}

void _start(void) {
    int ok = 1;
    puts_raw("[memperm] start\r\n");
    ok &= expect_child_fault(rodata_write, "rodata-write");
    ok &= expect_child_fault(mmap_readonly_write, "mmap-prot-read-write");
    ok &= expect_child_fault(mprotect_readonly_write, "mprotect-read-write");
    ok &= expect_child_fault(nx_execute, "nx-execute");
    if (ok) {
        puts_raw("[memperm] PASS: memory permissions enforced\r\n");
        sys_exit(0);
    }
    puts_raw("[memperm] FAIL: memory permission gate failed\r\n");
    sys_exit(1);
}
