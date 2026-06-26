#!/bin/bash
# SAIOS build script - produces dual BIOS + UEFI bootable ISO
#
# Usage:
#   ./build.sh               - build ISO (release mode, BIOS + UEFI)
#   ./build.sh --debug       - build in debug mode
#   ./build.sh --run         - build + launch in QEMU (BIOS mode)
#   ./build.sh --uefi        - build + launch in QEMU (UEFI mode)
#   ./build.sh --sign        - sign bootloader (Secure Boot)

set -e

# Parse arguments
DEBUG=false
RUN_BIOS=false
RUN_UEFI=false
SIGN_BOOT=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --debug)
            DEBUG=true
            shift
            ;;
        --run)
            RUN_BIOS=true
            shift
            ;;
        --uefi)
            RUN_UEFI=true
            shift
            ;;
        --sign)
            SIGN_BOOT=true
            shift
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [--debug] [--run] [--uefi] [--sign]"
            exit 1
            ;;
    esac
done

# Default to release builds for production-ready ISO
if [ "$DEBUG" = true ]; then
    PROFILE_DIR="debug"
    CARGO_ARGS=("build")
else
    PROFILE_DIR="release"
    CARGO_ARGS=("build" "--release")
fi

KERNEL_ARGS=(
    "--target" "x86_64-unknown-none"
    "-Z" "build-std=core,compiler_builtins,alloc"
    "-Z" "build-std-features=compiler-builtins-mem"
)

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DRIVE_LETTER=$(echo "$REPO_DIR" | cut -c1 | tr 'a-zA-Z' 'A-Z' | tr 'A-Z' 'a-z')
REST_PATH="${REPO_DIR:2}"
# Convert Windows path style for WSL if running under WSL
if command -v wslpath &>/dev/null; then
    # We're in WSL, convert to Windows path
    WSL_PATH=$(wslpath -w "$REPO_DIR" 2>/dev/null || echo "$REPO_DIR")
else
    WSL_PATH="$REPO_DIR"
fi

echo ""
echo "[SAIOS] Building kernel (${PROFILE_DIR} mode)..."

# -- 1. Build kernel (Multiboot2 + UEFI compatible) -------------------------
if ! cargo "${CARGO_ARGS[@]}" "${KERNEL_ARGS[@]}" 2>&1 | grep -v "warning:"; then
    echo "[SAIOS] Kernel build failed"
    exit 1
fi

ELF="target/x86_64-unknown-none/${PROFILE_DIR}/saios"
cp "$ELF" "iso/boot/saios.elf"
echo "[SAIOS] Kernel ELF: iso/boot/saios.elf"

# -- 2. Build UEFI stub -----------------------------------------------------
echo ""
echo "[SAIOS] Building UEFI stub..."

UEFI_BUILT=false

if command -v cargo &>/dev/null; then
    # Check if target exists
    if rustup target list --installed | grep -q "x86_64-saios-uefi"; then
        pushd uefi_stub >/dev/null
        export RUST_TARGET_PATH="$(pwd)"
        export RUSTFLAGS="-Zunstable-options"
        UEFI_ARGS=("${CARGO_ARGS[@]}")
        UEFI_ARGS+=("--target" "x86_64-saios-uefi" "-Z" "unstable-options" "-Z" "build-std=core,compiler_builtins")

        if cargo "${UEFI_ARGS[@]}" 2>&1 | grep -v "warning:"; then
            UEFI_EFI="target/x86_64-saios-uefi/${PROFILE_DIR}/saios-uefi.efi"
            if [ -f "$UEFI_EFI" ]; then
                mkdir -p "iso/EFI/BOOT"
                mkdir -p "iso/EFI/SAIOS"
                cp "$UEFI_EFI" "iso/EFI/BOOT/BOOTX64.EFI"
                cp "$ELF" "iso/EFI/SAIOS/saios.elf"
                echo "[SAIOS] UEFI stub: iso/EFI/BOOT/BOOTX64.EFI"
                UEFI_BUILT=true
            fi
        fi
        unset RUSTFLAGS
        unset RUST_TARGET_PATH
        popd >/dev/null
    fi
fi

if [ "$UEFI_BUILT" = false ]; then
    echo "[SAIOS] UEFI stub build skipped (target x86_64-saios-uefi not available)"
fi

# -- 3. Build GRUB EFI (for UEFI fallback) ----------------------------------
echo ""
echo "[SAIOS] Building GRUB EFI binary..."

