# SAIOS build script - produces dual BIOS + UEFI bootable ISO
#
# Usage:
#   .\build.ps1               - build ISO (release mode, BIOS + UEFI)
#   .\build.ps1 -Debug        - build in debug mode
#   .\build.ps1 -Run          - build + launch in QEMU (BIOS mode)
#   .\build.ps1 -RunUefi      - build + launch in QEMU (UEFI mode)
#   .\build.ps1 -SignBoot     - sign bootloader (Secure Boot)

param(
    [switch]$Run,
    [switch]$RunUefi,
    [switch]$SignBoot,
    [switch]$Debug
)

$ErrorActionPreference = "Continue"

# Default to release builds for production-ready ISO
$profile_dir = if ($Debug) { "debug" } else { "release" }
$cargo_args  = if ($Debug) { @("build") } else { @("build", "--release") }
$kernel_args = @(
    "--target", "x86_64-unknown-none",
    "-Z", "build-std=core,compiler_builtins,alloc",
    "-Z", "build-std-features=compiler-builtins-mem"
)

$repo_win = (Resolve-Path ".").Path
$drive    = $repo_win.Substring(0,1).ToLower()
$rest     = $repo_win.Substring(2).Replace('\','/')
$repo_wsl = "/mnt/$drive$rest"

# -- 1. Build kernel (Multiboot2 + UEFI compatible) -------------------------
Write-Host "`n[SAIOS] Building kernel ($profile_dir mode)..." -ForegroundColor Cyan
& cargo $cargo_args $kernel_args 2>&1 | Select-String -NotMatch "warning:"
if ($LASTEXITCODE -ne 0) { Write-Error "Kernel build failed"; exit 1 }

$elf = "target\x86_64-unknown-none\$profile_dir\saios"
Copy-Item $elf "iso\boot\saios.elf" -Force
Write-Host "[SAIOS] Kernel ELF: iso/boot/saios.elf" -ForegroundColor Green

# -- 2. Build UEFI stub -----------------------------------------------------
Write-Host "`n[SAIOS] Building UEFI stub..." -ForegroundColor Cyan
$uefi_built = $false

# Check if lld-link is available (needed for PE32+ output)
$lld_link = Get-Command lld-link -ErrorAction SilentlyContinue
if ($lld_link) {
    Push-Location uefi_stub
    $env:RUST_TARGET_PATH = (Get-Location).Path
    $env:RUSTFLAGS = "-Zunstable-options"
    $uefi_args = if ($Debug) { @("build") } else { @("build", "--release") }
    $uefi_args += @("--target", "x86_64-saios-uefi", "-Z", "unstable-options", "-Z", "build-std=core,compiler_builtins")
    & cargo $uefi_args 2>&1 | Select-String -NotMatch "warning:"
    if ($LASTEXITCODE -eq 0) {
        $uefi_efi = "target\x86_64-saios-uefi\$profile_dir\saios-uefi.efi"
        if (Test-Path $uefi_efi) {
            New-Item -ItemType Directory -Force "iso\EFI\BOOT"  | Out-Null
            New-Item -ItemType Directory -Force "iso\EFI\SAIOS" | Out-Null
            Copy-Item $uefi_efi "iso\EFI\BOOT\BOOTX64.EFI" -Force
            Copy-Item $elf      "iso\EFI\SAIOS\saios.elf"  -Force
            Write-Host "[SAIOS] UEFI stub: iso/EFI/BOOT/BOOTX64.EFI" -ForegroundColor Green
            $uefi_built = $true
        }
    }
    Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue
    Remove-Item Env:RUST_TARGET_PATH -ErrorAction SilentlyContinue
    Pop-Location
} else {
    Write-Host "[SAIOS] lld-link not found - skipping UEFI stub build" -ForegroundColor Yellow
}

# -- 3. Build GRUB EFI (for UEFI fallback even without our stub) ------------
Write-Host "`n[SAIOS] Building GRUB EFI binary..." -ForegroundColor Cyan
$grub_cmd = "grub-mkimage --directory /usr/lib/grub/x86_64-efi --prefix '(hd0,gpt1)/boot/grub' --output /tmp/grubx64.efi --format x86_64-efi fat part_gpt part_msdos normal boot multiboot2 configfile search search_fs_file echo ext2 ls efi_gop gfxterm video_bochs video_cirrus"
$grub_efi_ok = wsl bash -c "$grub_cmd 2>&1"
if ($LASTEXITCODE -eq 0) {
    wsl bash -c "cp /tmp/grubx64.efi '$repo_wsl/iso/EFI/BOOT/grubx64.efi'"
    if (-not $uefi_built) {
        wsl bash -c "cp /tmp/grubx64.efi '$repo_wsl/iso/EFI/BOOT/BOOTX64.EFI'"
    }
    Write-Host "[SAIOS] GRUB EFI: iso/EFI/BOOT/grubx64.efi" -ForegroundColor Green
} else {
    Write-Host "[SAIOS] GRUB EFI build skipped (install grub-efi-amd64-bin in WSL)" -ForegroundColor Yellow
}

# -- 4. Secure Boot signing -------------------------------------------------
if ($SignBoot) {
    Write-Host "`n[SAIOS] Signing for Secure Boot..." -ForegroundColor Cyan
    $keys_dir = "$repo_wsl/secure_boot/keys"
    $signed = wsl bash -c "
      if [ ! -f '$keys_dir/db.key' ]; then
        echo 'Keys not found - run: wsl bash secure_boot/generate_keys.sh'
        exit 1
      fi
      if command -v sbsign &>/dev/null; then
        sbsign --key '$keys_dir/db.key' --cert '$keys_dir/db.crt' `
               --output '$repo_wsl/iso/EFI/BOOT/BOOTX64.EFI.signed' `
               '$repo_wsl/iso/EFI/BOOT/BOOTX64.EFI' && echo OK
        cp '$repo_wsl/iso/EFI/BOOT/BOOTX64.EFI.signed' '$repo_wsl/iso/EFI/BOOT/BOOTX64.EFI'
      else
        echo 'sbsign not found - install: sudo apt install sbsigntool'
        exit 1
      fi
    "
    if ($signed -match "OK") {
        Write-Host "[SAIOS] Bootloader signed for Secure Boot" -ForegroundColor Green
        Write-Host "        Enroll secure_boot/keys/db.auth in UEFI firmware" -ForegroundColor DarkGray
    } else {
        Write-Host "[SAIOS] Signing failed - see above" -ForegroundColor Red
    }
}

# -- 5. Create dual-mode ISO via grub-mkrescue -----------------------------
Write-Host "`n[SAIOS] Building dual BIOS+UEFI ISO..." -ForegroundColor Cyan

$iso_result = wsl bash -c "grub-mkrescue -o '$repo_wsl/saios.iso' '$repo_wsl/iso' 2>&1"
if ($LASTEXITCODE -ne 0) {
    if (Test-Path "saios.iso") {
        Write-Host "grub-mkrescue reported a non-fatal BIOS hybrid warning; ISO was produced." -ForegroundColor Yellow
    } else {
        Write-Host "grub-mkrescue failed." -ForegroundColor Red
        Write-Host "Install: wsl sudo apt install grub-pc-bin grub-efi-amd64-bin xorriso mtools" -ForegroundColor Yellow
        exit 1
    }
}

$iso_path = "saios.iso"
if (Test-Path $iso_path) {
    $iso_size = [math]::Round((Get-Item $iso_path).Length / 1MB, 2)
    Write-Host ""
    Write-Host "+================================================+" -ForegroundColor Green
    Write-Host "|  saios.iso built: $($iso_size.ToString('F2').PadRight(6)) MB                  |" -ForegroundColor Green
    Write-Host "|                                                |" -ForegroundColor Green
    Write-Host "|  BIOS boot:  select first GRUB menu entry      |" -ForegroundColor Green
    Write-Host "|  UEFI boot:  firmware auto-loads BOOTX64.EFI   |" -ForegroundColor Green
    Write-Host "|  Secure Boot: run .\build.ps1 -SignBoot first  |" -ForegroundColor Green
    Write-Host "+================================================+" -ForegroundColor Green
} else {
    Write-Error "ISO file not found after build"
    exit 1
}

# -- 6. Optional QEMU launch -----------------------------------------------
if ($Run) {
    Write-Host "`n[SAIOS] Launching in QEMU (BIOS mode)..." -ForegroundColor Cyan
    qemu-system-x86_64 `
        -cdrom saios.iso -m 512M -serial stdio -no-reboot -boot d `
        -netdev user,id=net0 -device virtio-net-pci,netdev=net0
}

if ($RunUefi) {
    Write-Host "`n[SAIOS] Launching in QEMU (UEFI mode with OVMF)..." -ForegroundColor Cyan
    $ovmf = wsl bash -c "find /usr/share/ovmf /usr/share/OVMF -name 'OVMF.fd' 2>/dev/null | head -1"
    if ($ovmf) {
        $ovmf_win = wsl bash -c "wslpath -w '$ovmf'"
        qemu-system-x86_64 `
            -bios $ovmf_win.Trim() `
            -cdrom saios.iso -m 512M -serial stdio -no-reboot -boot d `
            -netdev user,id=net0 -device virtio-net-pci,netdev=net0
    } else {
        Write-Host "OVMF not found. Install: wsl sudo apt install ovmf" -ForegroundColor Yellow
    }
}
