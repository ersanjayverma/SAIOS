param(
    [switch]$Clean
)

$ErrorActionPreference = "Stop"

$repo = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$out = Join-Path $repo "target\userspace\libc"
$include = Join-Path $PSScriptRoot "include"
$src = Join-Path $PSScriptRoot "src"
$example = Join-Path $PSScriptRoot "examples\hello.c"
$linker = Join-Path $repo "userspace\user.ld"

if ($Clean -and (Test-Path $out)) {
    Remove-Item -Recurse -Force $out
}

New-Item -ItemType Directory -Force $out | Out-Null

function Convert-ToWslPath($path) {
    $resolved = Resolve-Path $path
    if ($resolved.Path -notmatch '^([A-Za-z]):\\(.*)$') {
        throw "expected absolute Windows path: $path"
    }
    $drive = $Matches[1].ToLowerInvariant()
    $tail = $Matches[2].Replace('\', '/')
    return "/mnt/$drive/$tail"
}

$wslOut = Convert-ToWslPath $out
$wslInclude = Convert-ToWslPath $include
$wslSrc = Convert-ToWslPath $src
$wslExample = Convert-ToWslPath $example
$wslLinker = Convert-ToWslPath $linker

$cflags = @(
    "-ffreestanding",
    "-fno-stack-protector",
    "-fno-builtin",
    "-fno-pic",
    "-mcmodel=large",
    "-mno-red-zone",
    "-nostdinc",
    "-I$wslInclude",
    "-Wall",
    "-Wextra"
)

wsl gcc -c "$wslSrc/crt0.S" -o "$wslOut/crt0.o"
if ($LASTEXITCODE -ne 0) { throw "crt0.S build failed" }

$objects = @()
foreach ($name in @("errno", "syscall", "unistd", "string", "stdio", "stdlib")) {
    $obj = "$wslOut/$name.o"
    wsl gcc @cflags -c "$wslSrc/$name.c" -o $obj
    if ($LASTEXITCODE -ne 0) { throw "$name.c build failed" }
    $objects += $obj
}

wsl ar rcs "$wslOut/libsaios.a" @objects
if ($LASTEXITCODE -ne 0) { throw "libsaios.a archive failed" }

wsl gcc @cflags -c $wslExample -o "$wslOut/hello.o"
if ($LASTEXITCODE -ne 0) { throw "hello.c build failed" }

wsl ld -static -nostdlib -T $wslLinker -e _start -o "$wslOut/hello_libc.elf" "$wslOut/crt0.o" "$wslOut/hello.o" "$wslOut/libsaios.a"
if ($LASTEXITCODE -ne 0) { throw "hello_libc.elf link failed" }

Write-Host "Built $out\libsaios.a"
Write-Host "Built $out\hello_libc.elf"
