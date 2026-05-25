# Plan de Integración: `memvid-agent-core` — Estado + Siguientes Pasos

## Estado actual (Mayo 2026)

### Lo que funciona

| Componente | Estado | Notas |
|---|---|---|
| `memvid/` (librería publicada) | ✅ Completo | Compila, tests pasan, CI verde, publicado en crates.io |
| `memvid-agent-core/` código base | ✅ Implementado | Fases 1-8 completas, compila con 0 errores |
| `build.rs` (cmake + bindgen) | ✅ Funcional | Compila llama-cpp-turboquant static libs, genera FFI bindings, parcha `unsafe extern "C"` |
| Submódulo `llama-cpp-turboquant/` | ✅ Inicializado | Apunta a `2cbfdc62`, sincronizado |
| `llama-cpp-turboquant/` standalone | ✅ Copia upstream | Sin modificaciones locales |

### Lo que falta

| Fase | Descripción | Estado |
|---|---|---|
| 9 | Init automático de `memvid_data/` con `core.mv2` real | ✅ Crea `.mv2` válido con identidad/versión vía `memvid-core::Memvid::create()` |
| 10 | Tests | ✅ 28 tests (26 unit + 2 integración) — types, manifest, playlist, writer, utils, writer_integration |
| 11 | CI/CD para `memvid-agent-core` | ✅ Workflow GitHub Actions en `.github/workflows/ci.yml` |
| 12 | Pequeñas mejoras de calidad | ✅ `unwrap()` → `expect()`, `tempfile` dev-dep, advertencias de bindgen toleradas |

---

## Fase 9 — Init automático de `core.mv2`

**Archivo**: `memvid-agent-core/src/memvid/playlist.rs:22-24`

Actualmente es un placeholder:
```rust
if !core_path.exists() {
    std::fs::write(&core_path, [])?;  // ❌ archivo vacío, no es un .mv2 válido
}
```

**Qué hacer**: Usar `memvid-core` para crear un `.mv2` real con:

1. **Identity segment** — nombre del agente, versión, fecha de creación
2. **System prompt / reglas** — instrucciones base del agente
3. **Metadata inicial** — tags como `type=core`, `version=1.0.0`

Código esperado:
```rust
if !core_path.exists() {
    let mut mv = memvid_core::Memvid::create(&core_path)
        .context("Failed to create core.mv2")?;
    let identity = serde_json::json!({
        "agent": "memvid-agent-core",
        "version": env!("CARGO_PKG_VERSION"),
        "created_at": chrono::Utc::now(),
    });
    mv.put_bytes_with_options(
        &serde_json::to_vec(&identity)?,
        memvid_core::PutOptions {
            tags: vec!["type=core".into(), "section=identity".into()],
            ..Default::default()
        },
    )?;
    mv.commit()?;
}
```

**Verificación**: `memvid-core::Memvid::verify(&core_path, false)?` debe pasar.

---

## Fase 10 — Tests

### 10a. Types — serialización roundtrip

Archivo: `memvid-agent-core/src/types.rs` (nuevo `#[cfg(test)]` module)

- Serializar/deserializar `ConversationBatch` a JSON
- Serializar/deserializar `Manifest` completo
- `WriterConfig::default()` produce valores correctos (batch_size=10, segment_max_bytes=50MB)
- `SegmentEntry` con checksum SHA-256

### 10b. Manifest — load/save/create/append

Archivo: `memvid-agent-core/src/memvid/manifest.rs`

- `create_initial_manifest()` produce versión "1.0.0"
- `save_manifest()` + `load_manifest()` roundtrip en temp file
- `append_conversation_to_manifest()` incrementa contador y actualiza timestamp
- Atomic write: save a temp path, verificar que el original no se corrompe si falla

### 10c. Playlist — init, segment rolling

Archivo: `memvid-agent-core/src/memvid/playlist.rs`

- `init()` crea directorios `conversations/`, `knowledge/`, `archive/`
- `init()` crea `core.mv2` (válido post-Fase-9)
- `init()` crea `manifest.json` si no existe
- `next_segment_path()` genera `conv_YYYYMMDD_NNN.mv2`
- `should_roll_segment()` retorna true cuando current_size >= max
- `add_segment()` crea backup y persiste
- Usar `tempfile::tempdir()` para aislar cada test (agregar `tempfile` a dev-dependencies)

