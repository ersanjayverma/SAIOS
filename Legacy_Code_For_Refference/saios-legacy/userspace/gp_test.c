/*
 * gp_test.c - Test General Protection Fault (CPL=3) handling
 *
 * Executes a privileged instruction in user mode which should cause #GP(0).
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

/* Test: IN instruction (privilege level 3 should #GP) */
void test_in_instruction(void) {
    serial_puts("[gp_test] Testing IN instruction from user mode...\r\n");

    /* IN AL, DX - reads from I/O port */
    /* This should cause #GP(0) because we're in CPL=3 */
    __asm__ volatile(
        "inb %%dx, %%al"
        :
        : "d"(0x64)  /* PS/2 controller status port */
        : "al"
    );

    serial_puts("[gp_test] FAIL: No #GP generated! Kernel may not handle user #GP.\r\n");
    sys_exit(1);
}

/* Test: OUT instruction (privilege level 3 should #GP) */
void test_out_instruction(void) {
    serial_puts("[gp_test] Testing OUT instruction from user mode...\r\n");

    /* OUT DX, AL - writes to I/O port */
    /* This should cause #GP(0) because we're in CPL=3 */
    __asm__ volatile(
        "outb %%al, %%dx"
        :
        : "d"(0x64), "a"(0xFF)
    );

    serial_puts("[gp_test] FAIL: No #GP generated! Kernel may not handle user #GP.\r\n");
    sys_exit(1);
}

/* Test: HLT instruction (privilege level 3 should #GP) */
void test_hlt_instruction(void) {
    serial_puts("[gp_test] Testing HLT instruction from user mode...\r\n");

    /* HLT - halts the CPU (privileged instruction) */
    /* This should cause #GP(0) because we're in CPL=3 */
    __asm__ volatile(
        "hlt"
    );

    serial_puts("[gp_test] FAIL: No #GP generated! Kernel may not handle user #GP.\r\n");
    sys_exit(1);
}

/* Test: CLI instruction (privilege level 3 should #GP) */
void test_cli_instruction(void) {
    serial_puts("[gp_test] Testing CLI instruction from user mode...\r\n");

    /* CLI - clears interrupt flag (privileged) */
    /* This should cause #GP(0) because we're in CPL=3 */
    __asm__ volatile(
        "cli"
    );

    serial_puts("[gp_test] FAIL: No #GP generated! Kernel may not handle user #GP.\r\n");
    sys_exit(1);
}

/* Test: STI instruction (privilege level 3 should #GP) */
void test_sti_instruction(void) {
    serial_puts("[gp_test] Testing STI instruction from user mode...\r\n");

    /* STI - sets interrupt flag (privileged) */
    /* This should cause #GP(0) because we're in CPL=3 */
    __asm__ volatile(
        "sti"
    );

    serial_puts("[gp_test] FAIL: No #GP generated! Kernel may not handle user #GP.\r\n");
    sys_exit(1);
}

/* Main function */
void _start(void) {
    serial_puts("SAIOS General Protection Fault Test\r\n");
    serial_puts("====================================\r\n\r\n");

    /* Test each privileged instruction */
    test_in_instruction();
    test_out_instruction();
    test_hlt_instruction();
    test_cli_instruction();
    test_sti_instruction();

    serial_puts("All tests completed.\r\n");
    sys_exit(0);
}
