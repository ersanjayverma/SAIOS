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

static long long sys_clone(unsigned long flags, void *stack) {
    long long ret;
    register long r10 __asm__("r10") = 0;
    register long r8 __asm__("r8") = 0;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"((long)56), "D"((long)flags), "S"((long)stack), "d"((long)0), "r"(r10), "r"(r8)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static long long sys_futex(int *uaddr, int op, int val) {
    long long ret;
    register long r10 __asm__("r10") = 0;
    register long r8 __asm__("r8") = 0;
    register long r9 __asm__("r9") = 0;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"((long)202), "D"((long)uaddr), "S"((long)op), "d"((long)val), "r"(r10), "r"(r8), "r"(r9)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static void sys_exit(int code) {
    __asm__ volatile("syscall" : : "a"((long)60), "D"((long)code) : "rcx", "r11", "memory");
    for (;;) { }
}

static volatile int futex_word;
static volatile int child_started;
static unsigned char child_stack[16384] __attribute__((aligned(16)));

void _start(void) {
    const unsigned long flags = 0x00000100 | 0x00000200 | 0x00000400 | 0x00000800 | 0x00010000;
    puts_raw("[futextest] start\r\n");
    long long tid = sys_clone(flags, child_stack + sizeof(child_stack) - 16);
    if (tid == 0) {
        child_started = 1;
        for (unsigned long i = 0; i < 500000UL; i++) {
            __asm__ volatile("pause");
        }
        futex_word = 1;
        long long woke = sys_futex((int *)&futex_word, 129, 1);
        if (woke < 1) {
            puts_raw("[futextest] FAIL: child wake found no waiter\r\n");
            sys_exit(4);
        }
        sys_exit(0);
    }
    if (tid < 0) {
        puts_raw("[futextest] FAIL: clone failed\r\n");
        sys_exit(1);
    }
    while (!child_started) {
        __asm__ volatile("pause");
    }
    long long wait_ret = sys_futex((int *)&futex_word, 128, 0);
    if (wait_ret != 0 || futex_word != 1) {
        puts_raw("[futextest] FAIL: futex wait/wake did not complete\r\n");
        sys_exit(2);
    }
    puts_raw("[futextest] PASS: futex wait/wake completed\r\n");
    sys_exit(0);
}
