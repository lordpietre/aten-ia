# Desarrollo

## Entorno

| Herramienta | Versión / notas |
|---|---|
| Rust | >= 1.89.0 (edition 2024, fijado en `rust-toolchain.toml`) |
| CMake | Para compilar llama.cpp |
| clang / libclang | Requerido por `bindgen` |
| libssl-dev | Dependencia de build |
| libgomp-dev | OpenMP para llama.cpp |
| fakeroot | Solo para empaquetar `.deb` |

```bash
rustup toolchain install stable   # si no lo tienes
cd memvid-agent-core
cargo build
```

La primera compilación compila llama.cpp (~30 min). Después es incremental.

### Bibliotecas de llama.cpp

`build.rs` resuelve en este orden:

1. `LLAMA_LOCAL_LIBS=/ruta` — copia los `.a` desde un directorio.
2. Descarga `llama-libs-{target}.tar.gz` desde GitHub Releases (`LLAMA_LIBS_REPO=usuario/repo`).
3. Compilación CMake (`CMAKE_BUILD_PARALLEL_LEVEL`, por defecto 2).

Para desarrollo local puedes compilar una vez y reutilizar:

```bash
cmake -B build-llama -S llama-cpp-turboquant \
  -DLLAMA_BUILD_TOOLS=OFF -DLLAMA_BUILD_EXAMPLES=OFF \
  -DLLAMA_BUILD_TESTS=OFF -DLLAMA_BUILD_SERVER=OFF \
  -DBUILD_SHARED_LIBS=OFF -DGGML_NATIVE=OFF \
  -DCMAKE_BUILD_TYPE=Release
cmake --build build-llama -j"$(nproc)"

mkdir -p llama-libs
find build-llama -name '*.a' -exec cp {} llama-libs/ \;

LLAMA_LOCAL_LIBS=llama-libs cargo build
```

### Cross-compilation

`build.rs` detecta `TARGET != HOST`, genera un toolchain CMake y pasa `--target` a bindgen. Para ARM64 usa `GGML_CPU_ARM_ARCH=armv8-a+dotprod` y `GGML_NATIVE=OFF`.

```bash
rustup target add aarch64-unknown-linux-gnu
cargo build --target aarch64-unknown-linux-gnu
```

El linker se configura en `.cargo/config.toml`.

## Estructura del repositorio

```
.
├── memvid-agent-core/     Crate Rust (lib + bin)
│   ├── src/               23 módulos
│   ├── tests/             Integración
│   ├── build.rs           CMake, bindgen y libs
│   ├── config.example.json  Configuración de ejemplo (config.json está ignorado)
│   └── llama-cpp-turboquant/  Fork de llama.cpp
├── docker/Dockerfile.build
├── scripts/               build-portable.sh, validate-compat.sh
├── man/aten-ia.1
├── wiki/
├── .github/workflows/     CI y release
├── .gitea/workflows/      Mismos workflows para Gitea
├── AGENTS.md              Notas para agentes/desarrolladores
└── PORTABILITY_SPEC.md    Especificación de portabilidad
```

## Tests

```bash
cargo test --locked -- --test-threads=1
cargo test --locked --lib -- --test-threads=1  # solo unitarios
cargo run  --locked -- --self-test              # verificación de uso sin modelo
../scripts/ci/verify.sh                         # suite completa
```

- **~555 tests** entre unitarios e integración.
- Los tests de integración (`tests/functional.rs`, `tests/writer_integration.rs`, `tests/kv_cache_and_history.rs`) usan `tempfile::tempdir()` y `LlamaContext::null()`: **no requieren GGUF**.
- `--test-threads=1` es obligatorio: varios tests mutan `MODEL_PATH` y comparten estado global del proceso.

Cobertura destacada:

| Área | Qué se prueba |
|---|---|
| Config | Persistencia, defaults, validación, overrides |
| Chunker | Estrategias, UTF-8, solapes |
| Extractor | Detección de formato, HTML→texto/Markdown |
| Retrieval | Scoring, filtros, dedup, reindex |
| Memvid | Escritura/lectura, manifest, atomicidad |
| API | Parseo HTTP, auth, rutas, health/models |
| Finetune | Estimaciones puras, corpus, scripts |
| KV-cache | Tipos válidos, ranking de fidelidad |
| CLI | `--version`, `--self-test`, `--verify` |
| Red local | fetch, feeds, lote y límite de tamaño |
| API TCP | health, auth, models, 404, JSON inválido |

## Calidad y CI

```bash
cargo fmt --all -- --check
cargo clippy --locked --lib   # CI solo analiza la lib, no main.rs
```

Pipeline en `.github/workflows/ci.yml` y `.gitea/workflows/ci.yml`:

1. Instala dependencias del sistema.
2. Compila llama.cpp y empaqueta las libs.
3. `cargo build` para `x86_64` y `aarch64`.
4. `cargo test --locked -- --test-threads=1` (solo nativo).
5. `cargo fmt --all -- --check` (solo nativo).
6. `cargo clippy --locked --lib` (solo nativo).
7. CLI smoke: `--version` y `--self-test` (solo nativo).
8. Job `docs`: enlaces, secretos y render del manual.

En tags y manualmente, `verify.yml` ejecuta `scripts/ci/verify.sh`; el release
repite los gates nativos antes de empaquetar.

## Releases

```bash
git tag v0.3.12
git push origin v0.3.12
```

El workflow de release compila en un contenedor `node:20-bullseye` (glibc 2.31) para `x86_64-unknown-linux-gnu` y `aarch64-unknown-linux-gnu` (cross) y publica:

- `aten-ia-<target>.tar.gz` + `.sha256`
- `aten-ia_<versión>_<arch>.deb` + `.sha256`
- Validación de compatibilidad glibc y de que `libstdc++`/`libgomp` sean estáticos

Los artefactos se suben a GitHub Releases y, si `GITEA_REPO` está configurado, a Gitea.

## Build portable local

```bash
./scripts/build-portable.sh
./scripts/validate-compat.sh dist/aten-ia
```

Usa `docker/Dockerfile.build` (Ubuntu 20.04) con `ATEN_PORTABLE=1`. El script de validación comprueba dependencias dinámicas y la matriz de glibc (Ubuntu 20.04+, Debian 12+).

## Convenciones

- Rust edition 2024; ejecuta `cargo fmt` antes de commitear (`rustfmt.toml` fija el estilo).
- `cargo clippy --locked --lib` debe estar limpio; `main.rs` no entra en ese chequeo.
- Las escrituras en disco pasan por `utils::atomic_write`.
- El chunker y cualquier corte de texto deben ser UTF-8 seguros (`floor_char_boundary`, `truncate_chars`).
- Los tests nuevos no deben requerir modelo ni red.
- Mantén la compatibilidad de `config.json`: nuevos campos con `#[serde(default)]`.
