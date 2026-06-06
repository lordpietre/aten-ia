#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
OUTPUT_DIR="${OUTPUT_DIR:-$PROJECT_ROOT/dist}"

echo "==> Building aten-ia portable binary..."
echo "    Project root: $PROJECT_ROOT"
echo "    Output dir: $OUTPUT_DIR"

cd "$PROJECT_ROOT"

if ! command -v docker &> /dev/null; then
    echo "✗ Docker not found. Please install Docker first."
    exit 1
fi

echo "==> Building Docker image..."
docker build -t aten-ia-builder -f docker/Dockerfile.build .

echo "==> Running build in container..."
CONTAINER_ID=$(docker create aten-ia-builder)
docker start -a "$CONTAINER_ID"

mkdir -p "$OUTPUT_DIR"
docker cp "$CONTAINER_ID:/build/memvid-agent-core/target/release/aten-ia" "$OUTPUT_DIR/aten-ia"
docker rm "$CONTAINER_ID"

BINARY="$OUTPUT_DIR/aten-ia"

if [ ! -f "$BINARY" ]; then
    echo "✗ Build failed: binary not found at $BINARY"
    exit 1
fi

echo "==> Build successful!"
echo "    Binary: $BINARY"
echo "    Size: $(du -h "$BINARY" | cut -f1)"

echo ""
echo "==> Run validation:"
echo "    ./scripts/validate-compat.sh $BINARY"
