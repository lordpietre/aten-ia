#!/usr/bin/env bash
# Build a Debian/Ubuntu .deb from a release binary.
#
# Usage:
#   ./scripts/package-deb.sh [--binary PATH] [--output DIR] [--arch ARCH]
#
# Defaults:
#   binary: dist/aten-ia (falls back to memvid-agent-core/target/release/aten-ia)
#   output: dist
#   arch:   dpkg --print-architecture
#
# The package contains the binary, man page, README/AGENTS/SECURITY and the
# Apache-2.0 license. Runtime dependency is only libc6, derived from the
# maximum GLIBC symbol version required by the binary.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

BINARY="dist/aten-ia"
OUTPUT_DIR="dist"
ARCH=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --binary) BINARY="$2"; shift 2 ;;
        --output) OUTPUT_DIR="$2"; shift 2 ;;
        --arch) ARCH="$2"; shift 2 ;;
        -h|--help)
            sed -n '2,18p' "$0"
            exit 0
            ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

if [[ ! -f "$BINARY" ]]; then
    if [[ -f memvid-agent-core/target/release/aten-ia ]]; then
        BINARY="memvid-agent-core/target/release/aten-ia"
    else
        echo "error: binary not found: $BINARY" >&2
        echo "build it first with ./scripts/build-portable.sh or cargo build --release" >&2
        exit 1
    fi
fi

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' memvid-agent-core/Cargo.toml | head -1)"
if [[ -z "$VERSION" ]]; then
    echo "error: could not read version from Cargo.toml" >&2
    exit 1
fi

if [[ -z "$ARCH" ]]; then
    ARCH="$(dpkg --print-architecture)"
fi

MAX_GLIBC="$(objdump -T "$BINARY" 2>/dev/null | grep -oP 'GLIBC_\K[0-9.]+' | sort -Vru | head -1 || true)"
if [[ -z "$MAX_GLIBC" ]]; then
    echo "warning: could not detect glibc requirement; using 2.31" >&2
    MAX_GLIBC="2.31"
fi

for f in man/aten-ia.1 LICENSE README.md AGENTS.md SECURITY.md; do
    [[ -f "$f" ]] || { echo "error: missing $f" >&2; exit 1; }
done

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

install -Dm755 "$BINARY" "$STAGE/usr/bin/aten-ia"
install -Dm644 man/aten-ia.1 "$STAGE/usr/share/man/man1/aten-ia.1"
gzip -9n "$STAGE/usr/share/man/man1/aten-ia.1"

DOCDIR="$STAGE/usr/share/doc/aten-ia"
mkdir -p "$DOCDIR"
install -m644 README.md "$DOCDIR/README.md"
install -m644 AGENTS.md "$DOCDIR/AGENTS.md"
install -m644 SECURITY.md "$DOCDIR/SECURITY.md"
install -m644 LICENSE "$DOCDIR/copyright"
gzip -9n "$DOCDIR/README.md" "$DOCDIR/AGENTS.md" "$DOCDIR/SECURITY.md"
chmod 644 "$DOCDIR"/*

cat > "$DOCDIR/changelog.Debian" <<EOF
aten-ia ($VERSION) unstable; urgency=medium

  * Release $VERSION.

 -- Dinámicas Electrónicas <dinamicas.electronicas@gmail.com>  $(date -u -R)
EOF
gzip -9n "$DOCDIR/changelog.Debian"
chmod 644 "$DOCDIR"/*

INSTALLED_SIZE="$(du -sk "$STAGE/usr" | cut -f1)"
mkdir -p "$STAGE/DEBIAN"
cat > "$STAGE/DEBIAN/control" <<EOF
Package: aten-ia
Version: $VERSION
Architecture: $ARCH
Maintainer: Dinámicas Electrónicas <dinamicas.electronicas@gmail.com>
Installed-Size: $INSTALLED_SIZE
Depends: libc6 (>= $MAX_GLIBC), libgcc-s1
Suggests: cmake, clang, libclang-dev, gcc, pkg-config
Section: utils
Priority: optional
Homepage: https://github.com/lordpietre/aten-ia
Description: Agente de IA local con memoria persistente, RAG y API OpenAI
 aten-ia ejecuta modelos GGUF en CPU con llama.cpp (fork TurboQuant),
 guarda conversaciones y conocimiento en segmentos .mv2 y expone una API
 HTTP compatible con OpenAI. Incluye RAG por palabras clave e ingesta de
 PDF, EPUB, HTML, Markdown, URLs y feeds RSS/Atom.
EOF

(
    cd "$STAGE"
    find usr -type f -print0 | sort -z | xargs -0 md5sum > DEBIAN/md5sums
)

mkdir -p "$OUTPUT_DIR"
# Normalize directory modes: mktemp creates 0700 and would leak into the
# package root entry.
chmod 755 "$STAGE"
find "$STAGE" -type d -exec chmod 755 {} +
DEB="$OUTPUT_DIR/aten-ia_${VERSION}_${ARCH}.deb"
dpkg-deb --build --root-owner-group --uniform-compression -Zxz "$STAGE" "$DEB" >/dev/null
sha256sum "$DEB" > "$DEB.sha256"

echo "==> Package: $DEB"
dpkg-deb --info "$DEB" | sed -n '1,14p'
echo "==> Contents:"
dpkg-deb --contents "$DEB"
echo "==> Binary dependencies:"
ldd "$BINARY" 2>&1 || true
echo "==> Max GLIBC required: $MAX_GLIBC"
echo "==> Size: $(du -h "$DEB" | cut -f1)"
echo "==> SHA-256: $(cut -d' ' -f1 "$DEB.sha256")"

# Smoke test the packaged binary without installing it.
EXTRACT="$(mktemp -d)"
trap 'rm -rf "$STAGE" "$EXTRACT"' EXIT
dpkg-deb --extract "$DEB" "$EXTRACT"
if ! "$EXTRACT/usr/bin/aten-ia" --self-test >/tmp/aten-ia-deb-self-test.log 2>&1; then
    echo "error: packaged binary failed --self-test" >&2
    cat /tmp/aten-ia-deb-self-test.log >&2
    exit 1
fi
echo "==> Packaged binary self-test: OK"
