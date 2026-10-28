# Fetches Microsoft's official ONNX Runtime for Windows ARM64 into
# src-tauri/vendor/ (git-ignored). The Parakeet, Moonshine and SenseVoice
# engines load onnxruntime.dll from there at run time.
#
#   powershell -ExecutionPolicy Bypass -File scripts/fetch_onnxruntime.ps1
#
# Pure win-arm64 build only: the win-arm64x package also contains x64 code.

$ErrorActionPreference = "Stop"
$Version = "1.31.0"
$Sha256 = "ef6206fa8d6975878d93a97035d280240ce5764c126a7430e5427e18923316d5"
$Name = "onnxruntime-win-arm64-$Version"
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
# Debug symbols are 430 MB and not needed to build or run.
Get-ChildItem $Target -Recurse -Filter *.pdb | Remove-Item
Write-Output "ONNX Runtime $Version ready in $Target"
