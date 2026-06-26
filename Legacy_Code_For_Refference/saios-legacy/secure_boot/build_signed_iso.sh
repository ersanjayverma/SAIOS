#!/usr/bin/env bash
# build_signed_iso.sh — produce a Secure Boot capable SAIOS ISO.
#
# Why this exists: grub-mkrescue embeds its OWN freshly-built (unsigned) GRUB
# core as the UEFI El Torito bootloader, so signing iso/EFI/BOOT/BOOTX64.EFI
# afterwards has no effect — the firmware runs the unsigned core and Secure Boot
# rejects it.  Here we build a self-contained signed GRUB with grub-mkstandalone
# (config embedded, no external prefix needed) and author the ISO with xorriso so
# THAT signed binary is the EFI boot image.  BIOS El Torito is added too when
# grub-pc-bin is available, so the ISO stays dual-boot.
#
# Requires: grub-efi-amd64-bin, grub-pc-bin (optional, for BIOS), sbsigntool,
#           mtools, xorriso, and the db key/cert in secure_boot/keys/.
set -e

REPO="$(cd "$(dirname "$0")/.." && pwd)"
KEYS="$REPO/secure_boot/keys"
cd "$REPO"

[ -f "$KEYS/db.key" ] || { echo "ERROR: $KEYS/db.key missing — generate keys first"; exit 1; }

# -- 1. Embedded GRUB config (locates the kernel on the CD by file search) -----
cat > /tmp/sb-grub.cfg <<'EOF'
search --no-floppy --file --set=root /boot/saios.elf
set prefix=($root)/boot/grub
insmod efi_gop
insmod video_bochs
insmod video_cirrus
insmod gfxterm
insmod chain
# Establish a framebuffer mode and switch GRUB's output to it, so that
# `gfxpayload=keep` has a real mode to hand to the kernel via the Multiboot2
# framebuffer tag.  Without this the kernel gets no framebuffer and the screen
# stays black under EFI (no VGA text fallback).
set gfxmode=1024x768x32,800x600x32,auto
terminal_output gfxterm
set timeout=8
set default=0
menuentry "Boot Installed System" {
    if [ -f (hd1,msdos1)/EFI/BOOT/BOOTX64.EFI ]; then
        chainloader (hd1,msdos1)/EFI/BOOT/BOOTX64.EFI
    elif [ -f (hd1,gpt1)/EFI/BOOT/BOOTX64.EFI ]; then
        chainloader (hd1,gpt1)/EFI/BOOT/BOOTX64.EFI
    elif [ -f (hd0,msdos1)/EFI/BOOT/BOOTX64.EFI ]; then
        chainloader (hd0,msdos1)/EFI/BOOT/BOOTX64.EFI
    elif [ -f (hd0,gpt1)/EFI/BOOT/BOOTX64.EFI ]; then
        chainloader (hd0,gpt1)/EFI/BOOT/BOOTX64.EFI
    elif [ -f (hd2,msdos1)/EFI/BOOT/BOOTX64.EFI ]; then
        chainloader (hd2,msdos1)/EFI/BOOT/BOOTX64.EFI
    elif [ -f (hd2,gpt1)/EFI/BOOT/BOOTX64.EFI ]; then
        chainloader (hd2,gpt1)/EFI/BOOT/BOOTX64.EFI
    else
        echo "No installed BOOTX64.EFI found on hd0/hd1/hd2."
        sleep 3
        return
    fi
    boot
}
menuentry "Live Environment" {
    set gfxpayload=keep
    multiboot2 /boot/saios.elf saios.mode=live
    boot
}
menuentry "Install SAIOS" {
    set gfxpayload=keep
    multiboot2 /boot/saios.elf saios.mode=install
    boot
}
menuentry "Update Existing System" {
    set gfxpayload=keep
    multiboot2 /boot/saios.elf saios.mode=update
    boot
}
menuentry "Recover Existing System" {
    set gfxpayload=keep
    multiboot2 /boot/saios.elf saios.mode=recover
    boot
}
menuentry "Storage Diagnostics" {
    set gfxpayload=keep
    multiboot2 /boot/saios.elf saios.mode=storage-diagnostics
    boot
}
menuentry "Memory Diagnostics" {
    set gfxpayload=keep
    multiboot2 /boot/saios.elf saios.mode=memory-diagnostics
    boot
}
menuentry "Safe Mode" {
    set gfxpayload=keep
    multiboot2 /boot/saios.elf saios.mode=safe
    boot
}
menuentry "Enroll Secure Boot keys (KeyTool)" {
    chainloader /EFI/BOOT/KeyTool.efi
    boot
}
EOF