if command -v grub-mkimage &>/dev/null; then
    grub-mkimage --directory /usr/lib/grub/x86_64-efi \
        --prefix '(hd0,gpt1)/boot/grub' \
        --output /tmp/grubx64.efi \
        --format x86_64-efi \
        fat part_gpt part_msdos normal boot multiboot2 configfile search search_fs_file echo ext2 ls efi_gop gfxterm video_bochs video_cirrus

    mkdir -p "iso/EFI/BOOT"
    cp /tmp/grubx64.efi "iso/EFI/BOOT/grubx64.efi"

    if [ "$UEFI_BUILT" = false ]; then
        cp /tmp/grubx64.efi "iso/EFI/BOOT/BOOTX64.EFI"
    fi

    echo "[SAIOS] GRUB EFI: iso/EFI/BOOT/grubx64.efi"
else
    echo "[SAIOS] GRUB EFI build skipped (install grub-efi-amd64-bin)"
    echo "        Run: sudo apt install grub-pc-bin grub-efi-amd64-bin xorriso mtools"
fi

# -- 4. Secure Boot signing -------------------------------------------------
if [ "$SIGN_BOOT" = true ]; then
    echo ""
    echo "[SAIOS] Signing for Secure Boot..."

    KEYS_DIR="$REPO_DIR/secure_boot/keys"
    ISO_PATH="$REPO_DIR/iso"

    if [ ! -f "$KEYS_DIR/db.key" ]; then
        echo "[SAIOS] Keys not found - run: bash secure_boot/generate_keys.sh"
        exit 1
    fi

    if command -v sbsign &>/dev/null; then
        sbsign --key "$KEYS_DIR/db.key" --cert "$KEYS_DIR/db.crt" \
               --output "$ISO_PATH/EFI/BOOT/BOOTX64.EFI.signed" \
               "$ISO_PATH/EFI/BOOT/BOOTX64.EFI"

        cp "$ISO_PATH/EFI/BOOT/BOOTX64.EFI.signed" "$ISO_PATH/EFI/BOOT/BOOTX64.EFI"
        echo "[SAIOS] Bootloader signed for Secure Boot"
        echo "        Enroll secure_boot/keys/db.auth in UEFI firmware"
    else
        echo "[SAIOS] Signing failed - sbsign not found"
        echo "        Install: sudo apt install sbsigntool"
        exit 1
    fi
fi

# -- 5. Create dual-mode ISO via grub-mkrescue -----------------------------
echo ""
echo "[SAIOS] Building dual BIOS+UEFI ISO..."

if ! grub-mkrescue -o "saios.iso" "iso" 2>&1; then
    echo "[SAIOS] grub-mkrescue failed."
    echo "        Install: sudo apt install grub-pc-bin grub-efi-amd64-bin xorriso mtools"
    exit 1
fi

ISO_PATH="saios.iso"
if [ -f "$ISO_PATH" ]; then
    ISO_SIZE=$(stat -c%s "$ISO_PATH" 2>/dev/null || stat -f%z "$ISO_PATH" 2>/dev/null)
    ISO_SIZE_MB=$(echo "scale=2; $ISO_SIZE / 1048576" | bc)
    echo ""
    echo "+================================================+"
    echo "|  saios.iso built: ${ISO_SIZE_MB} MB                  |"
    echo "|                                                |"
    echo "|  BIOS boot:  select first GRUB menu entry      |"
    echo "|  UEFI boot:  firmware auto-loads BOOTX64.EFI   |"
    if [ "$SIGN_BOOT" = true ]; then
        echo "|  Secure Boot: bootloader signed                |"
    fi
    echo "+================================================+"
else
    echo "[SAIOS] ISO file not found after build"
    exit 1
fi

# -- 6. Optional QEMU launch ------------------------------------------------
if [ "$RUN_BIOS" = true ]; then
    echo ""
    echo "[SAIOS] Launching in QEMU (BIOS mode)..."
    qemu-system-x86_64 \
        -cdrom "$ISO_PATH" -m 512M -serial stdio -no-reboot -boot d \
        -netdev user,id=net0 -device virtio-net-pci,netdev=net0
fi

if [ "$RUN_UEFI" = true ]; then
    echo ""
    echo "[SAIOS] Launching in QEMU (UEFI mode with OVMF)..."

    OVMF_PATH=""
    if [ -f "/usr/share/ovmf/OVMF.fd" ]; then
        OVMF_PATH="/usr/share/ovmf/OVMF.fd"
    elif [ -f "/usr/share/OVMF/OVMF.fd" ]; then
        OVMF_PATH="/usr/share/OVMF/OVMF.fd"
    elif [ -f "/usr/share/ovmf/x86_64/OVMF.fd" ]; then
        OVMF_PATH="/usr/share/ovmf/x86_64/OVMF.fd"
    fi

    if [ -n "$OVMF_PATH" ]; then
        qemu-system-x86_64 \
            -bios "$OVMF_PATH" \
            -cdrom "$ISO_PATH" -m 512M -serial stdio -no-reboot -boot d \
            -netdev user,id=net0 -device virtio-net-pci,netdev=net0
    else
        echo "[SAIOS] OVMF not found. Install: sudo apt install ovmf"
    fi
fi
