#!/usr/bin/env bash
set -euo pipefail

# Type-checks the workspace against the shipped Windows target from Linux.
# Extra arguments are forwarded, e.g. ./cross-check.sh -p ipc

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET=x86_64-pc-windows-msvc
XWIN_CACHE="${XWIN_CACHE_DIR:-$HOME/.cache/cargo-xwin}/xwin"
INCLUDE_DIRS=(
    "sdk/include/um" "sdk/include/shared" "sdk/include/ucrt" "sdk/include/winrt"
    "crt/include"
)

for tool in cargo rustup clang-cl; do
    command -v "$tool" >/dev/null || {
        echo "error: $tool is required but not installed" >&2
        exit 1
    }
done

if ! cargo xwin --version >/dev/null 2>&1; then
    echo "error: cargo-xwin is required: cargo install cargo-xwin" >&2
    exit 1
fi

if ! rustup target list --installed | grep -qx "$TARGET"; then
    echo "==> Adding the $TARGET target"
    rustup target add "$TARGET"
fi

fix_header_case() {
    local made=0 dir base actual name
    [ -d "$XWIN_CACHE" ] || return 0

    local sources=()
    while IFS= read -r d; do
        sources+=("$d")
    done < <(find "${CARGO_HOME:-$HOME/.cargo}/registry/src" -maxdepth 2 -type d \
        -name 'unrar-ng-sys-*' 2>/dev/null)
    [ ${#sources[@]} -gt 0 ] || return 0

    while IFS= read -r name; do
        base="$(basename "$name")"
        for dir in "${INCLUDE_DIRS[@]}"; do
            dir="$XWIN_CACHE/$dir"
            [ -d "$dir" ] || continue
            [ -e "$dir/$base" ] && continue
            actual="$(ls "$dir" | grep -ixF "$base" | head -1 || true)"
            if [ -n "$actual" ]; then
                ln -sf "$actual" "$dir/$base"
                made=$((made + 1))
            fi
        done
    done < <(grep -rhoE '#include[[:space:]]*<[^>]+>' "${sources[@]}" 2>/dev/null |
        sed -E 's/.*<([^>]+)>/\1/' | sort -u)

    [ "$made" -gt 0 ] && echo "==> Linked $made mis-cased SDK header(s)"
    return 0
}

export CL="${CL:-} -mssse3 -msse4.1 -maes -mpclmul"
export XWIN_ACCEPT_LICENSE=1

cd "$REPO_ROOT"
fix_header_case

if cargo xwin check --target "$TARGET" --workspace "$@"; then
    exit 0
fi

before="$(find "$XWIN_CACHE" -type l 2>/dev/null | wc -l)"
fix_header_case
after="$(find "$XWIN_CACHE" -type l 2>/dev/null | wc -l)"
[ "$before" = "$after" ] && exit 1

echo "==> Retrying after repairing the freshly downloaded SDK" >&2
exec cargo xwin check --target "$TARGET" --workspace "$@"
