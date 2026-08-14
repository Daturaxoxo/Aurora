$ErrorActionPreference = "Stop"
$target = "x86_64-pc-windows-msvc"
$env:RUSTFLAGS = "-C target-feature=+crt-static -Zunstable-options -Cpanic=immediate-abort"

cargo +nightly build --release -p AuroraInstaller --target $target -Z build-std=std,panic_abort
if ($LASTEXITCODE -ne 0) {
    Write-Error "build failed"
    exit $LASTEXITCODE
}

$out = "./target/$target/release"
foreach ($name in @("AuroraInstaller.exe", "AuroraUninstaller.exe")) {
    $file = Join-Path $out $name
    $mib = [math]::Round((Get-Item $file).Length / 1MB, 2)
    Write-Host "$file  $mib MiB"
}
