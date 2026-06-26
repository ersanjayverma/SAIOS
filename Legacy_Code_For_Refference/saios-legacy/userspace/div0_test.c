/*
 * div0_test.c - Test Divide Error (#DE) handling
 *
 * Divides by zero which should cause #DE.
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

/* Test: Integer divide by zero */
void test_divide_by_zero(void) {
    int numerator = 1;
    int denominator = 0;
    int result;

    serial_puts("[div0_test] Testing integer divide by zero...\r\n");

    /* This should cause #DE (Divide Error) */
    result = numerator / denominator;

    serial_puts("[div0_test] FAIL: No #DE generated! Kernel may not handle divide by zero.\r\n");
    sys_exit(1);
}

/* Test: IDIV by zero */
void test_idiv_by_zero(void) {
    long long numerator = 100;
    long long denominator = 0;
    long long result;

    serial_puts("[div0_test] Testing IDIV by zero...\r\n");

    /* This should cause #DE */
    __asm__ volatile(
        "idiv %3"
        : "=a"(result)
        : "a"(numerator), "d"(0), "r"(denominator)
    );

    serial_puts("[div0_test] FAIL: No #DE generated!\r\n");
    sys_exit(1);
}

/* Test: AID by zero */
void test_aid_by_zero(void) {
    int numerator = 1;
    int denominator = 0;
    int result;

    serial_puts("[div0_test] Testing AID by zero...\r\n");

    /* AID = Absolute Integer Division */
    /* This should cause #DE */
    __asm__ volatile(
        "idiv %2"
        : "=a"(result)
        : "a"(numerator), "r"(denominator)
    );

    serial_puts("[div0_test] FAIL: No #DE generated!\r\n");
    sys_exit(1);
}

/* Main function */
void _start(void) {
    serial_puts("SAIOS Divide by Zero Test\r\n");
    serial_puts("=========================\r\n\r\n");

    test_divide_by_zero();
    test_idiv_by_zero();
    test_aid_by_zero();

    serial_puts("All tests completed.\r\n");
    sys_exit(0);
}
