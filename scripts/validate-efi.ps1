param(
    [Parameter(Mandatory = $true)]
    [string]$Path,
    [switch]$Json
)

$ErrorActionPreference = "Stop"

function Read-U16([byte[]]$Data, [int]$Offset) {
    return [BitConverter]::ToUInt16($Data, $Offset)
}

function Read-U32([byte[]]$Data, [int]$Offset) {
    return [BitConverter]::ToUInt32($Data, $Offset)
}

function Read-U64([byte[]]$Data, [int]$Offset) {
    return [BitConverter]::ToUInt64($Data, $Offset)
}

function Test-Range([int]$Offset, [int]$Length, [int]$Total) {
    if ($Offset -lt 0 -or $Length -lt 0) {
        return $false
    }
    if ($Offset -gt $Total) {
        return $false
    }
    if ($Length -gt ($Total - $Offset)) {
        return $false
    }
    return $true
}

$resolved = (Resolve-Path -LiteralPath $Path).Path
$bytes = [System.IO.File]::ReadAllBytes($resolved)
$fileLength = $bytes.Length

if ($fileLength -lt 512) {
    throw "File is too small to be a valid EFI image: $resolved"
}

if ($bytes[0] -ne 0x4D -or $bytes[1] -ne 0x5A) {
    throw "Invalid DOS header (expected MZ): $resolved"
}

$peOffset = [BitConverter]::ToInt32($bytes, 0x3C)
if (-not (Test-Range $peOffset 24 $fileLength)) {
    throw "Invalid PE header offset ($peOffset) in $resolved"
}

if ($peOffset -lt 0x40) {
    throw "Suspicious PE header offset ($peOffset) in $resolved"
}

$peSig = Read-U32 $bytes $peOffset
if ($peSig -ne 0x00004550) {
    throw "Invalid PE signature at offset $peOffset in $resolved"
}

$machine = Read-U16 $bytes ($peOffset + 4)
$numberOfSections = Read-U16 $bytes ($peOffset + 6)
$characteristics = Read-U16 $bytes ($peOffset + 22)
$sizeOfOptionalHeader = Read-U16 $bytes ($peOffset + 20)

$optionalHeaderOffset = $peOffset + 24
if (-not (Test-Range $optionalHeaderOffset $sizeOfOptionalHeader $fileLength)) {
    throw "Optional header exceeds file bounds in $resolved"
}

if ($sizeOfOptionalHeader -lt 240) {
    throw "PE32+ optional header too small ($sizeOfOptionalHeader bytes) in $resolved"
}

$optionalMagic = Read-U16 $bytes $optionalHeaderOffset
if ($optionalMagic -ne 0x20B) {
    throw "Not a PE32+ image (optional header magic=0x$('{0:X}' -f $optionalMagic))"
}

$sectionAlignment = Read-U32 $bytes ($optionalHeaderOffset + 32)
$fileAlignment = Read-U32 $bytes ($optionalHeaderOffset + 36)
$addressOfEntryPoint = Read-U32 $bytes ($optionalHeaderOffset + 16)
$imageBase = Read-U64 $bytes ($optionalHeaderOffset + 24)
$sizeOfImage = Read-U32 $bytes ($optionalHeaderOffset + 56)
$sizeOfHeaders = Read-U32 $bytes ($optionalHeaderOffset + 60)
$subsystem = Read-U16 $bytes ($optionalHeaderOffset + 68)
$numberOfRvaAndSizes = Read-U32 $bytes ($optionalHeaderOffset + 108)

$relocRva = 0
$relocSize = 0
$dataDirectoryBase = $optionalHeaderOffset + 112
if ($numberOfRvaAndSizes -gt 5 -and (Test-Range $dataDirectoryBase (6 * 8) $fileLength)) {
    $relocRva = Read-U32 $bytes ($dataDirectoryBase + (5 * 8))
    $relocSize = Read-U32 $bytes ($dataDirectoryBase + (5 * 8) + 4)
}

$sectionTableOffset = $optionalHeaderOffset + $sizeOfOptionalHeader
$sectionTableSize = $numberOfSections * 40
$sectionTableInBounds = (Test-Range $sectionTableOffset $sectionTableSize $fileLength)

