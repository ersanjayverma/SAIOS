#!/bin/bash
# Build an EFI-only SAIOS ISO using grub-mkrescue, but with an isolated grub
# library directory that contains ONLY the x86_64-efi platform.  grub-mkrescue
# then cannot build the i386-pc BIOS El Torito image (the one that triggers the
# "Boot image too small for GRUB2 -> Image write cancelled" MISHAP here), so it
# produces a clean UEFI-only hybrid ISO — the format VBox's firmware boots.
set -e

# Determine the project root: if running under WSL the repo might be on the
# Windows mount, otherwise use the script's own directory.
if [ -n "$1" ]; then
    cd "$1"
elif [ -f "$(dirname "$0")/Cargo.toml" ]; then
    cd "$(dirname "$0")"
elif [ -d "/mnt/c/Users/Sanjar.Verma/Downloads/Personal/saios" ]; then
    cd /mnt/c/Users/Sanjar.Verma/Downloads/Personal/saios
else
    echo "ERROR: cannot locate SAIOS root. Pass it as: $0 /path/to/saios"
    exit 1
fi

echo "SAIOS root: $(pwd)"

# Build the kernel ELF first (release mode)
echo "== cargo build =="
cargo build --release \
    --target x86_64-unknown-none \
    -Z build-std=core,compiler_builtins,alloc \
    -Z build-std-features=compiler-builtins-mem 2>&1 | tail -5

# Copy the built ELF into the install/update media staging tree.
mkdir -p iso/boot/grub
cp target/x86_64-unknown-none/release/saios iso/boot/saios.elf

ISO_GRUB_DIR=/tmp/gisolate
rm -rf "$ISO_GRUB_DIR"
mkdir -p "$ISO_GRUB_DIR"

# Only copy if the platform directory exists (grub-pc-bin may not be installed)
if [ -d /usr/lib/grub/x86_64-efi ]; then
    cp -r /usr/lib/grub/x86_64-efi "$ISO_GRUB_DIR/"
    echo "isolated grub dir: $(ls "$ISO_GRUB_DIR")"
else
    echo "WARNING: /usr/lib/grub/x86_64-efi not found — grub-mkrescue may fail"
fi

rm -f saios.iso

echo "== grub-mkrescue (EFI-only) =="
grub-mkrescue -d "$ISO_GRUB_DIR/x86_64-efi" \
    -o saios.iso iso/ 2>&1 | grep -iE 'mishap|cancel|error|warning|FAILURE' | head || true

if [ -f saios.iso ]; then
    echo "OK: $(stat -c%s saios.iso) bytes"
    if command -v xorriso &>/dev/null; then
        echo "== el torito =="
        xorriso -indev saios.iso -report_el_torito plain 2>&1 | grep -E 'Pltf|img path' | head
    fi
else
    echo "FAILED: no saios.iso produced"
fi
