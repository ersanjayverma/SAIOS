// Test TTY/VFS integration - user process accessing /dev/console
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <fcntl.h>
#include <string.h>

int main() {
    int fd;
    char buf[256];
    
    // Test 1: Open console
    printf("Opening /dev/console...\n");
    fd = open("/dev/console", O_RDWR);
    if (fd < 0) {
        printf("FAIL: Could not open /dev/console: %d\n", errno);
        return 1;
    }
    printf("PASS: Opened /dev/console, fd=%d\n", fd);
    
    // Test 2: Write to console
    printf("Writing to console...\n");
    const char *msg = "Hello from user-space console test!\n";
    ssize_t n = write(fd, msg, strlen(msg));
    if (n < 0) {
        printf("FAIL: Write failed: %d\n", errno);
        close(fd);
        return 1;
    }
    printf("PASS: Wrote %zd bytes to console\n", n);
    
    // Test 3: Read from console (should return immediately if no input)
    printf("Reading from console (with timeout)...\n");
    fd_set rfds;
    FD_ZERO(&rfds);
    FD_SET(fd, &rfds);
    
    struct timeval tv;
    tv.tv_sec = 2;
    tv.tv_usec = 0;
    
    int sel = select(fd + 1, &rfds, NULL, NULL, &tv);
    if (sel < 0) {
        printf("FAIL: select failed: %d\n", errno);
        close(fd);
        return 1;
    } else if (sel == 0) {
        printf("PASS: No input available (expected - no keyboard input in test)\n");
    } else {
        n = read(fd, buf, sizeof(buf) - 1);
        if (n < 0) {
            printf("FAIL: Read failed: %d\n", errno);
            close(fd);
            return 1;
        }
        buf[n] = '\0';
        printf("PASS: Read %zd bytes: %s\n", n, buf);
    }
    
    // Test 4: Close console
    printf("Closing /dev/console...\n");
    close(fd);
    printf("PASS: Closed /dev/console\n");
    
    printf("\n=== TTY VFS Test Complete ===\n");
    return 0;
}