### 10d. Writer — flush, batch, atomic rename

Archivo: `memvid-agent-core/src/memvid/writer.rs`

- `init()` con config temporal
- `append_conversation()` acumula hasta `batch_size`
- `flush()` escribe .mv2 vía memvid-core y hace atomic rename
- `flush()` actualiza manifest con SegmentEntry correcto
- Verificar que tras flush el .mv2 existe y `Memvid::verify()` pasa
- `Drop` hace flush automático si hay pendientes
- `should_roll_segment()` crea nuevo segment path al superar el threshold

### 10e. Utils — sha256, atomic_write

Archivo: `memvid-agent-core/src/utils.rs`

- `sha256_digest()` produce hash correcto para inputs conocidos
- `compute_file_checksum()` coincide con `sha256_digest` del contenido
- `atomic_write()`: archivo temporal no queda visible tras rename
- `atomic_write()`: contenido final es correcto

### 10f. Integration — build + FFI

Archivo: `memvid-agent-core/tests/` (integration tests, nuevo directorio)

- `build_links_correctly.rs`: test que verifica que las funciones FFI de llama.h existen en los bindings generados (compila, llama backend init/free sin model — `llama_backend_init()` / `llama_backend_free()`)
- `writer_integration.rs`: ciclo completo — init Playlist, append ConversationBatch, flush, verificar .mv2 con memvid-core, verificar manifest.json

**Nota**: El test de `LlamaContext::init()` requiere un `.gguf` real. Marcar como `#[ignore]` con instrucciones de cómo ejecutarlo.

### Dependencias a agregar

```toml
[dev-dependencies]
tempfile = "3"       # temp dirs para tests
```

---

## Fase 11 — CI/CD

Archivo: `memvid-agent-core/.github/workflows/ci.yml`

Inspirado en `memvid/.github/workflows/ci.yml`:

```yaml
name: CI
on: [push, pull_request]
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          submodules: recursive
      - uses: dtolnay/rust-toolchain@stable
      - run: sudo apt-get update && sudo apt-get install -y cmake libssl-dev clang libgomp1
      - run: cargo build --verbose
      - run: cargo test --verbose
```

Puntos clave:
- `submodules: recursive` — necesario para el submódulo gitsubmodule
- `libgomp1` para OpenMP
- `clang` para bindgen
- `cmake` y `libssl-dev` para compilar llama.cpp

---

## Fase 12 — Mejoras de calidad

### 12a. Clippy cleanup en memvid-agent-core

Actualmente `cargo clippy` produce 0 warnings en memvid-core, pero memvid-agent-core tiene 314 warnings del código generado por bindgen. El código manual debe pasar clippy limpio. Agregar al `lib.rs`:

```rust
// los warnings de bindgen se emiten en OUT_DIR y no se pueden silenciar desde aquí
```

### 12b. Error handling

- `writer.rs:89-91`: `unwrap()` en `file_name()` puede panic si el path termina en `..`
- `writer.rs:51`: `Memvid::create` en el temp path no limpia el temp file si falla — considerar `Drop` guard
- `playlist.rs:43`: `std::fs::copy` con `.ok()` traga errores silenciosamente

### 12c. Re-exportar públicas clean

`lib.rs` actual:
```rust
pub mod agent;
pub mod types;
pub mod utils;
pub mod memvid;
pub mod llama;
```

Considerar si `llama` y `memvid` (internals) deben ser públicos. Al menos `llama` contiene bindings FFI unsafe.

---

## Resumen de prioridades

| Prioridad | Fase | Esfuerzo estimado |
|---|---|---|
| Fase | Estado |
|---|---|---|
| 9 — core.mv2 real | ✅ Completado |
| 10a-e — tests unitarios (21 tests) | ✅ Completado |
| 10f — test integración writer (2 tests) | ✅ Completado |
| 11 — CI | ✅ Completado |
| 12 — mejoras de calidad | ✅ Completado |

**Estado**: Proyecto listo para uso y desarrollo. 28 tests, CI configurado, core.mv2 válido generado automáticamente. Pendiente: agregar un modelo `.gguf` y probar end-to-end con `cargo run`.
