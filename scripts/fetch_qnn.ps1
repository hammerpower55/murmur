# Fetches Qualcomm's QNN execution provider for ONNX Runtime (the
# onnxruntime-qnn plugin, Windows ARM64 wheel from PyPI) into
# src-tauri/vendor/ (git-ignored). Murmur registers it at run time to run
# NPU builds of models on the Snapdragon X Hexagon NPU, and falls back to
# the CPU when it is missing or the PC has no such NPU.
#
#   powershell -ExecutionPolicy Bypass -File scripts/fetch_qnn.ps1
#
# Kept: the plugin, the HTP backend, its graph compiler (QnnHtpPrepare, for
# models that are not precompiled, compiled once on first load) and the
# Hexagon V73 files of Snapdragon X. Every DLL is ARM64; the
# .so files are Hexagon DSP code that runs on the NPU itself. Qualcomm's
# license allows shipping these inside an application, not on their own, so
# its license files stay next to them.

$ErrorActionPreference = "Stop"
$Version = "2.7.0"
$Sha256 = "da52c20c0934b2e35b7197ce17333687296da0f11f1f9a60e39858d5a662ea8e"
$Url = "https://files.pythonhosted.org/packages/88/74/97e08eba0a2ac3704840a25f3a74a0c000902e1c57713d188c84651faf9f/onnxruntime_qnn-$Version-cp313-cp313-win_arm64.whl"
$Keep = @(
    "onnxruntime_providers_qnn.dll",
    "QnnHtp.dll",
    "QnnHtpPrepare.dll",
    "QnnSystem.dll",
    "QnnHtpV73Stub.dll",
    "libQnnHtpV73Skel.so",
    "libqnnhtpv73.cat",
    "LICENSE",
    "Qualcomm_LICENSE.pdf",
    "ThirdPartyNotices.txt",
    "Privacy.md"
)

$Vendor = Join-Path $PSScriptRoot "..\src-tauri\vendor"
$Target = Join-Path $Vendor "onnxruntime-qnn-$Version"
if (Test-Path (Join-Path $Target "QnnHtpPrepare.dll")) {
    Write-Output "Already present: $Target"
    exit 0
}

New-Item -ItemType Directory -Force $Vendor | Out-Null
$Zip = Join-Path $Vendor "onnxruntime-qnn-$Version.zip"
Write-Output "Downloading $Url"
curl.exe -L --fail -s -S -o $Zip $Url
if ($LASTEXITCODE -ne 0) { throw "download failed" }

$Actual = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToLower()
if ($Actual -ne $Sha256) {
    Remove-Item $Zip
    throw "SHA256 mismatch: expected $Sha256, got $Actual"
}

$Unpacked = Join-Path $Vendor "onnxruntime-qnn-$Version-wheel"
Expand-Archive $Zip -DestinationPath $Unpacked -Force
Remove-Item $Zip
New-Item -ItemType Directory -Force $Target | Out-Null
foreach ($name in $Keep) {
    Copy-Item (Join-Path $Unpacked "onnxruntime_qnn\$name") $Target
}
Remove-Item $Unpacked -Recurse -Force
Write-Output "QNN execution provider $Version ready in $Target"
