<#
.SYNOPSIS
  Puts ffmpeg/ffprobe into src-tauri/binaries for the release build (Tauri sidecars).
.PARAMETER FromPath
  Offline mode: copy ffmpeg.exe/ffprobe.exe found on PATH. No download.
  Default mode downloads the gyan.dev release "essentials" build and verifies its SHA256.
#>
param([switch]$FromPath)
$ErrorActionPreference = 'Stop'
$triple = 'x86_64-pc-windows-msvc'
$dest = Join-Path (Split-Path $PSScriptRoot -Parent) 'src-tauri\binaries'
New-Item -ItemType Directory -Force $dest | Out-Null
$ff = Join-Path $dest "ffmpeg-$triple.exe"
$fp = Join-Path $dest "ffprobe-$triple.exe"
$lic = Join-Path $dest 'FFMPEG-LICENSE.txt'

if ($FromPath) {
    foreach ($pair in @(@('ffmpeg', $ff), @('ffprobe', $fp))) {
        $cmd = Get-Command $pair[0] -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
        if (-not $cmd) { throw "$($pair[0]) not found on PATH" }
        Copy-Item -Force $cmd.Source $pair[1]
    }
    if (-not (Test-Path $lic)) { Set-Content $lic 'FFmpeg license: https://ffmpeg.org/legal.html (copied from system PATH; check your build''s license).' }
    Write-Host "Copied ffmpeg/ffprobe from PATH to $dest"
    return
}

if ((Test-Path $ff) -and (Test-Path $fp) -and (Test-Path $lic)) {
    Write-Host "Already present in $dest (delete the files to refetch)."
    return
}
$url = 'https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip'
$work = Join-Path ([IO.Path]::GetTempPath()) "snapvault-ffmpeg-$([guid]::NewGuid())"
New-Item -ItemType Directory $work | Out-Null
try {
    $zip = Join-Path $work 'ffmpeg.zip'
    Invoke-WebRequest $url -OutFile $zip
    $expected = ((Invoke-WebRequest "$url.sha256").Content.ToString().Trim() -split '\s+')[0].ToLower()
    $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
    if ($actual -ne $expected) { throw "SHA256 mismatch: expected $expected, got $actual" }
    $x = Join-Path $work 'x'
    Expand-Archive $zip $x
    $root = Get-ChildItem $x -Directory | Select-Object -First 1
    Copy-Item -Force (Join-Path $root.FullName 'bin\ffmpeg.exe') $ff
    Copy-Item -Force (Join-Path $root.FullName 'bin\ffprobe.exe') $fp
    Copy-Item -Force (Join-Path $root.FullName 'LICENSE') $lic
    Write-Host "Fetched and verified ffmpeg ($($root.Name)) into $dest"
} finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