$hasRelocSection = $false
$relocSectionCoversDirectory = $false
$entryPointInSection = $false
$entryPointInExecutableSection = $false
$sections = @()
for ($i = 0; $i -lt $numberOfSections; $i++) {
    $entry = $sectionTableOffset + ($i * 40)
    if (-not (Test-Range $entry 40 $fileLength)) {
        break
    }

    $nameBytes = $bytes[$entry..($entry + 7)]
    $name = ([System.Text.Encoding]::ASCII.GetString($nameBytes)).Trim([char]0)
    $virtualSize = Read-U32 $bytes ($entry + 8)
    $virtualAddress = Read-U32 $bytes ($entry + 12)
    $sizeOfRawData = Read-U32 $bytes ($entry + 16)
    $pointerToRawData = Read-U32 $bytes ($entry + 20)
    $characteristicsSection = Read-U32 $bytes ($entry + 36)

    $secStart = [uint32]$virtualAddress
    $secSpan = [uint32]([Math]::Max($virtualSize, $sizeOfRawData))
    $secEnd = [uint32]($secStart + $secSpan)
    if ($addressOfEntryPoint -ne 0 -and [uint32]$addressOfEntryPoint -ge $secStart -and [uint32]$addressOfEntryPoint -lt $secEnd) {
        $entryPointInSection = $true
        if (($characteristicsSection -band 0x20000000) -ne 0) {
            $entryPointInExecutableSection = $true
        }
    }

    $sections += [pscustomobject]@{
        Name = $name
        VirtualAddress = $virtualAddress
        VirtualSize = $virtualSize
        SizeOfRawData = $sizeOfRawData
        PointerToRawData = $pointerToRawData
        Characteristics = $characteristicsSection
        RawInBounds = ($sizeOfRawData -eq 0 -or (Test-Range $pointerToRawData $sizeOfRawData $fileLength))
    }

    if ($name -eq ".reloc") {
        $hasRelocSection = $true
        if ($relocRva -ne 0 -and $relocSize -ne 0) {
            $relocStart = [uint32]$relocRva
            $relocEnd = [uint32]($relocStart + $relocSize)
            if ($relocStart -ge $secStart -and $relocEnd -le $secEnd) {
                $relocSectionCoversDirectory = $true
            }
        }
    }
}

$issues = @()
if ($numberOfSections -lt 1 -or $numberOfSections -gt 96) {
    $issues += "NumberOfSections is $numberOfSections; expected range is 1..96."
}
if (-not $sectionTableInBounds) {
    $issues += "Section table exceeds file bounds."
}
if ($machine -ne 0x8664) {
    $issues += "Machine is 0x$('{0:X}' -f $machine), expected 0x8664 (IMAGE_FILE_MACHINE_AMD64)."
}
if ($subsystem -ne 10) {
    $issues += "Subsystem is $subsystem, expected 10 (IMAGE_SUBSYSTEM_EFI_APPLICATION)."
}
if (($characteristics -band 0x0002) -eq 0) {
    $issues += "COFF characteristics do not include executable-image flag (0x0002)."
}
if ($sectionAlignment -lt 0x1000) {
    $issues += "SectionAlignment is 0x$('{0:X}' -f $sectionAlignment), expected at least 0x1000 for UEFI images."
}
if ($sectionAlignment -lt $fileAlignment) {
    $issues += "SectionAlignment (0x$('{0:X}' -f $sectionAlignment)) is smaller than FileAlignment (0x$('{0:X}' -f $fileAlignment))."
}
if ($fileAlignment -eq 0 -or ($fileAlignment -band ($fileAlignment - 1)) -ne 0 -or $fileAlignment -gt 0x10000) {
    $issues += "FileAlignment is invalid (0x$('{0:X}' -f $fileAlignment))."
}
if ($addressOfEntryPoint -eq 0) {
    $issues += "AddressOfEntryPoint is zero."
}
if (($imageBase -band 0xFFFF) -ne 0) {
    $issues += "ImageBase is not 64 KiB aligned (0x$('{0:X}' -f $imageBase))."
}
if ($sizeOfImage -eq 0 -or $sizeOfHeaders -eq 0 -or $sizeOfHeaders -gt $fileLength) {
    $issues += "SizeOfImage/SizeOfHeaders fields are inconsistent with file size."
}
if ($sectionAlignment -ne 0 -and ($sizeOfImage % $sectionAlignment) -ne 0) {
    $issues += "SizeOfImage (0x$('{0:X}' -f $sizeOfImage)) is not aligned to SectionAlignment (0x$('{0:X}' -f $sectionAlignment))."
}
if ($relocRva -eq 0 -or $relocSize -eq 0) {
    $issues += "Base relocation directory is empty; some UEFI firmware rejects non-relocatable images."
}
if (-not $hasRelocSection) {
    $issues += "No .reloc section found in section table."
}
if ($hasRelocSection -and $relocRva -ne 0 -and $relocSize -ne 0 -and -not $relocSectionCoversDirectory) {
    $issues += "Relocation data directory is not contained within the .reloc section."
}
if (-not $entryPointInSection) {
    $issues += "AddressOfEntryPoint does not map to any section."
}
if ($entryPointInSection -and -not $entryPointInExecutableSection) {
    $issues += "AddressOfEntryPoint is not in an executable section."
}

