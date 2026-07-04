param(
    [Parameter(Mandatory = $true)]
    [int]$DiskNumber,

    [ValidateSet("release", "debug")]
    [string]$Profile = "release",

    [string]$BootloaderPath,
    [string]$KernelPath,

    [ValidateSet("GPT", "MBR")]
    [string]$PartitionStyle = "GPT",

    [ValidateRange(5, 180)]
    [int]$MountReadyTimeoutSec = 30,

    [switch]$SkipBuild,
    [switch]$Force
)

$ErrorActionPreference = "Stop"
$ScriptVersion = "2026-07-03.8"

function Wait-PathReady {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [int]$TimeoutSec
    )

    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    while ($sw.Elapsed.TotalSeconds -lt $TimeoutSec) {
        if (Test-Path -LiteralPath $Path) {
            try {
                Get-Item -LiteralPath $Path -ErrorAction Stop | Out-Null
                return $true
            } catch {
                # Path was created but filesystem is still finalizing mount.
            }
        }
        Start-Sleep -Milliseconds 500
    }

    return $false
}

function Invoke-DiskPartScript {
    param(
        [Parameter(Mandatory = $true)]
        [string[]]$Commands
    )

    $tempScript = [System.IO.Path]::GetTempFileName()
    try {
        $Commands | Set-Content -LiteralPath $tempScript -Encoding ASCII
        return (diskpart /s $tempScript 2>&1 | Out-String)
    } finally {
        Remove-Item -LiteralPath $tempScript -ErrorAction SilentlyContinue
    }
}

