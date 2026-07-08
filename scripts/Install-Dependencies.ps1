<#
.SYNOPSIS
    Installs everything SnapVault needs to build and run on a fresh PC.

.DESCRIPTION
    Installs, if not already present:
      - Node.js LTS   (needed for the React/Vite frontend)
      - Rust          (needed for the Tauri backend)
      - MSVC C++ Build Tools (needed for Rust to compile on Windows)

    Safe to run more than once - it skips anything that's already installed.
    This will ask for an Administrator prompt (a Windows popup asking
    "Do you want to allow this app to make changes?") - click Yes. That's
    needed to install the C++ Build Tools.

    After this finishes, CLOSE this terminal window and open a new one
    before running any other script, so Windows picks up the new tools.

.EXAMPLE
    Right-click this file > "Run with PowerShell"
    -- or, from a PowerShell window opened in this folder --
    .\Install-Dependencies.ps1
#>

# --- Step 0: make sure we're running as Administrator ---------------------
# The C++ Build Tools installer refuses to run quietly unless elevated, so
# instead of failing halfway through, we relaunch ourselves elevated once,
# up front, and ask for the one UAC prompt we need.
$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    Write-Host "This needs an Administrator prompt to install the C++ Build Tools." -ForegroundColor Yellow
    Write-Host "A Windows popup will appear - please click 'Yes'." -ForegroundColor Yellow
    Start-Process -FilePath "powershell.exe" -ArgumentList @(
        "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "`"$PSCommandPath`""
    ) -Verb RunAs -Wait
    exit
}

function Write-Step($text) {
    Write-Host ""
    Write-Host ">> $text" -ForegroundColor Cyan
}

function Write-Done($text) {
    Write-Host "   $text" -ForegroundColor Green
}

# Pick up anything winget just installed without needing a brand new terminal.
function Update-SessionPath {
    $machinePath = [System.Environment]::GetEnvironmentVariable("Path", "Machine")
    $userPath = [System.Environment]::GetEnvironmentVariable("Path", "User")
    $env:Path = "$machinePath;$userPath;C:\Program Files\nodejs;$env:USERPROFILE\.cargo\bin"
}

Write-Host ""
Write-Host "SnapVault dependency installer" -ForegroundColor Cyan
Write-Host "================================"

if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
    Write-Host ""
    Write-Host "winget (the Windows Package Manager) was not found." -ForegroundColor Red
    Write-Host "Please install 'App Installer' from the Microsoft Store, then run this script again."
    exit 1
}

# --- Step 1: Node.js --------------------------------------------------------
Write-Step "Checking Node.js..."
Update-SessionPath
if (Get-Command node -ErrorAction SilentlyContinue) {
    Write-Done "Node.js is already installed ($(node -v))."
} else {
    Write-Done "Installing Node.js LTS (this can take a minute)..."
    winget install OpenJS.NodeJS.LTS --silent --accept-package-agreements --accept-source-agreements
    Update-SessionPath
}

# --- Step 2: Rust ------------------------------------------------------------
Write-Step "Checking Rust..."
Update-SessionPath
if (Get-Command cargo -ErrorAction SilentlyContinue) {
    Write-Done "Rust is already installed ($(cargo --version))."
} else {
    Write-Done "Installing Rust (this can take a minute)..."
    winget install Rustlang.Rustup --silent --accept-package-agreements --accept-source-agreements
    Update-SessionPath
}

# --- Step 3: MSVC C++ Build Tools (Rust needs this to compile on Windows) --
Write-Step "Checking MSVC C++ Build Tools..."
$vswhere = "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vswhere.exe"
$hasCppTools = $false
if (Test-Path $vswhere) {
    $installPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>$null
    if ($installPath) { $hasCppTools = $true }
}

if ($hasCppTools) {
    Write-Done "C++ Build Tools are already installed."
} else {
    Write-Done "Installing Visual Studio Build Tools (this is a big download, please be patient)..."
    winget install Microsoft.VisualStudio.2022.BuildTools --silent --accept-package-agreements --accept-source-agreements

    # winget's install can land the shell without the C++ workload actually
    # selected, so explicitly add it via the VS installer. This step is why
    # we needed Administrator rights at the top of this script.
    $vsInstaller = "C:\Program Files (x86)\Microsoft Visual Studio\Installer\vs_installer.exe"
    $buildToolsPath = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools"
    if (Test-Path $vsInstaller) {
        Write-Done "Adding the C++ workload to Build Tools..."
        Start-Process -FilePath $vsInstaller -ArgumentList @(
            "modify",
            "--installPath", "`"$buildToolsPath`"",
            "--add", "Microsoft.VisualStudio.Workload.VCTools",
            "--includeRecommended",
            "--quiet",
            "--norestart"
        ) -Wait
    }
}

# --- Step 4: WebView2 runtime (needed to display the app's UI) -------------
Write-Step "Checking WebView2 runtime..."
$webview2Key = "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
if (Get-ItemProperty -Path $webview2Key -ErrorAction SilentlyContinue) {
    Write-Done "WebView2 runtime is already installed."
} else {
    Write-Done "Installing WebView2 runtime..."
    winget install Microsoft.EdgeWebView2Runtime --silent --accept-package-agreements --accept-source-agreements
}

Write-Host ""
Write-Host "All done!" -ForegroundColor Green
Write-Host "Please CLOSE this terminal window and open a new one, then run:" -ForegroundColor Yellow
Write-Host "   scripts\Check-Environment.ps1   (to confirm everything installed correctly)"
Write-Host "   scripts\Start-Dev.ps1           (to launch the app)"
Write-Host ""
Read-Host "Press Enter to close this window"
