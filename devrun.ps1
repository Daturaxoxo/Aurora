cargo build -p Aurora
if ($LASTEXITCODE -eq 0) {
    $exePath = ".\target\debug\Aurora.exe"

    if (Get-Command gsudo -ErrorAction SilentlyContinue) {
        gsudo $exePath
    } elseif (Get-Command sudo -ErrorAction SilentlyContinue) {
        sudo $exePath
    } else {
        Start-Process $exePath -Verb RunAs
    }
}
