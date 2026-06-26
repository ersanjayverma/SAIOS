#!/bin/bash
# Boot SAIOS install/update media (clears NVRAM so EFI picks up DVD before disk).
# This is not a live-boot path. Runtime operation must boot from HDD.

set -e

# Configuration
VBOX_MANAGE="VBoxManage"
NVRAM_DIR="${HOME}/VirtualBox VMs/SAIOS"
NVRAM_FILE="${NVRAM_DIR}/SAIOS.nvram"
ISO_FILE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/saios.iso"
SERIAL_LOG="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/seriallog.txt"
VM_NAME="SAIOS"

# Check if VBoxManage is available
if ! command -v "$VBOX_MANAGE" &>/dev/null; then
    echo "Error: VBoxManage not found. Install VirtualBox."
    exit 1
fi

# Check if VM exists
if ! $VBOX_MANAGE list vms | grep -q "\"$VM_NAME\""; then
    echo "Error: VM '$VM_NAME' not found. Create it first with VirtualBox GUI or:"
    echo "  VBoxManage createvm --name SAIOS --ostype Linux_64 --register"
    exit 1
fi

# Check if ISO exists
if [ ! -f "$ISO_FILE" ]; then
    echo "Error: ISO file not found: $ISO_FILE"
    echo "Run ./build.sh first to build install/update media."
    exit 1
fi

# Power off VM if running
echo "Stopping VM..."
"$VBOX_MANAGE" controlvm "$VM_NAME" poweroff 2>/dev/null || true
sleep 3

# Clear NVRAM
echo "Clearing NVRAM..."
if [ -d "$NVRAM_DIR" ] && [ -f "$NVRAM_FILE" ]; then
    rm -f "$NVRAM_FILE"
    echo "NVRAM cleared"
else
    echo "NVRAM directory not found or already cleared"
fi

# Attach ISO (try AHCI first, fall back to IDE)
echo "Attaching install/update media: $ISO_FILE"
if ! "$VBOX_MANAGE" storageattach "$VM_NAME" \
    --storagectl "AHCI" \
    --port 1 \
    --device 0 \
    --type dvddrive \
    --medium "$ISO_FILE" 2>/dev/null; then
    echo "AHCI controller not found, trying IDE..."
    "$VBOX_MANAGE" storageattach "$VM_NAME" \
        --storagectl "IDE" \
        --port 1 \
        --device 0 \
        --type dvddrive \
        --medium "$ISO_FILE"
fi

# Configure serial COM1 to log to file
# SAIOS initializes a 16550 UART at COM1 (I/O port 0x3F8, IRQ4) and mirrors
# kernel diagnostics there through serial_print!/serial_println!. Routing the
# VM UART to a host file gives a persistent trace for early ring-3 failures,
# including crashes before the framebuffer console can be trusted.
echo "Serial COM1 log: $SERIAL_LOG"
rm -f "$SERIAL_LOG"
"$VBOX_MANAGE" modifyvm "$VM_NAME" \
    --uart1 0x3F8 4 \
    --uartmode1 file "$SERIAL_LOG"

# Start VM
echo "Starting VM..."
"$VBOX_MANAGE" startvm "$VM_NAME"
