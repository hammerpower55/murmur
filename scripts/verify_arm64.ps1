# Fails unless every .exe and .dll under the given paths is an ARM64 (AA64)
# binary. Reads the PE header directly, so it needs no Visual Studio tools.
#
#   powershell -ExecutionPolicy Bypass -File scripts/verify_arm64.ps1 <file or folder> ...
#
# Pass -AllowX86Installer to accept the 32 bit x86 NSIS stubs (*-setup.exe
# and the uninstall.exe it installs): NSIS ships no ARM64 stub, so the setup
# and uninstall wrappers are x86 while everything else must still be ARM64.

param(
    [Parameter(Mandatory = $true, ValueFromRemainingArguments = $true)]
    [string[]]$Paths,
    [switch]$AllowX86Installer
)

$ErrorActionPreference = "Stop"
$Machines = @{ 0xAA64 = "ARM64"; 0x8664 = "x64"; 0x014C = "x86"; 0xA641 = "ARM64EC"; 0x01C4 = "ARM" }

function Get-Machine([string]$File) {
    $stream = [IO.File]::OpenRead($File)
    try {
        $reader = New-Object IO.BinaryReader($stream)
        if ($reader.ReadUInt16() -ne 0x5A4D) { return $null }  # "MZ"
        $stream.Position = 0x3C
        $peOffset = $reader.ReadInt32()
        $stream.Position = $peOffset
        if ($reader.ReadUInt32() -ne 0x00004550) { return $null }  # "PE\0\0"
        return [int]$reader.ReadUInt16()
    } finally {
        $stream.Dispose()
    }
}

$files = foreach ($p in $Paths) {
    if (Test-Path $p -PathType Container) {
        Get-ChildItem $p -Recurse -File -Include *.exe, *.dll
    } else {
        Get-Item $p
    }
}

$bad = 0
$stubs = 0
foreach ($f in $files) {
    $machine = Get-Machine $f.FullName
    $name = if ($null -eq $machine) { "not a PE file" } elseif ($Machines.ContainsKey($machine)) { $Machines[$machine] } else { "0x{0:X4}" -f $machine }
    $ok = $machine -eq 0xAA64 -or ($AllowX86Installer -and $machine -eq 0x014C -and ($f.Name -like "*-setup.exe" -or $f.Name -eq "uninstall.exe"))
    Write-Output ("{0,-8} {1,-8} {2}" -f $(if ($ok) { "ok" } else { "FAIL" }), $name, $f.FullName)
    if (-not $ok) { $bad++ }
    elseif ($machine -ne 0xAA64) { $stubs++ }
}

if (@($files).Count -eq 0) { Write-Error "No binaries found in: $($Paths -join ', ')" }
if ($bad -gt 0) { Write-Error "$bad binary file(s) are not ARM64" }
$arm = @($files).Count - $stubs
if ($stubs -gt 0) {
    Write-Output "$arm binaries are ARM64, plus $stubs x86 installer stub(s)"
} else {
    Write-Output "All $arm binaries are ARM64"
}