$badRawSections = $sections | Where-Object { -not $_.RawInBounds }
if ($badRawSections.Count -gt 0) {
    foreach ($sec in $badRawSections) {
        $issues += "Section $($sec.Name) raw data points outside file bounds (offset=0x$('{0:X}' -f $sec.PointerToRawData), size=0x$('{0:X}' -f $sec.SizeOfRawData))."
    }
}

for ($i = 0; $i -lt $sections.Count; $i++) {
    $aStart = [uint32]$sections[$i].VirtualAddress
    $aEnd = [uint32]($aStart + [uint32]([Math]::Max($sections[$i].VirtualSize, $sections[$i].SizeOfRawData)))
    for ($j = $i + 1; $j -lt $sections.Count; $j++) {
        $bStart = [uint32]$sections[$j].VirtualAddress
        $bEnd = [uint32]($bStart + [uint32]([Math]::Max($sections[$j].VirtualSize, $sections[$j].SizeOfRawData)))
        if ($aStart -lt $bEnd -and $bStart -lt $aEnd) {
            $issues += "Sections $($sections[$i].Name) and $($sections[$j].Name) have overlapping virtual ranges."
        }
    }
}

$result = [ordered]@{
    path = $resolved
    machine = ('0x{0:X}' -f $machine)
    characteristics = ('0x{0:X}' -f $characteristics)
    subsystem = $subsystem
    numberOfSections = $numberOfSections
    imageBase = ('0x{0:X}' -f $imageBase)
    addressOfEntryPoint = ('0x{0:X}' -f $addressOfEntryPoint)
    sectionAlignment = ('0x{0:X}' -f $sectionAlignment)
    fileAlignment = ('0x{0:X}' -f $fileAlignment)
    sizeOfImage = ('0x{0:X}' -f $sizeOfImage)
    sizeOfHeaders = ('0x{0:X}' -f $sizeOfHeaders)
    relocRva = ('0x{0:X}' -f $relocRva)
    relocSize = $relocSize
    hasRelocSection = $hasRelocSection
    relocSectionCoversDirectory = $relocSectionCoversDirectory
    valid = ($issues.Count -eq 0)
    issues = $issues
}

if ($Json) {
    $result | ConvertTo-Json -Depth 4
} else {
    Write-Host "EFI validation result:"
    Write-Host "  Path:            $($result.path)"
    Write-Host "  Machine:         $($result.machine)"
    Write-Host "  Characteristics: $($result.characteristics)"
    Write-Host "  Subsystem:       $($result.subsystem)"
    Write-Host "  ImageBase:       $($result.imageBase)"
    Write-Host "  EntryPoint:      $($result.addressOfEntryPoint)"
    Write-Host "  Sections:        $($result.numberOfSections)"
    Write-Host "  Alignment:       sec=$($result.sectionAlignment), file=$($result.fileAlignment)"
    Write-Host "  Size img/hdr:    $($result.sizeOfImage) / $($result.sizeOfHeaders)"
    Write-Host "  Reloc RVA/Size:  $($result.relocRva) / $($result.relocSize)"
    Write-Host "  .reloc section:  $($result.hasRelocSection)"
    Write-Host "  Reloc mapped:    $($result.relocSectionCoversDirectory)"

    if ($issues.Count -eq 0) {
        Write-Host "Status: PASS"
    } else {
        Write-Host "Status: FAIL"
        foreach ($issue in $issues) {
            Write-Host "  - $issue"
        }
    }
}

if ($issues.Count -gt 0) {
    exit 2
}

exit 0
