# Installation

## Requisitos

- **Rust** >= 1.85.0 (edition 2024)
- **System deps**: `cmake libssl-dev clang libgomp-dev`
- **Packaging deps**: `fakeroot` (para .deb)

## Paquetes .deb (Ubuntu/Debian) - Recomendado

Descarga desde la pestaña **Releases** de GitHub o Gitea:

```bash
# Ubuntu/Debian (x86_64)
wget https://github.com/lordpietre/aten-ia/releases/latest/download/aten-ia_amd64.deb
sudo dpkg -i aten-ia_amd64.deb
aten-ia  # Ejecutar

# Ubuntu/Debian (ARM64)
wget https://github.com/lordpietre/aten-ia/releases/latest/download/aten-ia_arm64.deb
sudo dpkg -i aten-ia_arm64.deb
aten-ia  # Ejecutar
```

## Binarios portable (otras distribuciones)

Descarga desde releases:
- `aten-ia-x86_64-unknown-linux-gnu.tar.gz`
- `aten-ia-aarch64-unknown-linux-gnu.tar.gz`

```bash
# Extraer
tar -xzf aten-ia-x86_64-unknown-linux-gnu.tar.gz
./aten-ia  # Ejecutar
```

## Build desde código

```bash
git clone https://github.com/lordpietre/aten-ia.git
cd aten-ia/memvid-agent-core

# Primera compilación (~30 min, compila llama.cpp)
cargo build

# O build release (más lento pero binario optimizado)
cargo build --release
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
| `LLAMA_LOCAL_LIBS` | — | Usar libs precompiladas