function Find-BootPartition {
    param(
        [Parameter(Mandatory = $true)]
        [int]$DiskNumber,
        [Parameter(Mandatory = $true)]
        [string]$Layout,
        [int]$TimeoutSec = 20
    )

    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    while ($sw.Elapsed.TotalSeconds -lt $TimeoutSec) {
        if ($Layout -eq "GPT") {
            $espGuid = "{C12A7328-F81F-11D2-BA4B-00A0C93EC93B}"
            $esp = Get-Partition -DiskNumber $DiskNumber -ErrorAction SilentlyContinue |
                Where-Object { $_.GptType -eq $espGuid } |
                Select-Object -First 1
            if ($esp) {
                return $esp
            }
        }

        $first = Get-Partition -DiskNumber $DiskNumber -ErrorAction SilentlyContinue |
            Sort-Object PartitionNumber |
            Select-Object -First 1
        if ($first) {
            return $first
        }

        Start-Sleep -Milliseconds 700
    }

    return $null
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Write-Host "createuefiusb.ps1 version: $ScriptVersion"

if (-not $BootloaderPath) {
    $BootloaderPath = Join-Path $repoRoot "boot\uefi\efi_main\target\x86_64-unknown-uefi\$Profile\efi_main.efi"
}
if (-not $KernelPath) {
    $KernelPath = Join-Path $repoRoot "seed\saios\target\x86_64-unknown-none\$Profile\saios"
}

if (-not $SkipBuild) {
    Write-Host "[1/5] Building UEFI bootloader ($Profile)..."
    Push-Location (Join-Path $repoRoot "boot\uefi\efi_main")
    try {
        cargo build --profile $Profile
    } finally {
        Pop-Location
    }

    Write-Host "[2/5] Building kernel payload ($Profile)..."
    Push-Location (Join-Path $repoRoot "seed\saios")
    try {
        cargo build --target x86_64-unknown-none --profile $Profile
    } finally {
        Pop-Location
    }
}

if (-not (Test-Path -LiteralPath $BootloaderPath)) {
    throw "Bootloader not found: $BootloaderPath"
}
if (-not (Test-Path -LiteralPath $KernelPath)) {
    throw "Kernel payload not found: $KernelPath"
}

$validator = Join-Path $PSScriptRoot "validate-efi.ps1"
if (-not (Test-Path -LiteralPath $validator)) {
    throw "Missing validator script: $validator"
}

Write-Host "[3/5] Validating EFI image..."
& $validator -Path $BootloaderPath
if ($LASTEXITCODE -ne 0) {
    throw "EFI validation failed. Refusing to write USB media."
}

$disk = Get-Disk -Number $DiskNumber -ErrorAction Stop
if ($disk.BusType -eq "USB") {
    Write-Host "Target disk is USB: $($disk.FriendlyName) ($([Math]::Round($disk.Size / 1GB, 2)) GiB)"
} else {
    Write-Warning "Target disk is not reported as USB (BusType=$($disk.BusType))."
}

if ($disk.Size -le 0 -or $disk.OperationalStatus -contains "No Media") {
    throw "Selected disk $DiskNumber reports no media. This is usually a card reader slot or empty removable device. Choose the actual USB flash drive disk number from Get-Disk."
}

if (-not $Force) {
    Write-Warning "This will ERASE disk $DiskNumber completely."
    $confirmation = Read-Host "Type YES to continue"
    if ($confirmation -ne "YES") {
        throw "Aborted by user."
    }
}

try {
    Set-Disk -Number $DiskNumber -IsReadOnly $false -ErrorAction Stop
} catch {
    Write-Warning "Could not clear read-only state on disk ${DiskNumber}: $($_.Exception.Message)"
}

try {
    Set-Disk -Number $DiskNumber -IsOffline $false -ErrorAction Stop
} catch {
    Write-Warning "Could not force online state on disk ${DiskNumber}: $($_.Exception.Message)"
}

$effectivePartitionStyle = $PartitionStyle
if ($disk.BusType -eq "USB" -and $PartitionStyle -eq "GPT") {
    Write-Warning "Removable USB media can reject GPT conversion on some controllers. Falling back to MBR for reliability."
    $effectivePartitionStyle = "MBR"
}

$diskpartCommands = @(
    "select disk $DiskNumber"
    "clean"
)

$lettersInUse = @(
    Get-Volume | Where-Object DriveLetter | Select-Object -ExpandProperty DriveLetter
    Get-PSDrive -PSProvider FileSystem | ForEach-Object { $_.Name }
) | ForEach-Object { $_.ToString().ToUpperInvariant() } | Select-Object -Unique

$targetLetter = ([char[]](82..90) | Where-Object { $lettersInUse -notcontains $_ } | Select-Object -First 1)
if (-not $targetLetter) {
    throw "No free drive letter available in R:..Z: for USB boot partition assignment."
}

if ($effectivePartitionStyle -eq "GPT") {
    $diskpartCommands += @(
        "convert gpt"
        "create partition efi size=512"
        "format quick fs=fat32 label=SAIOS_ESP"
        "assign letter=$targetLetter"
    )
} else {
    $diskpartCommands += @(
        "convert mbr"
        "create partition primary"
        "format quick fs=fat32 label=SAIOS_USB"
        "active"
        "assign letter=$targetLetter"
    )
}

$diskpartCommands += @(
    "list part"
    "list vol"
    "exit"
)

Write-Host "[4/5] Partitioning and formatting disk via DiskPart..."
$diskpartOutput = Invoke-DiskPartScript -Commands $diskpartCommands
Write-Host $diskpartOutput

if ($diskpartOutput -match "DiskPart has encountered an error") {
    throw "DiskPart failed while preparing disk $DiskNumber. See output above."
}

$bootPartition = Find-BootPartition -DiskNumber $DiskNumber -Layout $effectivePartitionStyle -TimeoutSec 25

if (-not $bootPartition) {
    throw "Could not find boot partition on disk $DiskNumber after DiskPart"
}

$bootRoot = "${targetLetter}:\"

if ($bootRoot -and -not (Wait-PathReady -Path $bootRoot -TimeoutSec $MountReadyTimeoutSec)) {
    Write-Warning "Boot path did not become ready within $MountReadyTimeoutSec seconds: $bootRoot"
}

if (-not $bootRoot -or -not (Test-Path -LiteralPath $bootRoot)) {
    throw "Boot volume path unavailable after mounting (disk $DiskNumber)."
}

$bootDir = Join-Path $bootRoot "EFI\BOOT"
$msBootDir = Join-Path $bootRoot "EFI\Microsoft\Boot"
$kernelDir = Join-Path $bootRoot "SAIOS"
[System.IO.Directory]::CreateDirectory($bootDir) | Out-Null
[System.IO.Directory]::CreateDirectory($msBootDir) | Out-Null
[System.IO.Directory]::CreateDirectory($kernelDir) | Out-Null

$bootFile = Join-Path $bootDir "BOOTX64.EFI"
$msBootFile = Join-Path $msBootDir "bootmgfw.efi"
$seedFile = Join-Path $kernelDir "seed.elf"
[System.IO.File]::Copy($BootloaderPath, $bootFile, $true)
[System.IO.File]::Copy($BootloaderPath, $msBootFile, $true)
[System.IO.File]::Copy($KernelPath, $seedFile, $true)

if (-not (Test-Path -LiteralPath $bootFile)) {
    throw "Failed to copy BOOTX64.EFI"
}
if (-not (Test-Path -LiteralPath $seedFile)) {
    throw "Failed to copy seed.elf"
}
if (-not (Test-Path -LiteralPath $msBootFile)) {
    throw "Failed to copy EFI\\Microsoft\\Boot\\bootmgfw.efi"
}

Write-Host "[5/5] Media ready."
Write-Host ""
Write-Host "Partition style: $effectivePartitionStyle"
Write-Host "FAT32 boot volume: $bootRoot"
Write-Host "  - EFI\\BOOT\\BOOTX64.EFI"
Write-Host "  - EFI\\Microsoft\\Boot\\bootmgfw.efi"
Write-Host "  - SAIOS\\seed.elf"
Write-Host ""
Write-Host "Dell UEFI notes:"
Write-Host "  1. Boot mode must be UEFI (CSM/Legacy off)."
Write-Host "  2. If Secure Boot is enabled, unsigned BOOTX64.EFI may be blocked."
Write-Host "  3. Use one-time boot menu (often F12) and pick the UEFI USB entry."
