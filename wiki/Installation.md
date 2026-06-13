# Installation

## Requisitos

- **Rust** >= 1.85.0 (edition 2024)
- **System deps**: `cmake libssl-dev clang libgomp-dev`
- **Packaging deps**: `fakeroot` (para .deb)

## Build desde código

```bash
git clone https://github.com/lordpietre/aten-ia.git
cd aten-ia/memvid-agent-core

# Primera compilación (~30 min, compila llama.cpp)
cargo build

# O build release (más lento pero binario optimizado)
cargo build --release
```

## Usar binario precompilado

Descarga desde releases de GitHub o Gitea:
- `aten-ia-x86_64-unknown-linux-gnu.tar.gz`
- `aten-ia-aarch64-unknown-linux-gnu.tar.gz`
- `aten-ia_*.deb` (Debian/Ubuntu)

## Docker (opcional)

```bash
# Build portable
./scripts/build-portable.sh

# Validar compatibilidad
./scripts/validate-compat.sh dist/aten-ia
```

## Configuración inicial

La primera ejecución muestra un wizard interactivo para:
1. Seleccionar modelo LLM
2. Configurar directorio de datos
3. Configurar API (opcional)

Los modelos se descargan automáticamente de HuggingFace si no existen.

## Variables de entorno

| Variable | Default | Descripción |
|----------|---------|-------------|
| `MODEL_PATH` | `models/qwen2.5-0.5b.gguf` | Ruta al modelo GGUF |
| `MODEL_NAME` | `Qwen2.5-0.5B-Instruct` | Nombre del modelo |
| `MODEL_CTX` | `8192` | Tamaño de contexto |
| `LLAMA_LOCAL_LIBS` | — | Usar libs precompiladas |