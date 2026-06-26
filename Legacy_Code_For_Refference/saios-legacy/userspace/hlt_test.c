/*
 * hlt_test.c - Test HLT instruction in user mode
 *
 * Executes HLT instruction which should cause #GP(0).
 * This is actually the "ring3halt" test that validates user mode execution.
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

/* Test: HLT instruction in user mode */
void test_hlt_in_user_mode(void) {
    serial_puts("[hlt_test] Executing HLT in user mode...\r\n");
    serial_puts("  This should cause #GP(0) since HLT is privileged.\r\n");
    serial_puts("  The kernel should handle this and terminate the process.\r\n\r\n");

    /* HLT - halts the CPU (privileged instruction, causes #GP in CPL=3) */
    __asm__ volatile(
        "hlt"
    );

    /* If we reach here, something went wrong */
    serial_puts("\r\n[hlt_test] FAIL: HLT did not cause #GP! Kernel may not handle privileged instructions properly.\r\n");
    sys_exit(1);
}

/* Main function */
void _start(void) {
    serial_puts("SAIOS HLT Instruction Test\r\n");
    serial_puts("==========================\r\n\r\n");

    test_hlt_in_user_mode();

    serial_puts("Test completed.\r\n");
    sys_exit(0);
}
