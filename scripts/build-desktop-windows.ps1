# Build the MarkItDown desktop window on Windows.
# Requires Rust 1.89+ with the MSVC toolchain and the Visual Studio C++ build tools.
# The frontend is the plain files in ui/, so Node is not required.
# --locked keeps the hayro-jbig2 0.3.0 pin. Do not cargo update that crate:
# 0.3.1 asks for Rust 1.92.
#Requires -Version 5.1
param(
  [switch]$Run,
  [switch]$Debug
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Fail([string]$Message) {
  throw $Message
}

if ($env:OS -ne 'Windows_NT') {
  Fail 'This script is for Windows. On Linux or macOS run: cargo build -p markitdown-desktop --release --locked'
}

$Root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $Root

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
  Fail @'
cargo was not found.
Install Rust from https://rustup.rs/ and then open a new terminal.
Choose the default MSVC toolchain (x86_64-pc-windows-msvc). The desktop app needs rustc 1.89 or newer.
'@
}

$rustcVersion = & rustc --version
if ($LASTEXITCODE -ne 0) { Fail 'rustc --version failed.' }
if ($rustcVersion -notmatch 'rustc (\d+)\.(\d+)') {
  Fail "Could not read the rustc version from: $rustcVersion"
}
$rustcMajor = [int]$Matches[1]
$rustcMinor = [int]$Matches[2]
if ($rustcMajor -lt 1 -or ($rustcMajor -eq 1 -and $rustcMinor -lt 89)) {
  Fail "rustc $($Matches[1]).$($Matches[2]) is too old. Run: rustup update stable"
}

$rustcVerbose = (& rustc -vV | Out-String)
if ($LASTEXITCODE -ne 0) { Fail 'rustc -vV failed.' }
if ($rustcVerbose -notmatch '(?m)^host:\s+(\S+)') {
  Fail 'Could not read the rustc host triple.'
}
$hostTriple = $Matches[1]
if ($hostTriple -notlike '*-windows-msvc') {
  $cpu = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'aarch64' } else { 'x86_64' }
  Fail @"
rustc is using $hostTriple. This window needs the MSVC toolchain.
Install the Visual Studio C++ build tools, then run:
  rustup toolchain install stable-$cpu-pc-windows-msvc
  rustup default stable-$cpu-pc-windows-msvc
"@
}

$programFiles86 = ${env:ProgramFiles(x86)}
if (-not $programFiles86) { $programFiles86 = $env:ProgramFiles }
$vswhere = Join-Path $programFiles86 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere)) {
  Fail @'
The Visual Studio Installer was not found.
Install "Build Tools for Visual Studio" from
https://visualstudio.microsoft.com/visual-cpp-build-tools/
and select the workload "Desktop development with C++".
That workload supplies MSVC and the Windows SDK. Then open a new terminal and run this script again.
'@
}

$vsInstall = (& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or -not $vsInstall) {
  $vsInstall = (& $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.ARM64 -property installationPath | Out-String).Trim()
}
if ($LASTEXITCODE -ne 0 -or -not $vsInstall) {
  Fail @'
MSVC was not found.
Open the Visual Studio Installer, modify the build tools, and add the workload
"Desktop development with C++". Then open a new terminal and run this script again.
'@
}

$webviewKeys = @(
  'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
  'HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
  'HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
)
$webviewFound = $false
foreach ($key in $webviewKeys) {
  if (Test-Path -LiteralPath $key) {
    $webviewFound = $true
    break
  }
}
if (-not $webviewFound) {
  Write-Warning @'
The WebView2 Runtime was not found. The executable can still be built.
Windows 11 and current Windows 10 include it. To launch the window, install the Evergreen runtime:
https://developer.microsoft.com/microsoft-edge/webview2/
'@
}

Write-Host "Using MSVC from $vsInstall"
$cargoArgs = @('build', '-p', 'markitdown-desktop', '--locked')
$profile = 'debug'
if ($Debug) {
  Write-Host "Building markitdown-desktop (debug, locked) for $hostTriple"
} else {
  $cargoArgs += '--release'
  $profile = 'release'
  Write-Host "Building markitdown-desktop (release, locked) for $hostTriple"
}

& cargo @cargoArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$exe = Join-Path $Root "target\$profile\markitdown-desktop.exe"
if (-not (Test-Path -LiteralPath $exe)) {
  Fail "The build finished but $exe is missing."
}

Write-Host ""
Write-Host "Built $exe"
if ($Run) {
  & $exe
}
