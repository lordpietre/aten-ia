#!/usr/bin/env bash
# Static documentation checks: required files, local links, secrets, man page.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

python3 "$ROOT/scripts/ci/check_docs.py" "$ROOT"

if command -v groff >/dev/null 2>&1; then
    groff -Kutf8 -man -Tutf8 "$ROOT/man/aten-ia.1" >/dev/null
    echo "  ok    man page renders (groff)"
else
    echo "  skip  groff not installed; man render not checked"
fi
