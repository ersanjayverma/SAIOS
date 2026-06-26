#!/bin/bash
# generate_keys.sh — Generate SAIOS Secure Boot keys
#
# Creates a complete Secure Boot PKI:
#   PK  (Platform Key)       — root of trust, signs KEK
#   KEK (Key Exchange Key)   — intermediate CA, signs db
#   db  (Signature Database) — signs bootloaders (BOOTX64.EFI, grubx64.efi)
#
# Usage:
#   bash secure_boot/generate_keys.sh
#   # Keys are written to secure_boot/keys/
#
# After generating keys, enroll them in firmware:
#   VirtualBox: enable Secure Boot in VM settings, then use EFI Shell to enroll
#   Real HW:    boot into UEFI setup → Secure Boot → Key Management → Enroll PK/KEK/db
#
# Requirements (WSL):
#   sudo apt install openssl sbsigntool efitools

set -e
KEYS_DIR="$(dirname "$0")/keys"
mkdir -p "$KEYS_DIR"
cd "$KEYS_DIR"

SAIOS_GUID="d4c93c5f-5937-4a3d-8a1c-3a5f8b2e1d6c"

echo "=== SAIOS Secure Boot Key Generation ==="
echo "Output directory: $KEYS_DIR"
echo

# -- 1. Platform Key (PK) --------------------------------------------------
echo "[1/6] Generating Platform Key (PK)..."
openssl req -newkey rsa:4096 -nodes -keyout PK.key -new \
    -x509 -sha256 -days 3650 \
    -subj "/CN=SAIOS Platform Key/" \
    -out PK.crt
openssl x509 -in PK.crt -out PK.cer -outform DER
cert-to-efi-sig-list -g "$SAIOS_GUID" PK.crt PK.esl
sign-efi-sig-list -g "$SAIOS_GUID" -k PK.key -c PK.crt PK PK.esl PK.auth
echo "    PK.key, PK.crt, PK.auth created."

# -- 2. Key Exchange Key (KEK) ---------------------------------------------
echo "[2/6] Generating Key Exchange Key (KEK)..."
openssl req -newkey rsa:4096 -nodes -keyout KEK.key -new \
    -x509 -sha256 -days 3650 \
    -subj "/CN=SAIOS Key Exchange Key/" \
    -out KEK.crt
openssl x509 -in KEK.crt -out KEK.cer -outform DER
cert-to-efi-sig-list -g "$SAIOS_GUID" KEK.crt KEK.esl
sign-efi-sig-list -g "$SAIOS_GUID" -k PK.key -c PK.crt KEK KEK.esl KEK.auth
echo "    KEK.key, KEK.crt, KEK.auth created."

# -- 3. Signature Database key (db) ----------------------------------------
echo "[3/6] Generating Signature Database key (db)..."
openssl req -newkey rsa:4096 -nodes -keyout db.key -new \
    -x509 -sha256 -days 3650 \
    -subj "/CN=SAIOS Bootloader Signing Key/" \
    -out db.crt
openssl x509 -in db.crt -out db.cer -outform DER
cert-to-efi-sig-list -g "$SAIOS_GUID" db.crt db.esl
sign-efi-sig-list -g "$SAIOS_GUID" -k KEK.key -c KEK.crt db db.esl db.auth
echo "    db.key, db.crt, db.auth created."

# -- 4. Sign the UEFI stub -------------------------------------------------
echo "[4/6] Looking for UEFI stub to sign..."
STUB="../saios-uefi.efi"
if [ -f "$STUB" ]; then
    sbsign --key db.key --cert db.crt \
           --output saios-uefi-signed.efi "$STUB"
    echo "    saios-uefi-signed.efi created."
else
    echo "    WARNING: $STUB not found — build the UEFI stub first."
    echo "    Run: cd uefi_stub && RUST_TARGET_PATH=$PWD cargo build --release --target x86_64-saios-uefi -Z unstable-options -Z build-std=core,compiler_builtins"
fi

# -- 5. Sign GRUB EFI ------------------------------------------------------
echo "[5/6] Looking for GRUB EFI binary to sign..."
GRUB_EFI=$(find /usr/lib/grub/x86_64-efi* -name "*.efi" 2>/dev/null | head -1)
if command -v grub-mkimage &>/dev/null; then
    echo "    Building GRUB EFI binary..."
    grub-mkimage \
        --directory /usr/lib/grub/x86_64-efi \
        --prefix    /boot/grub \
        --output    grubx64-unsigned.efi \
        --format    x86_64-efi \
        fat part_gpt part_msdos normal boot multiboot2 \
        configfile search search_fs_file echo ext2 ls linuxefi efi_gop gfxterm video_bochs video_cirrus
    sbsign --key db.key --cert db.crt \
           --output grubx64.efi grubx64-unsigned.efi
    echo "    grubx64.efi (signed) created."
else
    echo "    grub-mkimage not found — install grub-efi-amd64-bin"
fi

# -- 6. Summary ------------------------------------------------------------
echo
echo "[6/6] Summary of generated files:"
echo "  PK.auth   — enroll as Platform Key in UEFI firmware"
echo "  KEK.auth  — enroll as Key Exchange Key"
echo "  db.auth   — enroll in Signature Database"
echo "  grubx64.efi          — signed GRUB (use as BOOTX64.EFI)"
echo "  saios-uefi-signed.efi — signed SAIOS UEFI stub"
echo
echo "=== Enrollment instructions ==="
echo
echo "VirtualBox:"
echo "  1. Settings → System → Motherboard → Enable EFI"
echo "  2. Settings → System → Secure Boot → Enable Secure Boot"
echo "  3. Boot SAIOS ISO → at UEFI firmware shell run:"
echo "     FS0:\\EFI\\BOOT\\enroll_keys.efi"
echo "  4. Or use: Setup Mode → Enroll from file"
echo
echo "Physical hardware (AMI/Phoenix UEFI):"
echo "  UEFI Setup → Boot → Secure Boot → Key Management"
echo "  → Enroll All Factory Keys → No"
echo "  → PK  → 'Enroll New Key' → select PK.auth"
echo "  → KEK → 'Enroll New Key' → select KEK.auth"
echo "  → db  → 'Enroll New Key' → select db.auth"
echo "  → Save and reboot"
