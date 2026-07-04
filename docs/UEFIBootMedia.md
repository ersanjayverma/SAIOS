# SAIOS UEFI Boot Media (Real Hardware)

This guide focuses on UEFI boot reliability on physical machines (for example, Dell Vostro).

## Why VirtualBox Works But Hardware Fails

Virtual firmware is often more tolerant than laptop firmware. Physical firmware commonly enforces:

- Exact fallback path: `\EFI\BOOT\BOOTX64.EFI`
- Strict FAT32 ESP handling
- Stricter PE/COFF checks (x64 machine type, EFI subsystem, relocations)
- Secure Boot signature enforcement

## What Changed In This Repository

- `scripts/createiso.sh`
  - Adds PE sanity checks before packaging.
  - Keeps fallback files both inside the EFI FAT image and directly under ISO paths.
  - Uses stricter UEFI El Torito settings.
- `scripts/validate-efi.ps1`
  - Validates `efi_main.efi` PE headers:
    - Machine: `0x8664`
    - Subsystem: `EFI_APPLICATION` (`10`)
    - Relocation directory / `.reloc` presence
- `scripts/createuefiusb.ps1`
  - Creates a GPT disk with FAT32 ESP on a USB drive.
  - Copies:
    - `EFI\BOOT\BOOTX64.EFI`
    - `SAIOS\seed.elf`
  - Runs EFI validation before writing media.

## Recommended Flow On Windows

1. Build and create a real UEFI USB (destructive to selected disk):

```powershell
powershell -ExecutionPolicy Bypass -File scripts/createuefiusb.ps1 -DiskNumber <N> -Profile release
```

1. Enter firmware boot menu and choose the **UEFI USB** entry.

1. If boot is blocked before execution:

- Disable Secure Boot temporarily (unsigned image case), or
- Sign the EFI binary and enroll keys

## ISO Path (Mostly For VM / Optical)

```bash
scripts/createiso.sh --rebuild --profile release --out ./saios.iso
```

For physical USB boot, prefer `createuefiusb.ps1` over writing the ISO with generic tools.

## Firmware Checklist (Dell-Friendly)

- UEFI mode enabled
- Legacy/CSM disabled
- Fast Boot disabled (or thorough USB init mode)
- USB boot enabled
- Boot path exists exactly as `EFI/BOOT/BOOTX64.EFI`
- Secure Boot aligned with your signing state
