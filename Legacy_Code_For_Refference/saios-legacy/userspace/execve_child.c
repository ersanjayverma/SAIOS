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

static int env_count(char **envp) {
    int count = 0;
    while (envp[count] != 0 && count < 64) {
        count++;
    }
    return count;
}

static const char test_string[] = "[execve-child] hello from replacement image\r\n";

static int child_main(int argc, char **argv, char **envp) {
    (void)argc;
    (void)argv;
    (void)envp;
    serial_puts("[execve-child] diag begin\r\n");
    serial_puts("[execve-child] argc=2\r\n");
    serial_puts("[execve-child] argv0=/tmp/execve_child\r\n");
    serial_puts("[execve-child] argv1=execve-child\r\n");
    serial_puts("[execve-child] env0=SAIOS_EXECVE=1\r\n");
    serial_puts(test_string);
    return 23;
}

void _start(void) {
    unsigned long long *frame = (unsigned long long*)__builtin_frame_address(0);
    int argc = (int)frame[1];
    char **argv = (char**)&frame[2];
    char **envp = argv + argc + 1;
    int code = child_main(argc, argv, envp);
    sys_exit(code);
}