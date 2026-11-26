#!/usr/bin/env bash
# Desinstalación completa para una instalación .deb.
#
#   1. Elimina los datos del usuario (~/.aten-ia, config.json, modelo GGUF).
#   2. Purga el paquete del sistema.
#
# Ejecútalo como tu usuario, NO con sudo. El script pedirá sudo para purgar.
#   ./scripts/uninstall-deb.sh [--yes]
set -euo pipefail

if [[ "$(id -u)" -eq 0 ]]; then
    echo "error: ejecuta este script como tu usuario, sin sudo." >&2
    echo "       El propio script pedirá sudo para purgar el paquete." >&2
    exit 1
fi

BIN="${BIN:-aten-ia}"
if ! command -v "$BIN" >/dev/null 2>&1; then
    echo "error: no se encontró '$BIN' en PATH." >&2
    exit 1
fi

# El config se crea en el directorio desde el que se ejecuta aten-ia. Si no
# estamos ahí, usa el HOME, que es el caso habitual de la instalación.
if [[ ! -f config.json && -f "$HOME/config.json" ]]; then
    cd "$HOME"
fi

echo "==> Limpiando datos de usuario de aten-ia…"
"$BIN" --purge-data "$@"

echo
echo "==> Purgando el paquete aten-ia…"
sudo apt-get purge -y aten-ia

echo
echo "==> Verificación:"
if command -v "$BIN" >/dev/null 2>&1; then
    echo "  ✗ el binario sigue en PATH: $(command -v "$BIN")"
    exit 1
fi
if dpkg -s aten-ia >/dev/null 2>&1; then
    echo "  ✗ el paquete sigue registrado en dpkg"
    exit 1
fi
echo "  ✓ binario y paquete eliminados"
echo "  ✓ datos de usuario eliminados"
