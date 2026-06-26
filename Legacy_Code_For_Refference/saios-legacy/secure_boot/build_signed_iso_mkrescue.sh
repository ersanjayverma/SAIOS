#!/usr/bin/env bash
# build_signed_iso_mkrescue.sh — Secure Boot install/update media using grub-mkrescue's OWN core.
#
# Rationale: the self-contained grub-mkstandalone core (~10 MB) appears to
# overlap the kernel's low load addresses and the kernel never starts.
# grub-mkrescue's core is small and boots the kernel fine — so we let
# grub-mkrescue build the ISO, then SIGN its EFI core and splice the signed
# bytes back into the embedded efi.img IN PLACE (same container size, so the
# El Torito LBA pointer stays valid).  We also drop KeyTool + our key .auth
# files into the efi.img ESP so Secure Boot keys can be enrolled from the guest.
set -e

REPO="$(cd "$(dirname "$0")/.." && pwd)"
KEYS="$REPO/secure_boot/keys"
KEYTOOL=/usr/lib/efitools/x86_64-linux-gnu/KeyTool.efi
cd "$REPO"
[ -f "$KEYS/db.key" ] || { echo "ERROR: $KEYS/db.key missing"; exit 1; }

# -- 1. Ensure the ISO grub.cfg has a KeyTool entry (chainloaded from iso9660) -
CFG=iso/boot/grub/grub.cfg
if ! grep -q 'KeyTool.efi' "$CFG"; then
cat >> "$CFG" <<'EOF'

menuentry "Enroll Secure Boot keys (KeyTool)" {
    insmod chain
    chainloader /EFI/BOOT/KeyTool.efi
    boot
}
EOF
fi
mkdir -p iso/EFI/BOOT iso/keys
cp "$KEYTOOL" iso/EFI/BOOT/KeyTool.efi
cp "$KEYS"/PK.auth "$KEYS"/KEK.auth "$KEYS"/db.auth iso/keys/

# -- 2. Let grub-mkrescue build the install/update media -----------------------
grub-mkrescue -o saios.iso iso 2>&1 | tail -1

# -- 3. Locate the embedded UEFI efi.img inside the ISO (El Torito img #2) -----
read EFI_LBA EFI_LDSIZ < <(xorriso -indev saios.iso -report_el_torito plain 2>/dev/null \
    | awk '/El Torito boot img/ && /UEFI/ { print $NF, $(NF-1) }')
[ -n "$EFI_LBA" ] || { echo "ERROR: could not find UEFI El Torito image"; exit 1; }
EFI_BYTES=$(( EFI_LDSIZ * 512 ))       # El Torito load size is in 512-byte units
EFI_SECT=$(( (EFI_BYTES + 2047) / 2048 ))   # round up to whole 2048-byte sectors
EFI_PADDED=$(( EFI_SECT * 2048 ))
echo "efi.img: LBA=$EFI_LBA sectors=$EFI_SECT bytes=$EFI_PADDED"

# -- 4. Extract efi.img, sign its bootloader, add KeyTool + auth (same size) ---
# Block-aligned dd (the El Torito LBA is a 2048-byte sector index) — fast.
dd if=saios.iso of=/tmp/efi.img bs=2048 skip=$EFI_LBA count=$EFI_SECT status=none
mcopy -i /tmp/efi.img ::/EFI/BOOT/BOOTX64.EFI /tmp/grub-core.efi   # mkrescue's core
sbsign --key "$KEYS/db.key" --cert "$KEYS/db.crt" --output /tmp/grub-core-signed.efi /tmp/grub-core.efi
echo "--- signature check ---"; sbverify --cert "$KEYS/db.crt" /tmp/grub-core-signed.efi
mcopy -D o -i /tmp/efi.img /tmp/grub-core-signed.efi ::/EFI/BOOT/BOOTX64.EFI   # overwrite
mcopy -D o -i /tmp/efi.img "$KEYTOOL" ::/EFI/BOOT/KeyTool.efi
for f in PK KEK db; do mcopy -D o -i /tmp/efi.img "$KEYS/$f.auth" "::/$f.auth"; done

# Guard: the container size must not change (so we can write it back in place).
NEW=$(stat -c%s /tmp/efi.img)
[ "$NEW" -eq "$EFI_PADDED" ] || { echo "ERROR: efi.img size changed ($NEW != $EFI_PADDED)"; exit 1; }

# -- 5. Splice the patched efi.img back into the ISO at the same offset --------
dd if=/tmp/efi.img of=saios.iso bs=2048 seek=$EFI_LBA count=$EFI_SECT conv=notrunc status=none

# -- 6. Verify the bootloader the firmware will actually run is signed ---------
rm -rf /tmp/bi; xorriso -osirrox on -indev saios.iso -extract_boot_images /tmp/bi 2>/dev/null
echo "--- bootloader inside final ISO ---"
mcopy -i /tmp/bi/eltorito_img2_uefi.img ::/EFI/BOOT/BOOTX64.EFI /tmp/final.efi 2>/dev/null
sbverify --cert "$KEYS/db.crt" /tmp/final.efi && echo SIGNED_MKRESCUE_ISO_OK