# -- 2. Build a self-contained EFI image, then sign it -------------------------
grub-mkstandalone -O x86_64-efi -o /tmp/bootx64-unsigned.efi \
    --modules="part_gpt part_msdos fat iso9660 normal multiboot2 search search_fs_file configfile echo ls efi_gop gfxterm video_bochs video_cirrus chain" \
    "boot/grub/grub.cfg=/tmp/sb-grub.cfg"

sbsign --key "$KEYS/db.key" --cert "$KEYS/db.crt" \
       --output /tmp/bootx64.efi /tmp/bootx64-unsigned.efi
echo "--- signature check ---"
sbverify --cert "$KEYS/db.crt" /tmp/bootx64.efi

# -- 3. Build the FAT EFI System Partition image holding the signed bootloader -
rm -f /tmp/efiboot.img
# Size the FAT ESP to comfortably hold the (large) standalone EFI + slack.
EFI_KB=$(( ($(stat -c%s /tmp/bootx64.efi) / 1024) + 2048 ))
dd if=/dev/zero of=/tmp/efiboot.img bs=1024 count=$EFI_KB status=none
mkfs.vfat /tmp/efiboot.img >/dev/null
mmd   -i /tmp/efiboot.img ::/EFI ::/EFI/BOOT
mcopy -i /tmp/efiboot.img /tmp/bootx64.efi ::/EFI/BOOT/BOOTX64.EFI

# Bundle KeyTool + our key .auth files so they can be enrolled from inside the
# guest UEFI (boot in Setup Mode, run KeyTool, enroll db/KEK/PK -> SB enforces
# and our signed GRUB is trusted).  Place them on both the ESP (FAT, KeyTool's
# file browser) and the ISO tree (GRUB chainloads KeyTool from here).
KEYTOOL=/usr/lib/efitools/x86_64-linux-gnu/KeyTool.efi
mcopy -i /tmp/efiboot.img "$KEYTOOL"   ::/EFI/BOOT/KeyTool.efi
for f in PK KEK db; do mcopy -i /tmp/efiboot.img "$KEYS/$f.auth" "::/$f.auth"; done

# Mirror signed binary + KeyTool + auth files into the ISO tree.
mkdir -p iso/EFI/BOOT iso/keys
cp /tmp/bootx64.efi          iso/EFI/BOOT/BOOTX64.EFI
cp "$KEYTOOL"                iso/EFI/BOOT/KeyTool.efi
cp "$KEYS"/PK.auth "$KEYS"/KEK.auth "$KEYS"/db.auth iso/keys/

# -- 4. Optional BIOS El Torito (keeps the ISO dual-boot) ----------------------
BIOS_ARGS=()
if [ -d /usr/lib/grub/i386-pc ]; then
    grub-mkstandalone -O i386-pc -o /tmp/core.img \
        --modules="biosdisk iso9660 part_msdos normal multiboot2 search search_fs_file configfile echo all_video gfxterm" \
        "boot/grub/grub.cfg=/tmp/sb-grub.cfg" 2>/dev/null || true
    if [ -f /usr/lib/grub/i386-pc/cdboot.img ] && [ -f /tmp/core.img ]; then
        cat /usr/lib/grub/i386-pc/cdboot.img /tmp/core.img > /tmp/eltorito.img
        mkdir -p iso/boot/grub/i386-pc
        cp /tmp/eltorito.img iso/boot/grub/i386-pc/eltorito.img
        BIOS_ARGS=(-b boot/grub/i386-pc/eltorito.img -no-emul-boot \
                   -boot-load-size 4 -boot-info-table --grub2-boot-info -eltorito-alt-boot)
    fi
fi

# -- 5. Author the ISO: (optional BIOS) + UEFI via appended EFI partition ------
xorriso -as mkisofs -o saios.iso -R -J -V SAIOS \
    "${BIOS_ARGS[@]}" \
    -append_partition 2 0xef /tmp/efiboot.img \
    -e --interval:appended_partition_2:all:: -no-emul-boot \
    -isohybrid-gpt-basdat \
    iso

echo "--- El Torito report ---"
xorriso -indev saios.iso -report_el_torito plain 2>/dev/null | grep -iE 'boot img|platform|UEFI|BIOS'
echo "SIGNED_ISO_OK"
