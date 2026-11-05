# Builds the portable zip from a release build: the app, ONNX Runtime,
# Qualcomm's QNN plugin (in qnn\ next to the exe, where the app looks for
# it), and the licence files. Run after `pnpm tauri build`.
#
#   powershell -ExecutionPolicy Bypass -File scripts/package_portable.ps1
#
# The portable app keeps its data in %APPDATA%\<productName> like the
# installed one. The app name and version come from tauri.conf.json.

$ErrorActionPreference = "Stop"
$Root = Resolve-Path (Join-Path $PSScriptRoot "..")
$Conf = Get-Content (Join-Path $Root "src-tauri\tauri.conf.json") -Raw | ConvertFrom-Json
$Name = $Conf.productName
$Version = $Conf.version
$Release = Join-Path $Root "src-tauri\target\aarch64-pc-windows-msvc\release"
$Runtime = Join-Path $Root "src-tauri\vendor\onnxruntime-win-arm64-1.31.0\lib\onnxruntime.dll"
$Qnn = Join-Path $Root "src-tauri\vendor\onnxruntime-qnn-2.7.0"

$Exe = Join-Path $Release "murmur.exe"
foreach ($required in $Exe, $Runtime, $Qnn) {
    if (-not (Test-Path $required)) { throw "Missing $required. Build the release first." }
}

$Out = Join-Path $Root "dist-release"
$Stage = Join-Path $Out "$Name"
if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
New-Item -ItemType Directory -Force $Stage | Out-Null

Copy-Item $Exe (Join-Path $Stage "$Name.exe")
Copy-Item $Runtime $Stage
Copy-Item $Qnn (Join-Path $Stage "qnn") -Recurse
# Same names as in the installer: Settings > About opens NOTICE next to the exe.
Copy-Item (Join-Path $Root "NOTICE") $Stage
Copy-Item (Join-Path $Root "LICENSE") $Stage

& (Join-Path $PSScriptRoot "verify_arm64.ps1") $Stage

$Zip = Join-Path $Out "${Name}_${Version}_arm64-portable.zip"
if (Test-Path $Zip) { Remove-Item -Force $Zip }
Compress-Archive -Path $Stage -DestinationPath $Zip
Remove-Item -Recurse -Force $Stage
Write-Output "Wrote $Zip ($([math]::Round((Get-Item $Zip).Length / 1MB, 1)) MB)"
