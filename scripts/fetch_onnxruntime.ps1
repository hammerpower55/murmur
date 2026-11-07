# Fetches Microsoft's official ONNX Runtime for Windows into
# src-tauri/vendor/ (git-ignored). The Parakeet, Moonshine and SenseVoice
# engines load onnxruntime.dll from there at run time.
#
#   powershell -ExecutionPolicy Bypass -File scripts/fetch_onnxruntime.ps1 [-Arch arm64|x64]
#
# arm64 is the released target and the default; x64 exists only for local
# builds on Intel/AMD machines and is never shipped. Each zip is fetched by
# its own name, never the arm64x package, which contains both.

param(
    [ValidateSet("arm64", "x64")]
    [string]$Arch = "arm64"
)

$ErrorActionPreference = "Stop"
$Version = "1.31.0"

# SHA-256 of the GitHub release assets at v1.31.0.
$Sha256 = @{
    "arm64" = "ef6206fa8d6975878d93a97035d280240ce5764c126a7430e5427e18923316d5"
    "x64"   = "3958d8a44984160692c1b4ffd0de3cb48f8c39f9d34bb31e433db06e30b82195"
}[$Arch]

$Name = "onnxruntime-win-$Arch-$Version"
$Url = "https://github.com/microsoft/onnxruntime/releases/download/v$Version/$Name.zip"

$Vendor = Join-Path $PSScriptRoot "..\src-tauri\vendor"
$Target = Join-Path $Vendor $Name
if (Test-Path (Join-Path $Target "lib\onnxruntime.dll")) {
    Write-Output "Already present: $Target"
    exit 0
}

New-Item -ItemType Directory -Force $Vendor | Out-Null
$Zip = Join-Path $Vendor "$Name.zip"
Write-Output "Downloading $Url"
curl.exe -L --fail -s -S -o $Zip $Url
if ($LASTEXITCODE -ne 0) { throw "download failed" }

$Actual = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToLower()
if ($Actual -ne $Sha256) {
    Remove-Item $Zip
    throw "SHA256 mismatch: expected $Sha256, got $Actual"
}

Expand-Archive $Zip -DestinationPath $Vendor -Force
Remove-Item $Zip
# Debug symbols are hundreds of MB and not needed to build or run.
Get-ChildItem $Target -Recurse -Filter *.pdb | Remove-Item
Write-Output "ONNX Runtime $Version ($Arch) ready in $Target"
