#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE=aurora-appimage-build
TARGET_DIR=target-appimage
APPDIR="$REPO_ROOT/$TARGET_DIR/AppDir"
OUT="$REPO_ROOT/Aurora-x86_64.AppImage"

for tool in podman appimagetool; do
    command -v "$tool" >/dev/null || {
        echo "error: $tool is required but not installed" >&2
        exit 1
    }
done

echo "==> Building the container image"
podman build -t "$IMAGE" -f "$REPO_ROOT/packaging/Containerfile" "$REPO_ROOT/packaging"

echo "==> Compiling Aurora"
podman run --rm \
    -v "$REPO_ROOT:/src:Z" \
    -w /src \
    -e CARGO_TARGET_DIR="/src/$TARGET_DIR" \
    "$IMAGE" \
    cargo build --release -p Aurora

echo "==> Assembling the AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" \
         "$APPDIR/usr/lib/Aurora" \
         "$APPDIR/usr/share/applications" \
         "$APPDIR/usr/share/icons/hicolor/64x64/apps"

install -m755 "$REPO_ROOT/$TARGET_DIR/release/Aurora" "$APPDIR/usr/bin/Aurora"

cp -a "$REPO_ROOT/Bin" "$APPDIR/usr/lib/Aurora/Bin"

cp "$REPO_ROOT/packaging/aurora.desktop" "$APPDIR/aurora.desktop"
cp "$REPO_ROOT/packaging/aurora.desktop" "$APPDIR/usr/share/applications/aurora.desktop"
cp "$REPO_ROOT/production/icons/logo.png" "$APPDIR/aurora.png"
cp "$REPO_ROOT/production/icons/logo.png" \
   "$APPDIR/usr/share/icons/hicolor/64x64/apps/aurora.png"

ln -sf usr/bin/Aurora "$APPDIR/AppRun"

echo "==> Packing the AppImage"
rm -f "$OUT"
ARCH=x86_64 appimagetool "$APPDIR" "$OUT"
chmod +x "$OUT"

echo "==> Built $OUT"
