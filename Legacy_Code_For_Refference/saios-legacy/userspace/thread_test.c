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

static long long sys_clone(unsigned long flags, void *stack, int *parent_tid, int *child_tid, void *tls) {
    long long ret;
    register long r10 __asm__("r10") = (long)child_tid;
    register long r8 __asm__("r8") = (long)tls;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"((long)56), "D"((long)flags), "S"((long)stack), "d"((long)parent_tid), "r"(r10), "r"(r8)
        : "rcx", "r11", "memory"
    );
    return ret;
}

static void sys_exit(int code) {
    __asm__ volatile("syscall" : : "a"((long)60), "D"((long)code) : "rcx", "r11", "memory");
    for (;;) { }
}

static volatile int started;
static volatile int child_tid_slot = 1;
static unsigned char child_stack[16384] __attribute__((aligned(16)));

void _start(void) {
    const unsigned long CLONE_VM = 0x00000100;
    const unsigned long CLONE_FS = 0x00000200;
    const unsigned long CLONE_FILES = 0x00000400;
    const unsigned long CLONE_SIGHAND = 0x00000800;
    const unsigned long CLONE_THREAD = 0x00010000;
    const unsigned long CLONE_CHILD_CLEARTID = 0x00200000;
    unsigned long flags = CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND | CLONE_THREAD | CLONE_CHILD_CLEARTID;

    puts_raw("[threadtest] start\r\n");
    void *stack_top = child_stack + sizeof(child_stack) - 16;
    long long tid = sys_clone(flags, stack_top, 0, (int *)&child_tid_slot, 0);
    if (tid == 0) {
        started = 1;
        sys_exit(0);
    }
    if (tid < 0) {
        puts_raw("[threadtest] FAIL: clone(CLONE_THREAD) failed\r\n");
        sys_exit(1);
    }

    for (unsigned long i = 0; i < 10000000UL && started == 0; i++) {
        __asm__ volatile("pause");
    }
    if (started != 1) {
        puts_raw("[threadtest] FAIL: child thread did not start deterministically\r\n");
        sys_exit(2);
    }

    for (unsigned long i = 0; i < 10000000UL && child_tid_slot != 0; i++) {
        __asm__ volatile("pause");
    }
    if (child_tid_slot != 0) {
        puts_raw("[threadtest] FAIL: child-thread exit was not parent-visible\r\n");
        sys_exit(3);
    }
    puts_raw("[threadtest] PASS: clone thread startup and child exit visible\r\n");
    sys_exit(0);
}
