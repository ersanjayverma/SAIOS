def arr(p):
    with open(p, 'rb') as f:
        return ','.join(str(b) for b in f.read())

boot = arr('/usr/lib/grub/i386-pc/boot.img')
core = arr('/tmp/c.img')
efi  = arr('/tmp/BOOTX64.EFI')
with open('src/install/grub_embed.rs', 'w') as f:
    f.write("// Auto-generated GRUB images: BIOS boot.img + core.img, and UEFI BOOTX64.EFI.\n")
    f.write("pub static GRUB_BOOT_IMG: &[u8] = &[%s];\n" % boot)
    f.write("pub static GRUB_CORE_IMG: &[u8] = &[%s];\n" % core)
    f.write("pub static GRUB_EFI_IMG: &[u8] = &[%s];\n" % efi)
print("boot", boot.count(',') + 1, "core", core.count(',') + 1, "efi", efi.count(',') + 1)
