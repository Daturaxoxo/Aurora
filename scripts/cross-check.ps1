<#
Script for cross-checking Aurora for Linux using WSL.
By default the name of the WSL distro is 'archlinux'.

Examples:
.\cross-check.ps1
Checks the Aurora package for Linux.

.\cross-check.ps1 -Package ipc
Specifies to only check the ipc crate.

.\cross-check.ps1 -Command clippy
Runs clippy.

.\cross-check.ps1 -Command build -Release
Actually cross-builds a Linux binary.

.\cross-check.ps1 -AppImage 2.1.0
Builds the AppImage inside WSL by running 'python ./release.py 2.1.0'.
#>

[CmdletBinding(PositionalBinding = $false)]
param(
    [string]$Package = 'Aurora',

    [ValidateSet('check', 'clippy', 'build', 'test')]
    [string]$Command = 'check',

    [string]$AppImage,

    [string]$Distro = 'archlinux',

    [switch]$Release,

    [switch]$Clean,

    [switch]$DryRun,

    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$CargoArgs
)

$ErrorActionPreference = 'Stop'

$LinuxTargetDir = '$HOME/.cache/aurora-linux-target'

function Fail($Message) {
    Write-Host "cross-check: $Message" -ForegroundColor Red
    exit 1
}

if (-not (Get-Command wsl -ErrorAction SilentlyContinue)) {
    Fail 'wsl.exe not found. Install WSL, then re-run.'
}

$distroList = (& wsl.exe -l -q) -replace "`0", '' | ForEach-Object { $_.Trim() } | Where-Object { $_ }
if ($distroList -notcontains $Distro) {
    Fail "WSL distro '$Distro' not found. Available: $($distroList -join ', '). Pass -Distro <name>."
}

$repoWsl = (& wsl.exe -d $Distro -e wslpath -a "$PSScriptRoot") -replace "`0", ''
$repoWsl = ($repoWsl | Select-Object -First 1).Trim()
if (-not $repoWsl) {
    Fail "Could not translate '$PSScriptRoot' to a WSL path."
}

if ($AppImage) {
    $pyPrelude = @'
export PATH="$HOME/.cargo/bin:$PATH"
if command -v python3 >/dev/null 2>&1; then
  PY=python3
elif command -v python >/dev/null 2>&1; then
  PY=python
else
  echo "cross-check: no python found in WSL." >&2
  exit 127
fi
'@

    $versionQuoted = "'" + ($AppImage -replace "'", "'\''") + "'"
    $script = $pyPrelude + "`n" + "cd '$repoWsl'`nexec `"`$PY`" ./release.py $versionQuoted`n"
    $script = $script -replace "`r", ''

    if ($DryRun) {
        Write-Host "wsl -d $Distro -e bash <script> # script contents:" -ForegroundColor DarkGray
        Write-Host $script
        exit 0
    }

    Write-Host "cross-check: python ./release.py $AppImage  (appimage, via WSL/$Distro)" -ForegroundColor Cyan

    $tmp = [System.IO.Path]::Combine($env:TEMP, "aurora-cross-check-$PID.sh")
    [System.IO.File]::WriteAllText($tmp, $script, (New-Object System.Text.UTF8Encoding $false))
    try {
        $tmpWsl = (& wsl.exe -d $Distro -e wslpath -a "$tmp") -replace "`0", ''
        $tmpWsl = ($tmpWsl | Select-Object -First 1).Trim()
        & wsl.exe -d $Distro -e bash $tmpWsl
        $code = $LASTEXITCODE
    }
    finally {
        Remove-Item $tmp -ErrorAction SilentlyContinue
    }

    if ($code -eq 0) {
        Write-Host "cross-check: OK" -ForegroundColor Green
    }
    else {
        Write-Host "cross-check: FAILED (exit $code)" -ForegroundColor Red
    }
    exit $code
}

$cargo = @($Command)
if ($Package) { $cargo += @('-p', $Package) }
if ($Release) { $cargo += '--release' }
if ($CargoArgs) { $cargo += ($CargoArgs | Where-Object { $_ -ne '--' }) }

$cargoLine = ($cargo | ForEach-Object {
        if ($_ -match '[^\w\-=./:+]') { "'" + ($_ -replace "'", "'\''") + "'" } else { $_ }
    }) -join ' '

$prelude = @'
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TERM_COLOR=always
export CARGO_TARGET_DIR="__TARGETDIR__"
if ! command -v cargo >/dev/null 2>&1; then
  echo "cross-check: no cargo found in WSL. Install rustup and a nightly toolchain." >&2
  exit 127
fi
case "$(rustc --version 2>/dev/null)" in
  *nightly*) ;;
  *) echo "cross-check: WARNING: rustc in WSL is not nightly; this tree uses nightly features." >&2 ;;
esac
'@ -replace '__TARGETDIR__', $LinuxTargetDir

$cleanLine = ''
if ($Clean) {
    $cleanLine = "rm -rf `"$LinuxTargetDir`"`n"
}

$script = $prelude + "`n" + $cleanLine + "cd '$repoWsl'`nexec cargo $cargoLine`n"
$script = $script -replace "`r", ''

if ($DryRun) {
    Write-Host "wsl -d $Distro -e bash <script> # script contents:" -ForegroundColor DarkGray
    Write-Host $script
    exit 0
}

Write-Host "cross-check: cargo $cargoLine  (linux, via WSL/$Distro)" -ForegroundColor Cyan

$tmp = [System.IO.Path]::Combine($env:TEMP, "aurora-cross-check-$PID.sh")
[System.IO.File]::WriteAllText($tmp, $script, (New-Object System.Text.UTF8Encoding $false))
try {
    $tmpWsl = (& wsl.exe -d $Distro -e wslpath -a "$tmp") -replace "`0", ''
    $tmpWsl = ($tmpWsl | Select-Object -First 1).Trim()
    & wsl.exe -d $Distro -e bash $tmpWsl
    $code = $LASTEXITCODE
}
finally {
    Remove-Item $tmp -ErrorAction SilentlyContinue
}

if ($code -eq 0) {
    Write-Host "cross-check: OK" -ForegroundColor Green
}
else {
    Write-Host "cross-check: FAILED (exit $code)" -ForegroundColor Red
}
exit $code
