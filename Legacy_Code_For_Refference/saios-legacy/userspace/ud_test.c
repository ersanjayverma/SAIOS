/*
 * ud_test.c - Test Invalid Opcode (UD2) handling
 *
 * Executes an invalid opcode which should cause #UD.
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

/* Test: UD2 invalid opcode */
void test_ud2_opcode(void) {
    serial_puts("[ud_test] Executing UD2 invalid opcode...\r\n");

    /* UD2 is the official "undefined opcode" instruction */
    __asm__ volatile(
        "ud2"
    );

    serial_puts("[ud_test] FAIL: No #UD generated! Kernel may not handle invalid opcodes.\r\n");
    sys_exit(1);
}

/* Test: Random invalid opcode bytes */
void test_random_invalid_opcode(void) {
    serial_puts("[ud_test] Executing random invalid opcode bytes...\r\n");

    /* Execute random bytes that aren't valid instructions */
    __asm__ volatile(
        ".byte 0xFF, 0xFF, 0xFF, 0xFF"
    );

    serial_puts("[ud_test] FAIL: No #UD generated!\r\n");
    sys_exit(1);
}

/* Main function */
void _start(void) {
    serial_puts("SAIOS Invalid Opcode Test\r\n");
    serial_puts("=========================\r\n\r\n");

    test_ud2_opcode();
    test_random_invalid_opcode();

    serial_puts("All tests completed.\r\n");
    sys_exit(0);
}
