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
    $targets = [ordered]@{
        ""        = "Everlight.asi"
        "version" = "version.dll"
        "winhttp" = "winhttp.dll"
        "winmm"   = "winmm.dll"
        "dwmapi"  = "dwmapi.dll"
        "dbghelp" = "dbghelp.dll"
        "xinput1_4" = "xinput1_4.dll"
    }

    foreach ($feature in $targets.Keys) {
        $out = $targets[$feature]
        if ($feature) {
            Write-Host "Building proxy ($out)..."
            cargo build -p everlight --features $feature --release
        } else {
            Write-Host "Building engine ($out)..."
            cargo build -p everlight --release
        }
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
        Copy-Item $built "$stage\$out" -Force
    }

    Write-Host "Staged into $root\$stage\:"
    $targets.Values | ForEach-Object { Write-Host "  $_" }

    if ($Deploy) {
        $targets.Values | ForEach-Object { Copy-Item "$stage\$_" "Bin\nte\$_" -Force }
        Write-Host "Deployed all into Bin\nte\"
    }
}
finally {
    Pop-Location
}
