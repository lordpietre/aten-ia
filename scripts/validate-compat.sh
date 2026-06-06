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
echo "==> Dynamic dependencies (should only show libc, libm, libgcc_s):"
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

MAX_GLIBC=$(objdump -T "$BINARY" 2>/dev/null | grep -oP 'GLIBC_\K[0-9.]+' | sort -V | tail -1)

if [ -z "$MAX_GLIBC" ]; then
    echo "✗ Could not determine glibc version requirement"
    exit 1
fi

echo "    Maximum glibc version required: GLIBC_$MAX_GLIBC"

compare_versions() {
    local v1="$1"
    local v2="$2"
    if [ "$(printf '%s\n' "$v1" "$v2" | sort -V | head -1)" = "$v1" ]; then
        return 0
    else
        return 1
    fi
}

echo ""
echo "==> Compatibility check:"

if compare_versions "$MAX_GLIBC" "2.31"; then
    echo "✓ Compatible with Ubuntu 20.04 LTS (glibc 2.31)"
else
    echo "✗ NOT compatible with Ubuntu 20.04 LTS (requires glibc > 2.31)"
    FAILED=1
fi

if compare_versions "$MAX_GLIBC" "2.36"; then
    echo "✓ Compatible with Debian 12 (glibc 2.36)"
else
    echo "✗ NOT compatible with Debian 12 (requires glibc > 2.36)"
    FAILED=1
fi

if compare_versions "$MAX_GLIBC" "2.28"; then
    echo "✓ Compatible with CentOS/RHEL 8 (glibc 2.28)"
else
    echo "✗ NOT compatible with CentOS/RHEL 8 (requires glibc > 2.28)"
    FAILED=1
fi

echo ""
if [ "$FAILED" -eq 0 ]; then
    echo "✓ All compatibility checks PASSED"
    echo ""
    echo "==> All glibc symbols used:"
    objdump -T "$BINARY" 2>/dev/null | grep -oP 'GLIBC_[0-9.]+' | sort -uV
    exit 0
else
    echo "✗ Compatibility checks FAILED"
    echo ""
    echo "==> All glibc symbols used:"
    objdump -T "$BINARY" 2>/dev/null | grep -oP 'GLIBC_[0-9.]+' | sort -uV
    exit 1
fi
