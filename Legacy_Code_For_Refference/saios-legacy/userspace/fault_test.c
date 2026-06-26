/*
 * SAIOS Userspace Fault Validation Tests
 *
 * Tests various fault conditions to ensure proper kernel handling:
 * 1. Read-only violation
 * 2. NX violation (instruction fetch from non-executable page)
 * 3. Invalid user pointer
 * 4. Invalid jump target
 *
 * Each test should cause a fault that is properly handled by the kernel,
 * allowing the system to survive and continue operation.
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

static long long sys_write(int fd, const void *buf, unsigned long long len) {
    long long ret;
    __asm__ volatile(
        "syscall"
        : "=a"(ret)
        : "a"((long long)1), "D"((long long)fd), "S"((long long)buf), "d"((long long)len)
        : "rcx", "r11", "memory"
    );
    return ret;
}

/* Test 1: Read-only violation */
void test_readonly_violation(void) {
    serial_puts("[Test 1] Read-only violation...\r\n");

    // Try to write to a read-only string in .rodata section
    const char* s = "This is read-only data";
    volatile char* ptr = (volatile char*)s;

    serial_puts("  Attempting to write to read-only memory...\r\n");

    // This should cause a #PF (page fault) due to write protection
    // The kernel should handle this fault properly and terminate the process
    *ptr = 'X';

    // If we reach here, the fault wasn't handled properly
    serial_puts("  FAIL: No fault generated! Kernel fault handling may be broken.\r\n");
    sys_exit(1);
}

/* Test 2: NX violation (instruction fetch from non-executable page) */
void test_nx_violation(void) {
    serial_puts("[Test 2] NX violation...\r\n");

    // Create a data buffer (should be non-executable)
    unsigned char buf[32];
    for (int i = 0; i < 32; i++) {
        buf[i] = 0x90; // NOP instruction
    }

    serial_puts("  Attempting to execute from non-executable page...\r\n");

    // Try to execute from this buffer (should cause #PF due to NX bit)
    void (*func)(void) = (void(*)(void))buf;
    func();

    // If we reach here, the fault wasn't handled properly
    serial_puts("  FAIL: No NX fault generated! Kernel may not enforce NX protection.\r\n");
    sys_exit(1);
}

/* Test 3: Invalid user pointer */
void test_invalid_pointer(void) {
    serial_puts("[Test 3] Invalid user pointer...\r\n");

    // Try to write to an invalid kernel address
    volatile char* ptr = (volatile char*)0xffffffff80000000ULL; // Kernel space address

    serial_puts("  Attempting to access invalid kernel pointer...\r\n");

    // This should cause a #PF due to non-canonical address or protection
    *ptr = 'A';

    // If we reach here, the fault wasn't handled properly
    serial_puts("  FAIL: No fault generated for invalid pointer! Kernel protection may be broken.\r\n");
    sys_exit(1);
}

/* Test 4: Invalid jump target */
void test_invalid_jump(void) {
    serial_puts("[Test 4] Invalid jump target...\r\n");

    // Try to jump to an unmapped address in user space
    void (*func)(void) = (void(*)(void))0x1000ULL; // Unmapped low address

    serial_puts("  Attempting to jump to unmapped address...\r\n");

    // This should cause a #PF when trying to fetch instructions
    func();

    // If we reach here, the fault wasn't handled properly
    serial_puts("  FAIL: No fault generated for invalid jump! Page fault handling may be broken.\r\n");
    sys_exit(1);
}

/* Test 5: Stack overflow */
void test_stack_overflow(void) {
    serial_puts("[Test 5] Stack overflow...\r\n");

    // Allocate a large array on the stack
    volatile char big_array[0x100000]; // 1MB array

    // Initialize it to force stack growth
    for (int i = 0; i < 0x100000; i++) {
        big_array[i] = (char)(i & 0xFF);
    }

    serial_puts("  Large stack allocation completed.\r\n");

    // If we reach here, stack protection may not be working
    serial_puts("  WARNING: No stack overflow detected!\r\n");
}

/* Main function */
void _start(void) {
    serial_puts("SAIOS Userspace Fault Validation Tests\r\n");
    serial_puts("=====================================\r\n\r\n");

    // Run tests - each should terminate the process with SIGSEGV
    // If any test returns normally, that indicates a fault handling problem

    test_readonly_violation();
    test_nx_violation();
    test_invalid_pointer();
    test_invalid_jump();
    // test_stack_overflow(); // Uncomment to test stack overflow

    serial_puts("All tests completed.\r\n");
    sys_exit(0);
}