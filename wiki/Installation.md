# Instalación

## Requisitos

- **Rust** >= 1.89.0 (edition 2024, fijado por `rust-toolchain.toml`)
- **Dependencias de build:** `build-essential cmake libssl-dev clang libclang-dev`
- **Empaquetado `.deb`:** `fakeroot`
- Opcional: Docker, solo para el build portable reproducible

## Paquetes `.deb` (Ubuntu/Debian) — recomendado

Descarga el paquete desde la pestaña **Releases** del repositorio (GitHub o Gitea). El nombre incluye la versión y la arquitectura:

```
aten-ia_<versión>_amd64.deb
aten-ia_<versión>_arm64.deb
```

```bash
# x86_64
sudo dpkg -i aten-ia_0.3.12_amd64.deb

# ARM64
sudo dpkg -i aten-ia_0.3.12_arm64.deb

aten-ia
```

El paquete instala:

| Ruta | Contenido |
|---|---|
| `/usr/bin/aten-ia` | Binario |
| `/usr/share/man/man1/aten-ia.1.gz` | Página de manual |
| `/usr/share/doc/aten-ia/` | README y AGENTS |

Dependencias declaradas: `libc6 (>= 2.31)`, `libssl3`, `pkg-config`. Se recomiendan `gcc`, `cmake` y `clang` solo si vas a compilar desde fuente.

## Binarios portables

Para otras distribuciones, usa los archivos de Releases:

```
aten-ia-x86_64-unknown-linux-gnu.tar.gz
aten-ia-aarch64-unknown-linux-gnu.tar.gz
```

```bash
tar -xzf aten-ia-x86_64-unknown-linux-gnu.tar.gz
./aten-ia
```

Ambos se compilan en un contenedor con glibc 2.31 y exigen como máximo **GLIBC_2.31**. Son compatibles con Ubuntu 20.04+ y Debian 11+.

Cada archivo incluye su checksum SHA-256 (`.sha256`):

```bash
sha256sum -c aten-ia-x86_64-unknown-linux-gnu.tar.gz.sha256
```

## Build desde código

```bash
git clone https://github.com/lordpietre/aten-ia.git
cd aten-ia/memvid-agent-core

cargo build              # debug
cargo build --release    # optimizado
```

La primera compilación tarda ~30 min porque compila llama.cpp. Para evitarlo, `build.rs` intenta, en orden:

1. Copiar bibliotecas estáticas desde `LLAMA_LOCAL_LIBS=/ruta`.
2. Descargar `llama-libs-{target}.tar.gz` desde GitHub Releases (`LLAMA_LIBS_REPO` para cambiar el repo).
3. Compilar con CMake (`CMAKE_BUILD_PARALLEL_LEVEL`, por defecto 2).

Comprobar la versión:

```bash
cargo run -- --version    # aten-ia 0.3.12
# o, con el binario compilado:
./target/release/aten-ia --version
```

### Build portable (glibc antiguo)

```bash
./scripts/build-portable.sh
./scripts/validate-compat.sh dist/aten-ia
./scripts/package-deb.sh
```

Genera `dist/aten-ia_<versión>_<arch>.deb` con binario, manual y documentación.
Solo depende de `libc6` y `libgcc-s1`.

Requiere Docker. Compila en Ubuntu 20.04 con `ATEN_PORTABLE=1`, enlazando `libstdc++` y `libgomp` estáticamente.

## Primer arranque

Si no existe `config.json` en el directorio de trabajo, se inicia un asistente que configura:

1. **Directorio de datos** — por defecto `~/.aten-ia` (o `memvid_data` si no hay `$HOME`).
2. **Modelo** — una de las 21 entradas del catálogo.
3. **Documentación de un lenguaje** — opcional.
4. **Servidor API** — puerto, token generado y configuración de OpenCode.

El modelo se descarga automáticamente de HuggingFace si no está en disco.

> El repositorio incluye `memvid-agent-core/config.example.json` con `data_dir: "memvid_data"`. Cópialo a `config.json` o bórralo para repetir el asistente. `config.json` está en `.gitignore` porque puede contener el token de la API.

## Variables de entorno

| Variable | Default | Descripción |
|---|---|---|
| `MODEL_PATH` | `models/qwen2.5-0.5b.gguf` | Ruta al GGUF |
| `MODEL_NAME` | `Qwen2.5-0.5B-Instruct` | Nombre visible del modelo |
| `MODEL_CTX` | `8192` | Tamaño de contexto |
| `MODEL_URL` | HuggingFace | URL alternativa de descarga |
| `LLAMA_LOCAL_LIBS` | — | Directorio con `.a` precompiladas |
| `LLAMA_LIBS_REPO` | `lordpietre/aten-ia` | Repo de libs precompiladas |
| `LLAMA_FINETUNE_BIN` | — | Binario `llama-finetune` para `/finetune` |
| `ATEN_PORTABLE` | — | `1` enlaza `libstdc++`/`libgomp` estáticamente |
| `ATEN_THREADS` | Núcleos disponibles | Hilos de inferencia |
| `ATEN_LOG` | `warn` | Nivel de log (`error`, `warn`, `info`, `debug`, `trace`) |
| `CMAKE_BUILD_PARALLEL_LEVEL` | `2` | Paralelismo de CMake |
| `TARGET` | Auto | Target triple de compilación cruzada |
| `HOME` | — | Define el `data_dir` por defecto |

Si existe un `.env`, se carga automáticamente antes de leer la configuración. Las variables de modelo se aplican al cargar el binario.
