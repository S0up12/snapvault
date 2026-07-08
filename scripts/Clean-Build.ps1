<#
.SYNOPSIS
    Wipes all build artifacts and rebuilds SnapVault from scratch.

.DESCRIPTION
    Use this when something feels broken and you want a truly clean build -
    for example after pulling new changes, or if a previous build failed
    halfway through. It removes:
      - node_modules      (frontend dependencies)
      - dist              (built frontend files)
      - src-tauri\target  (built Rust files)
    ...then reinstalls everything and rebuilds.

    Requires Install-Dependencies.ps1 to have been run at least once.

.PARAMETER Release
    Build a final, installable version of the app (produces a .msi / .exe
    installer under src-tauri\target\release\bundle). Without this switch,
    it builds a faster debug version just to confirm everything compiles.

.EXAMPLE
    .\Clean-Build.ps1
    .\Clean-Build.ps1 -Release
#>

param(
    [switch]$Release
)

$ErrorActionPreference = "Stop"

# The repo root is always the parent folder of this scripts\ folder,
# regardless of which folder you happened to open PowerShell in.
$repoRoot = Split-Path -Parent $PSScriptRoot

function Write-Step($text) {
    Write-Host ""
    Write-Host ">> $text" -ForegroundColor Cyan
}

# Pick up tools installed earlier without needing a brand new terminal.
$machinePath = [System.Environment]::GetEnvironmentVariable("Path", "Machine")
$userPath = [System.Environment]::GetEnvironmentVariable("Path", "User")
$env:Path = "$machinePath;$userPath;C:\Program Files\nodejs;$env:USERPROFILE\.cargo\bin"

if (-not (Get-Command node -ErrorAction SilentlyContinue) -or -not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "Node.js and/or Rust are not installed." -ForegroundColor Red
    Write-Host "Run scripts\Install-Dependencies.ps1 first, then try again."
    exit 1
}

Push-Location $repoRoot
try {
    Write-Step "Removing old build files..."
    $pathsToRemove = @(
        (Join-Path $repoRoot "node_modules"),
        (Join-Path $repoRoot "dist"),
        (Join-Path $repoRoot "src-tauri\target")
    )
    foreach ($path in $pathsToRemove) {
        if (Test-Path $path) {
            Write-Host "   Deleting $path"
            Remove-Item -Recurse -Force $path
        }
    }

    Write-Step "Installing frontend dependencies (npm install)..."
    npm install
    if ($LASTEXITCODE -ne 0) { throw "npm install failed." }

    if ($Release) {
        Write-Step "Building the app (release mode - this makes the real installer, and can take several minutes)..."
        npm run tauri build
    } else {
        Write-Step "Building the app (debug mode - quicker, just to confirm it compiles)..."
        npm run tauri build -- --debug
    }
    if ($LASTEXITCODE -ne 0) { throw "Build failed - scroll up to see the error." }

    Write-Host ""
    Write-Host "Build succeeded!" -ForegroundColor Green
    if ($Release) {
        Write-Host "Installer files are under src-tauri\target\release\bundle\"
    } else {
        Write-Host "You can now run scripts\Start-Dev.ps1 to launch the app."
    }
}
finally {
    Pop-Location
}
