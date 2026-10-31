# scripts\build-everlight.ps1            --- stage into target\everlight\
# scripts\build-everlight.ps1 -Deploy    --- also copy into Bin\nte\

param([switch]$Deploy)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Push-Location $root
try {
    $built = "target\release\everlight.dll"
    $stage = "target\everlight"
    New-Item -ItemType Directory -Force -Path $stage | Out-Null

    Write-Host "Building engine (Everlight.asi)..."
    cargo build -p everlight --release
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    Copy-Item $built "$stage\Everlight.asi" -Force

    Write-Host "Building proxy (version.dll)..."
    cargo build -p everlight --features version --release
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    Copy-Item $built "$stage\version.dll" -Force

    Write-Host "Staged:`n  $root\$stage\Everlight.asi`n  $root\$stage\version.dll"

    if ($Deploy) {
        Copy-Item "$stage\Everlight.asi" "Bin\nte\Everlight.asi" -Force
        Copy-Item "$stage\version.dll"  "Bin\nte\version.dll"   -Force
        Write-Host "Deployed both into Bin\nte\"
    }
}
finally {
    Pop-Location
}
