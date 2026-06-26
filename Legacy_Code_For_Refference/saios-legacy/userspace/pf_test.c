/*
 * pf_test.c - Test Page Fault (#PF) handling
 *
 * Dereferences invalid pointers which should cause #PF.
 * The kernel should terminate this process and return to the shell.
 */

#define SERIAL_PORT 0x3F8

/* SAIOS-native serial syscalls */
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

static long long saios_putc(unsigned char c) {
    long long ret;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"((long long)0x80000002), "D"((long long)1), "S"((unsigned long long)c)
        : "rcx", "r11", "memory"
    );
    return ret;
}

/* Serial I/O functions */
static void serial_write(unsigned char c) {
    saios_putc(c);
}

static void serial_puts(const char *s) {
    unsigned long long n = 0;
    const char *p = s;
    while (*p && n < 4096) { p++; n++; }
    saios_puts(s, n);
}

/* Syscall functions */
static long long sys_exit(int code) {
    long long ret;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"((long long)60), "D"((long long)code)
        : "rcx", "r11", "memory"
    );
    return ret;
}

/* Test: NULL pointer dereference */
void test_null_pointer(void) {
    serial_puts("[pf_test] Testing NULL pointer dereference...\r\n");

    /* This should cause #PF - NULL (0x0) is not mapped */
    volatile int *null_ptr = (volatile int *)0;
    *null_ptr = 42;

    serial_puts("[pf_test] FAIL: No #PF generated for NULL pointer! Kernel may not handle #PF properly.\r\n");
    sys_exit(1);
}

/* Test: Invalid low address dereference */
void test_invalid_low_address(void) {
    serial_puts("[pf_test] Testing invalid low address dereference...\r\n");

    /* Low addresses are typically not mapped in user space */
    volatile int *ptr = (volatile int *)0x1000;
    *ptr = 42;

    serial_puts("[pf_test] FAIL: No #PF generated for invalid address!\r\n");
    sys_exit(1);
}

/* Test: Kernel address dereference */
void test_kernel_address(void) {
    serial_puts("[pf_test] Testing kernel address dereference...\r\n");

    /* This should cause #PF - kernel addresses are not accessible from user mode */
    volatile int *ptr = (volatile int *)0xffffffff80000000ULL;
    *ptr = 42;

    serial_puts("[pf_test] FAIL: No #PF generated for kernel address access!\r\n");
    sys_exit(1);
}

/* Test: Stack pointer underflow */
void test_stack_underflow(void) {
    serial_puts("[pf_test] Testing stack underflow (accessing below RSP)...\r\n");

    /* Read from just below current stack pointer - should be unmapped */
    volatile int *ptr;
    __asm__ volatile(
        "movq %%rsp, %0"
        : "=r"(ptr)
    );
    /* Move below the stack pointer and read */
    ptr = (volatile int *)((char *)ptr - 0x1000);

    int val = *ptr;
    (void)val; /* Suppress unused warning */

    serial_puts("[pf_test] FAIL: No #PF generated for stack underflow!\r\n");
    sys_exit(1);
}

/* Main function */
void _start(void) {
    serial_puts("SAIOS Page Fault Test\r\n");
    serial_puts("=====================\r\n\r\n");

    test_null_pointer();
    test_invalid_low_address();
    test_kernel_address();
    test_stack_underflow();

    serial_puts("All tests completed.\r\n");
    sys_exit(0);
}
