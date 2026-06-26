# Boot SAIOS install/update media (clears NVRAM so EFI picks up DVD before disk).
# This is not a live-boot path. Runtime operation must boot from HDD.

$vbox  = "C:\Program Files\Oracle\VirtualBox\VBoxManage.exe"
$nvram = "C:\Users\Sanjar.Verma\VirtualBox VMs\SAIOS\SAIOS.nvram"
$iso   = "$PSScriptRoot\saios.iso"
$serialLog = "$PSScriptRoot\seriallog.txt"

& $vbox controlvm "SAIOS" poweroff 2>$null
Start-Sleep 3

Remove-Item $nvram -Force -ErrorAction SilentlyContinue
Write-Host "NVRAM cleared"

& $vbox storageattach "SAIOS" --storagectl "AHCI" --port 1 --device 0 --type dvddrive --medium $iso
Write-Host "Install/update media attached: $iso"

# Capture SAIOS COM1 output to a repository-local log file.
#
# SAIOS initializes a 16550 UART at COM1 (I/O port 0x3F8, IRQ4) and mirrors
# kernel diagnostics there through serial_print!/serial_println!. Routing the
# VM UART to a host file gives a persistent trace for early ring-3 failures,
# including crashes before the framebuffer console can be trusted.
#
# The file is removed before each boot so every run starts with a clean log.
# VirtualBox recreates it as soon as the VM writes to COM1.
Remove-Item $serialLog -Force -ErrorAction SilentlyContinue
& $vbox modifyvm "SAIOS" --uart1 0x3F8 4 --uartmode1 file $serialLog
Write-Host "Serial COM1 log: $serialLog"

& $vbox startvm "SAIOS"
