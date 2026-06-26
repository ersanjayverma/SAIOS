#!/bin/bash
set -e

ROOT=$(pwd)

ISO_DIR="$ROOT/disk"
EFI_IMG="$ROOT/efiboot.img"
ISO_OUT="$ROOT/saios.iso"
BOOTLOADER="$ROOT/boot/uefi/efi_main/target/x86_64-unknown-uefi/release/efi_main.efi"
rm -f "$EFI_IMG"
rm -f "$ISO_OUT"

#
# Create 16MB FAT EFI image
#
dd if=/dev/zero of="$EFI_IMG" bs=1M count=16

mkfs.fat "$EFI_IMG"

#
# Create directories inside FAT image
#
mmd -i "$EFI_IMG" ::/EFI
mmd -i "$EFI_IMG" ::/EFI/BOOT

#
# Copy bootloader
#
mcopy -i "$EFI_IMG" "$BOOTLOADER" ::/EFI/BOOT/BOOTX64.EFI
cp "$EFI_IMG" "$ISO_DIR/efiboot.img"
cp "$ROOT/seed/saios/target/x86_64-unknown-none/release/saios" "$ISO_DIR/seed.elf"
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