#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BINARY="${1:-$PROJECT_ROOT/memvid-agent-core/target/release/aten-ia}"

echo "==> Validating portable binary: $BINARY"

if [ ! -f "$BINARY" ]; then
    echo "✗ Binary not found: $BINARY"
    exit 1
fi

echo "==> Binary info:"
echo "    Size: $(du -h "$BINARY" | cut -f1)"
echo "    Type: $(file "$BINARY")"

echo ""
echo "==> Dynamic dependencies (should only show libc, libm, libgcc_s, libdl, libpthread):"
ldd "$BINARY" 2>&1 | grep -E "(libc\.so|libm\.so|libgcc_s\.so|libdl\.so|libpthread\.so|librt\.so)" || echo "    (none or minimal)"

echo ""
echo "==> Checking for unwanted dynamic dependencies..."

FAILED=0

if ldd "$BINARY" 2>&1 | grep -q "libstdc++\.so"; then
    echo "✗ Binary depends on libstdc++.so dynamically (should be static)"
    FAILED=1
fi

if ldd "$BINARY" 2>&1 | grep -q "libgomp\.so"; then
    echo "✗ Binary depends on libgomp.so dynamically (should be static)"
    FAILED=1
fi

if [ "$FAILED" -eq 1 ]; then
    echo ""
    echo "✗ Validation FAILED: unwanted dynamic dependencies found"
    exit 1
fi

echo "✓ libstdc++ is statically linked"
echo "✓ libgomp is statically linked"

echo ""
echo "==> Checking glibc version requirement..."

MAX_GLIBC=$(objdump -T "$BINARY" 2>/dev/null | grep -oP 'GLIBC_\K[0-9.]+' | sort -Vru | head -1)

if [ -z "$MAX_GLIBC" ]; then
    echo "✗ Could not determine glibc version requirement"
    exit 1
fi

echo "    Maximum glibc version required: GLIBC_$MAX_GLIBC"

check_compat() {
    local distro="$1"
    local glibc_ver="$2"
    if printf '%s\n' "$MAX_GLIBC" "$glibc_ver" | sort -VC; then
        echo "  ✓ $distro (glibc $glibc_ver): OK"
        return 0
    else
        echo "  ✗ $distro (glibc $glibc_ver): FAIL (requires glibc > $glibc_ver)"
        return 1
    fi
}

echo ""
echo "==> Compatibility matrix:"
echo ""

ALL_OK=true

check_compat "Ubuntu 20.04 LTS" "2.31" || ALL_OK=false
check_compat "Ubuntu 22.04 LTS" "2.35" || ALL_OK=false
check_compat "Ubuntu 24.04 LTS" "2.39" || ALL_OK=false
check_compat "Debian 12 (bookworm)" "2.36" || ALL_OK=false
check_compat "Debian 13 (trixie)" "2.38" || ALL_OK=false

echo ""
if [ "$ALL_OK" = true ]; then
    echo "✓ All compatibility checks PASSED"
    echo ""
    echo "==> All glibc symbols used:"
    objdump -T "$BINARY" 2>/dev/null | grep -oP 'GLIBC_[0-9.]+' | sort -uV
    exit 0
else
    echo "✗ Some compatibility checks FAILED"
    echo ""
    echo "==> All glibc symbols used:"
    objdump -T "$BINARY" 2>/dev/null | grep -oP 'GLIBC_[0-9.]+' | sort -uV
    exit 1
fi