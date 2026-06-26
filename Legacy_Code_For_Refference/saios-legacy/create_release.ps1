# SAIOS GitHub release script

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $RepoRoot

$versionFile = Join-Path $RepoRoot "shared_version.rs"
if (-not (Test-Path $versionFile)) {
    throw "shared_version.rs not found at $versionFile"
}

$versionText = Get-Content $versionFile -Raw
$versionMatch = [regex]::Match($versionText, 'SAIOS_VERSION:\s*&str\s*=\s*"([^"]+)"')
$tagMatch = [regex]::Match($versionText, 'SAIOS_VERSION_TAG:\s*&str\s*=\s*"([^"]+)"')
if (-not $versionMatch.Success -or -not $tagMatch.Success) {
    throw "Could not parse version constants from shared_version.rs"
}

$Version = $versionMatch.Groups[1].Value
$TagName = $tagMatch.Groups[1].Value
$ReleaseTitle = "SAIOS $TagName"
$cargoToml = Join-Path $RepoRoot "Cargo.toml"
$cargoTomlText = Get-Content $cargoToml -Raw
$cargoVersionMatch = [regex]::Match($cargoTomlText, '(?ms)^\[workspace\.package\]\s+version\s*=\s*"([^"]+)"')
if (-not $cargoVersionMatch.Success) {
    throw "Could not parse [workspace.package].version from Cargo.toml"
}

$CargoVersion = $cargoVersionMatch.Groups[1].Value
if ($CargoVersion -ne $Version) {
    throw "Version mismatch: shared_version.rs=$Version but Cargo.toml=$CargoVersion"
}

$ReleaseNotes = Join-Path $RepoRoot "RELEASE_NOTES_$TagName.md"
if (-not (Test-Path $ReleaseNotes)) {
    throw "Release notes file not found: $(Split-Path $ReleaseNotes -Leaf)"
}

Write-Host "SAIOS Release Script" -ForegroundColor Cyan
Write-Host "====================" -ForegroundColor Cyan
Write-Host "Version: $Version" -ForegroundColor Cyan
Write-Host "Tag:     $TagName" -ForegroundColor Cyan

$iso = Get-Item (Join-Path $RepoRoot "saios.iso") -ErrorAction SilentlyContinue
if ($null -eq $iso) {
    $iso = Get-ChildItem -Path $RepoRoot -Filter *.iso | Sort-Object LastWriteTime -Descending | Select-Object -First 1
}
if ($null -eq $iso) {
    throw "No ISO file found in $RepoRoot. Build one first with .\build.ps1"
}

Write-Host "Found ISO: $($iso.Name)" -ForegroundColor Green
Write-Host "Release notes: $(Split-Path $ReleaseNotes -Leaf)" -ForegroundColor Green

$localTag = git tag --list $TagName
if (-not $localTag) {
    Write-Host "Creating local tag $TagName..." -ForegroundColor Yellow
    git tag $TagName
}

Write-Host "Pushing tag..." -ForegroundColor Yellow
git push origin $TagName

Write-Host "Checking release..." -ForegroundColor Yellow
gh release view $TagName *> $null
$releaseExists = ($LASTEXITCODE -eq 0)

if ($releaseExists) {
    Write-Host "Release exists. Updating notes and uploading ISO..." -ForegroundColor Yellow
    gh release edit `
        $TagName `
        --title $ReleaseTitle `
        --notes-file $ReleaseNotes
    gh release upload `
        $TagName `
        $iso.FullName `
        --clobber
}
else {
    Write-Host "Creating release..." -ForegroundColor Yellow
    gh release create `
        $TagName `
        $iso.FullName `
        --title $ReleaseTitle `
        --notes-file $ReleaseNotes
}

Write-Host ""
Write-Host "SUCCESS" -ForegroundColor Green
Write-Host "Release URL:"
Write-Host "https://github.com/ersanjayverma/SAIOS/releases/tag/$TagName"