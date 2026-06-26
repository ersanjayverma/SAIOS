# setup_vbox.ps1 — One-shot Secure Boot setup for SAIOS in VirtualBox
#
# Usage:
#   .\secure_boot\setup_vbox.ps1 -VmName "SAIOS"
#
# What this script does:
#   1. Generates Secure Boot PKI (PK, KEK, db) in WSL
#   2. Signs BOOTX64.EFI with the db key
#   3. Rebuilds saios.iso with the signed bootloader
#   4. Enrolls our keys into the VirtualBox VM's NVRAM
#      (replaces Microsoft certs — SAIOS-only Secure Boot)
#
# After this, booting the ISO in VirtualBox with Secure Boot ON will work.

param(
    [Parameter(Mandatory=$true)]
    [string]$VmName,
    [switch]$KeepMsCerts  # also keep Microsoft certs so Windows VMs still work
)

$ErrorActionPreference = "Continue"
$repo = (Resolve-Path ".").Path
$d    = $repo.Substring(0,1).ToLower()
$r    = $repo.Substring(2).Replace('\','/')
$wsl  = "/mnt/$d$r"

Write-Host "=== SAIOS Secure Boot Setup for VirtualBox ===" -ForegroundColor Cyan
Write-Host "VM: $VmName"
Write-Host ""

# ── Step 1: Install required WSL packages ─────────────────────────────────
Write-Host "[1/6] Checking WSL dependencies..." -ForegroundColor Yellow
wsl bash -c "command -v openssl sbsign cert-to-efi-sig-list sign-efi-sig-list &>/dev/null || sudo apt-get install -y openssl sbsigntool efitools 2>&1 | tail -3"

# ── Step 2: Generate Secure Boot keys ─────────────────────────────────────
Write-Host "[2/6] Generating Secure Boot keys..." -ForegroundColor Yellow
$keys_wsl = "$wsl/secure_boot/keys"
wsl bash -c "mkdir -p '$keys_wsl'"

$key_script = @'
set -e
GUID="d4c93c5f-5937-4a3d-8a1c-3a5f8b2e1d6c"
cd "$1"

# Only regenerate if keys don't exist
if [ -f db.key ] && [ -f db.crt ]; then
  echo "Keys already exist, skipping generation."
  exit 0
fi

echo "Generating PK..."
openssl req -newkey rsa:2048 -nodes -keyout PK.key -new \
    -x509 -sha256 -days 3650 -subj "/CN=SAIOS Platform Key/" -out PK.crt 2>/dev/null
openssl x509 -in PK.crt -out PK.cer -outform DER
cert-to-efi-sig-list -g "$GUID" PK.crt PK.esl
sign-efi-sig-list -g "$GUID" -k PK.key -c PK.crt PK PK.esl PK.auth

echo "Generating KEK..."
openssl req -newkey rsa:2048 -nodes -keyout KEK.key -new \
    -x509 -sha256 -days 3650 -subj "/CN=SAIOS KEK/" -out KEK.crt 2>/dev/null
openssl x509 -in KEK.crt -out KEK.cer -outform DER
cert-to-efi-sig-list -g "$GUID" KEK.crt KEK.esl
sign-efi-sig-list -g "$GUID" -k PK.key -c PK.crt KEK KEK.esl KEK.auth

echo "Generating db..."
openssl req -newkey rsa:2048 -nodes -keyout db.key -new \
    -x509 -sha256 -days 3650 -subj "/CN=SAIOS Bootloader Key/" -out db.crt 2>/dev/null
openssl x509 -in db.crt -out db.cer -outform DER
cert-to-efi-sig-list -g "$GUID" db.crt db.esl
sign-efi-sig-list -g "$GUID" -k KEK.key -c KEK.crt db db.esl db.auth

echo "Keys generated successfully."
'@
$key_script | wsl bash -c "bash -s '$keys_wsl'"

# ── Step 3: Build GRUB EFI ─────────────────────────────────────────────────
Write-Host "[3/6] Building GRUB EFI binary..." -ForegroundColor Yellow
wsl bash -c "
  sudo apt-get install -y grub-efi-amd64-bin 2>&1 | grep -v '^Get\|^Fetch\|^Unpack' | tail -3
  grub-mkimage \
    --directory /usr/lib/grub/x86_64-efi \
    --prefix '(hd0)/boot/grub' \
    --output /tmp/grubx64-unsigned.efi \
    --format x86_64-efi \
    fat part_gpt part_msdos normal boot multiboot2 \
        configfile search echo ext2 ls efi_gop gfxterm video_bochs video_cirrus 2>&1 && echo GRUB_OK
"

# ── Step 4: Sign BOOTX64.EFI ──────────────────────────────────────────────
Write-Host "[4/6] Signing bootloader..." -ForegroundColor Yellow
New-Item -ItemType Directory -Force "iso\EFI\BOOT" | Out-Null
wsl bash -c "
  sbsign --key '$keys_wsl/db.key' --cert '$keys_wsl/db.crt' \
         --output '$wsl/iso/EFI/BOOT/BOOTX64.EFI' \
         /tmp/grubx64-unsigned.efi && echo SIGNED_OK
"

# ── Step 5: Rebuild ISO ────────────────────────────────────────────────────
Write-Host "[5/6] Rebuilding ISO with signed bootloader..." -ForegroundColor Yellow
wsl bash -c "grub-mkrescue -o '$wsl/saios.iso' '$wsl/iso' 2>&1 | tail -2"
$iso_size = [math]::Round((Get-Item "saios.iso").Length / 1MB, 2)
Write-Host "  saios.iso: $iso_size MB" -ForegroundColor Green

# ── Step 6: Enroll keys in VirtualBox VM NVRAM ────────────────────────────
Write-Host "[6/6] Enrolling keys in VirtualBox VM '$VmName'..." -ForegroundColor Yellow

# Check VBoxManage is available
if (-not (Get-Command VBoxManage -ErrorAction SilentlyContinue)) {
    $vbox_paths = @(
        "C:\Program Files\Oracle\VirtualBox\VBoxManage.exe",
        "C:\Program Files (x86)\Oracle\VirtualBox\VBoxManage.exe"
    )
    $vbox = $vbox_paths | Where-Object { Test-Path $_ } | Select-Object -First 1
    if (-not $vbox) {
        Write-Host "VBoxManage not found. Enroll keys manually:" -ForegroundColor Red
        print_manual_instructions
        exit 1
    }
    Set-Alias VBoxManage $vbox
}

# Convert WSL key paths to Windows paths
$pk_cer  = "$repo\secure_boot\keys\PK.cer"
$kek_cer = "$repo\secure_boot\keys\KEK.cer"
$db_cer  = "$repo\secure_boot\keys\db.cer"

# Copy .cer files from WSL if they don't exist on Windows
if (-not (Test-Path $pk_cer)) {
    wsl bash -c "cp '$keys_wsl/PK.cer'  '$wsl/secure_boot/keys/PK.cer'"
    wsl bash -c "cp '$keys_wsl/KEK.cer' '$wsl/secure_boot/keys/KEK.cer'"
    wsl bash -c "cp '$keys_wsl/db.cer'  '$wsl/secure_boot/keys/db.cer'"
}

# Stop VM if running
$vm_state = VBoxManage showvminfo $VmName --machinereadable 2>$null | Select-String "VMState="
if ($vm_state -match "running") {
    Write-Host "  Stopping VM..."
    VBoxManage controlvm $VmName poweroff
    Start-Sleep -Seconds 2
}

# Reset NVRAM to Setup Mode (clears all existing keys including Microsoft's)
Write-Host "  Resetting UEFI NVRAM to Setup Mode..."
VBoxManage modifynvram $VmName inituefivarstore

if ($KeepMsCerts) {
    Write-Host "  Enrolling Microsoft signatures (Windows compatibility)..."
    VBoxManage modifynvram $VmName enrollmssignatures 2>$null
}

# Enroll our keys.
#
# NOTE (VBox 7.x): `modifynvram` only supports enrolling a custom Platform Key
# (enrollpk) and additional trusted signing certs as Machine Owner Keys
# (enrollmok).  There is NO custom-KEK or custom-db enrollment subcommand.
# The MOK list is consulted when verifying the bootloader, so we enroll the db
# cert (which signs BOOTX64.EFI) as a MOK — that is what makes our bootloader
# trusted under Secure Boot.  KEK.cer is unused for VBox enrollment.
$owner_guid = "d4c93c5f-5937-4a3d-8a1c-3a5f8b2e1d6c"
Write-Host "  Enrolling SAIOS Platform Key (PK)..."
VBoxManage modifynvram $VmName enrollpk `
    --platform-key=$pk_cer `
    --owner-uuid=$owner_guid

Write-Host "  Enrolling SAIOS bootloader cert as MOK (db -> trusted signer)..."
VBoxManage modifynvram $VmName enrollmok `
    --mok=$db_cer `
    --owner-uuid=$owner_guid

Write-Host "  Enabling Secure Boot on the VM..."
VBoxManage modifynvram $VmName secureboot --enable

Write-Host ""
Write-Host "╔══════════════════════════════════════════════════════╗" -ForegroundColor Green
Write-Host "║  Secure Boot configured successfully!                ║" -ForegroundColor Green
Write-Host "║                                                      ║" -ForegroundColor Green
Write-Host "║  1. Attach saios.iso to the VM                       ║" -ForegroundColor Green
Write-Host "║  2. Make sure Secure Boot is ENABLED in VM settings  ║" -ForegroundColor Green
Write-Host "║  3. Boot the VM — GRUB will load (our key is trusted) ║" -ForegroundColor Green
Write-Host "╚══════════════════════════════════════════════════════╝" -ForegroundColor Green

function print_manual_instructions {
    Write-Host ""
    Write-Host "Manual enrollment via UEFI Shell:" -ForegroundColor Cyan
    Write-Host "  1. Add KeyTool.efi to the ISO:"
    Write-Host "     wsl sudo apt install efitools"
    Write-Host "     cp /usr/share/efitools/efi/KeyTool.efi iso/EFI/BOOT/"
    Write-Host "  2. Boot the ISO → UEFI Shell → run KeyTool.efi"
    Write-Host "  3. Enroll: PK.auth, KEK.auth, db.auth from the ISO"
    Write-Host ""
    Write-Host "Quick disable Secure Boot (temporary):" -ForegroundColor Yellow
    Write-Host "  VirtualBox: Settings → System → Motherboard → Secure Boot → OFF"
    Write-Host "  Then boot normally, run this script, re-enable."
}
