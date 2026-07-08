<#
.SYNOPSIS
    Launches SnapVault in development mode.

.DESCRIPTION
    Starts the app with live-reload: editing frontend or Rust code will
    automatically rebuild and refresh the running app window. Leave this
    terminal window open while you work - press Ctrl+C in it to stop the app.

    Requires Install-Dependencies.ps1 to have been run at least once, and
    "npm install" to have been run at least once (Clean-Build.ps1 does both).

.EXAMPLE
    .\Start-Dev.ps1
#>

$repoRoot = Split-Path -Parent $PSScriptRoot

# Pick up tools installed earlier without needing a brand new terminal.
$machinePath = [System.Environment]::GetEnvironmentVariable("Path", "Machine")
$userPath = [System.Environment]::GetEnvironmentVariable("Path", "User")
$env:Path = "$machinePath;$userPath;C:\Program Files\nodejs;$env:USERPROFILE\.cargo\bin"

if (-not (Get-Command node -ErrorAction SilentlyContinue) -or -not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "Node.js and/or Rust are not installed." -ForegroundColor Red
    Write-Host "Run scripts\Install-Dependencies.ps1 first, then try again."
    exit 1
}

if (-not (Test-Path (Join-Path $repoRoot "node_modules"))) {
    Write-Host "Dependencies haven't been installed yet." -ForegroundColor Yellow
    Write-Host "Running 'npm install' for you first..."
    Push-Location $repoRoot
    npm install
    Pop-Location
}

Push-Location $repoRoot
try {
    Write-Host ""
    Write-Host "Starting SnapVault... a window should open in a few seconds." -ForegroundColor Cyan
    Write-Host "(Press Ctrl+C here to stop it.)" -ForegroundColor DarkGray
    Write-Host ""
    npm run tauri dev
}
finally {
    Pop-Location
}
