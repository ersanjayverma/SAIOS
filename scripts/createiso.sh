#!/bin/bash
set -e

ROOT=$(pwd)

ISO_DIR="$ROOT/disk"
EFI_IMG="$ROOT/efiboot.img"
ISO_OUT="$ROOT/saios.iso"
BOOTLOADER="$ROOT/boot/uefi/efi_main/target/x86_64-unknown-uefi/release/efi_main.efi"
rm -f "$EFI_IMG"
rm -f "$ISO_OUT"
rm -f "$ISO_DIR/efiboot.img"
#rm -f "/mnt/c/Users/Black/VirtualBox VMs/SAIOS/SAIOS.nvram"
#
# Create 16MB FAT EFI image
#
dd if=/dev/zero of="$EFI_IMG" bs=1M count=64

mkfs.fat "$EFI_IMG"

#
# Create directories inside FAT image
#
mmd -i "$EFI_IMG" ::/EFI
mmd -i "$EFI_IMG" ::/EFI/BOOT
mmd -i "$EFI_IMG" ::/SAIOS
#
# Copy bootloader
#
mcopy -i "$EFI_IMG" "$BOOTLOADER" ::/EFI/BOOT/BOOTX64.EFI
mcopy -i "$EFI_IMG" "$ROOT/seed/saios/target/x86_64-unknown-none/release/saios" ::/SAIOS/seed.elf
cp "$EFI_IMG" "$ISO_DIR/efiboot.img"
mdir -i "$EFI_IMG" ::/SAIOS
xorriso \
    -as mkisofs \
    -iso-level 3 \
    -R \
    -J \
    --efi-boot efiboot.img \
    -no-emul-boot \
    -o "$ISO_OUT" \
    "$ISO_DIR"

echo
echo "Created:"
echo "$ISO_OUT"