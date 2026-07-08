<#
.SYNOPSIS
    Checks whether this PC has everything SnapVault needs to build and run.

.DESCRIPTION
    This script does NOT install or change anything - it only looks and
    reports. Run this first on a new PC (or if something feels broken) to
    see what's missing. If anything shows a red [MISSING], run
    Install-Dependencies.ps1 to fix it.

.EXAMPLE
    Right-click this file > "Run with PowerShell"
    -- or, from a PowerShell window opened in this folder --
    .\Check-Environment.ps1
#>

function Write-Pass($label, $detail) {
    Write-Host "  [OK]      " -ForegroundColor Green -NoNewline
    Write-Host "$label $detail"
}

function Write-Fail($label, $hint) {
    Write-Host "  [MISSING] " -ForegroundColor Red -NoNewline
    Write-Host "$label"
    if ($hint) {
        Write-Host "             -> $hint" -ForegroundColor Yellow
    }
}

# Pick up any tools that were installed earlier in this same terminal session
# but haven't shown up on PATH yet (Windows only refreshes PATH for *new*
# terminal windows, not ones that are already open).
$machinePath = [System.Environment]::GetEnvironmentVariable("Path", "Machine")
$userPath = [System.Environment]::GetEnvironmentVariable("Path", "User")
$env:Path = "$machinePath;$userPath;$env:Path"

Write-Host ""
Write-Host "SnapVault environment check" -ForegroundColor Cyan
Write-Host "============================"
Write-Host ""

$allGood = $true

# --- Node.js / npm ---
$node = Get-Command node -ErrorAction SilentlyContinue
if ($node) {
    Write-Pass "Node.js" "($(node -v))"
} else {
    Write-Fail "Node.js" "Run Install-Dependencies.ps1 to install it."
    $allGood = $false
}

$npm = Get-Command npm -ErrorAction SilentlyContinue
if ($npm) {
    Write-Pass "npm" "($(npm -v))"
} else {
    Write-Fail "npm" "Comes bundled with Node.js - install Node.js first."
    $allGood = $false
}

# --- Rust / Cargo ---
$cargo = Get-Command cargo -ErrorAction SilentlyContinue
if ($cargo) {
    Write-Pass "Rust (cargo)" "($(cargo --version))"
} else {
    Write-Fail "Rust (cargo)" "Run Install-Dependencies.ps1 to install it."
    $allGood = $false
}

# --- MSVC C++ Build Tools (required for Rust to compile on Windows) ---
$vswhere = "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"
$hasCppTools = $false
if (Test-Path $vswhere) {
    $installPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
    if ($installPath) {
        $hasCppTools = $true
    }
}
if ($hasCppTools) {
    Write-Pass "MSVC C++ Build Tools" "(needed for Rust to compile)"
} else {
    Write-Fail "MSVC C++ Build Tools" "Run Install-Dependencies.ps1 to install it. This one is a big download."
    $allGood = $false
}

# --- WebView2 runtime (needed to actually show the app's UI) ---
$webview2Key = "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
$webview2 = Get-ItemProperty -Path $webview2Key -ErrorAction SilentlyContinue
if ($webview2) {
    Write-Pass "WebView2 runtime" "(version $($webview2.pv))"
} else {
    Write-Fail "WebView2 runtime" "Usually pre-installed on Windows 11. Run Install-Dependencies.ps1 to install it."
    $allGood = $false
}

Write-Host ""
if ($allGood) {
    Write-Host "Everything looks good! You can run Start-Dev.ps1 to launch the app." -ForegroundColor Green
} else {
    Write-Host "Some things are missing. Run Install-Dependencies.ps1 to fix them." -ForegroundColor Yellow
}
Write-Host ""
