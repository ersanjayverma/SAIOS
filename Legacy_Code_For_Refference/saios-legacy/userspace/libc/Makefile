CC ?= gcc
AR ?= ar
BUILD_DIR := build
INCLUDE_DIR := include

CFLAGS ?= -ffreestanding -fno-stack-protector -fno-builtin -fno-pic -mcmodel=large -mno-red-zone -Wall -Wextra -I$(INCLUDE_DIR)
ARFLAGS ?= rcs

CRT := $(BUILD_DIR)/crt0.o
OBJS := \
	$(BUILD_DIR)/syscall.o \
	$(BUILD_DIR)/unistd.o \
	$(BUILD_DIR)/string.o \
	$(BUILD_DIR)/stdio.o \
	$(BUILD_DIR)/stdlib.o

LIBC := $(BUILD_DIR)/libc.a

.PHONY: all clean

all: $(CRT) $(LIBC)

$(BUILD_DIR):
	mkdir -p $(BUILD_DIR)

$(BUILD_DIR)/crt0.o: crt/crt0.S | $(BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(BUILD_DIR)/%.o: src/%.c | $(BUILD_DIR)
	$(CC) $(CFLAGS) -c $< -o $@

$(LIBC): $(OBJS)
	$(AR) $(ARFLAGS) $@ $(OBJS)

clean:
	rm -rf $(BUILD_DIR)
