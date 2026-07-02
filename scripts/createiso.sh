#!/usr/bin/env bash
set -Eeuo pipefail

# Robust UEFI ISO builder for SAIOS.
#
# Produces an ISO with a proper UEFI El Torito entry backed by a FAT EFI image
# that contains:
#   /EFI/BOOT/BOOTX64.EFI   (UEFI application)
#   /SAIOS/seed.elf         (kernel payload)

usage() {
    cat <<'EOF'
Usage: scripts/createiso.sh [options]

Options:
  --profile <release|debug>   Build/profile to package (default: release)
  --out <path>                Output ISO path (default: ./saios.iso)
  --efi-size-mib <N>          EFI FAT image size in MiB (default: 64)
  --rebuild                   Build efi_main + seed/saios before packaging
  --keep-temp                 Keep temporary staging directory
  -h, --help                  Show this help

Requirements:
  xorriso, dd, mkfs.fat, mmd, mcopy, mdir
EOF
}

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE="release"
ISO_OUT="$ROOT/saios.iso"
EFI_SIZE_MIB="64"
REBUILD="0"
KEEP_TEMP="0"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --profile)
            PROFILE="${2:-}"
            shift 2
            ;;
        --out)
            ISO_OUT="${2:-}"
            shift 2
            ;;
        --efi-size-mib)
            EFI_SIZE_MIB="${2:-}"
            shift 2
            ;;
        --rebuild)
            REBUILD="1"
            shift
            ;;
        --keep-temp)
            KEEP_TEMP="1"
            shift
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "Unknown argument: $1" >&2
            usage
            exit 2
            ;;
    esac
done

if [[ "$PROFILE" != "release" && "$PROFILE" != "debug" ]]; then
    echo "Invalid profile: $PROFILE (must be release or debug)" >&2
    exit 2
fi

if ! [[ "$EFI_SIZE_MIB" =~ ^[0-9]+$ ]] || [[ "$EFI_SIZE_MIB" -lt 16 ]]; then
    echo "Invalid --efi-size-mib: $EFI_SIZE_MIB (must be integer >= 16)" >&2
    exit 2
fi

require_cmd() {
    local cmd="$1"
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "Missing required command: $cmd" >&2
        exit 1
    fi
}

for cmd in xorriso dd mkfs.fat mmd mcopy mdir; do
    require_cmd "$cmd"
done

BOOTLOADER="$ROOT/boot/uefi/efi_main/target/x86_64-unknown-uefi/$PROFILE/efi_main.efi"
KERNEL="$ROOT/seed/saios/target/x86_64-unknown-none/$PROFILE/saios"

if [[ "$REBUILD" == "1" ]]; then
    echo "[1/5] Building UEFI bootloader ($PROFILE)..."
    (
        cd "$ROOT/boot/uefi/efi_main"
        cargo build --profile "$PROFILE"
    )

    echo "[2/5] Building kernel payload ($PROFILE)..."
    (
        cd "$ROOT/seed/saios"
        cargo build --target x86_64-unknown-none --profile "$PROFILE"
    )
fi

if [[ ! -f "$BOOTLOADER" ]]; then
    echo "Bootloader not found: $BOOTLOADER" >&2
    echo "Build it first or run with --rebuild" >&2
    exit 1
fi

if [[ ! -f "$KERNEL" ]]; then
    echo "Kernel payload not found: $KERNEL" >&2
    echo "Build it first or run with --rebuild" >&2
    exit 1
fi

STAGE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/saios-iso.XXXXXX")"
trap '[[ "$KEEP_TEMP" == "1" ]] || rm -rf "$STAGE_DIR"' EXIT

ISO_ROOT="$STAGE_DIR/isoroot"
EFI_IMG="$STAGE_DIR/efiboot.img"
mkdir -p "$ISO_ROOT/EFI/BOOT"

echo "[3/5] Creating FAT EFI image (${EFI_SIZE_MIB} MiB)..."
dd if=/dev/zero of="$EFI_IMG" bs=1M count="$EFI_SIZE_MIB" status=none
mkfs.fat -F 32 -n SAIOS_EFI "$EFI_IMG" >/dev/null

echo "[4/5] Populating EFI image..."
mmd -i "$EFI_IMG" ::/EFI
mmd -i "$EFI_IMG" ::/EFI/BOOT
mmd -i "$EFI_IMG" ::/SAIOS
mcopy -i "$EFI_IMG" "$BOOTLOADER" ::/EFI/BOOT/BOOTX64.EFI
mcopy -i "$EFI_IMG" "$KERNEL" ::/SAIOS/seed.elf

echo "[5/5] Building ISO..."
cp "$EFI_IMG" "$ISO_ROOT/EFI/BOOT/efiboot.img"

xorriso \
    -as mkisofs \
    -iso-level 3 \
    -full-iso9660-filenames \
    -volid "SAIOS" \
    -R \
    -J \
    -eltorito-alt-boot \
    -e EFI/BOOT/efiboot.img \
    -no-emul-boot \
    -isohybrid-gpt-basdat \
    -o "$ISO_OUT" \
    "$ISO_ROOT" >/dev/null

echo
echo "ISO created: $ISO_OUT"
echo "Embedded boot files:"
mdir -i "$EFI_IMG" ::/EFI/BOOT
mdir -i "$EFI_IMG" ::/SAIOS

if [[ "$KEEP_TEMP" == "1" ]]; then
    echo "Temporary staging kept at: $STAGE_DIR"
fi