# Plan de Tests — aten-ia

## Versión: 1.0
## Fecha: 2026-06-06

---

## 1. Resumen de Tests Existentes

| Módulo | Tests Unitarios | Tests Integración | Total |
|--------|----------------|-------------------|-------|
| agent.rs | 17 | - | 17 |
| api.rs | 14 | - | 14 |
| books_catalog.rs | 12 | - | 12 |
| chunker.rs | 18 | - | 18 |
| config.rs | 19 | - | 19 |
| context_policy.rs | 11 | - | 11 |
| extractor.rs | 35 | - | 35 |
| feeds.rs | 5 | - | 5 |
| generation.rs | 2 | - | 2 |
| languages_catalog.rs | 16 | - | 16 |
| llama/context.rs | 6 | - | 6 |
| memvid/manifest.rs | 9 | - | 9 |
| memvid/playlist.rs | 11 | - | 11 |
| memvid/reader.rs | 8 | - | 8 |
| memvid/writer.rs | 13 | - | 13 |
| models_catalog.rs | 5 | - | 5 |
| prompt.rs | 14 | - | 14 |
| queue.rs | 9 | - | 9 |
| retrieval.rs | 30 | - | 30 |
| session.rs | 10 | - | 10 |
| types.rs | 18 | - | 18 |
| utils.rs | 14 | - | 14 |
| web_fetcher.rs | 7 | - | 7 |
| functional.rs | - | ~55 | 55 |
| writer_integration.rs | - | 2 | 2 |
| kv_cache_and_history.rs | - | 5 | 5 |
| shutdown.rs | 2 | - | 2 |
| **Total** | **~280** | **~62** | **~342** |

## 2. Fallos Potenciales Encontrados

### 2.1 Alta Severidad (Pánico en Producción)

| ID | Archivo:Línea | Descripción | Estado |
|----|---------------|-------------|--------|
| F-01 | `main.rs` (múltiples) | `agent.lock().unwrap()` puede panic si Mutex está envenenado | Arreglado: shutdown graceful reduce riesgo |
| F-02 | `main.rs:1015` | `parts.next().unwrap()` pánico si argumento vacío | **ARREGLADO** |
| F-03 | `writer.rs:117,203` | `.expect("segment path has no file name")` panic si path termina en `/` | **ARREGLADO** |

### 2.2 Severidad Media

| ID | Archivo:Línea | Descripción | Estado |
|----|---------------|-------------|--------|
| F-04 | `api.rs:317` | `.unwrap()` en `find("\r\n\r\n")` - ya validado previamente | Aceptable |
| F-05 | `chunker.rs:190` | `max_size=0` retorna vector vacío (no documentado) | **Test agregado** |
| F-06 | `retrieval.rs` | `remove_by_source_prefix("")` elimina todas las entradas | **Test agregado** |
| F-07 | `api.rs` | Threads sin join en shutdown | Aceptable (timeout 30s limita daño) |

### 2.3 Baja Severidad

| ID | Archivo:Línea | Descripción | Estado |
|----|---------------|-------------|--------|
| F-08 | `web_fetcher.rs:130` | `global_throttle` race condition benigna con `AtomicU64` | Aceptable |
| F-09 | `utils.rs` | `FileLock` no atómico en NFS | Documentado |
| F-10 | `config.rs` | JSON con tipos incorrectos causa error, no panic | **Test agregado** |

## 3. Tests Nuevos Agregados

| Test | Archivo | Módulo Cubierto | Edge Case |
|------|---------|----------------|-----------|
| `chunker_max_size_zero_returns_empty` | functional.rs | chunker | max_size=0 Fixed |
| `chunker_max_size_zero_paragraph_strategy` | functional.rs | chunker | max_size=0 Paragraph |
| `chunker_max_size_zero_heading_strategy` | functional.rs | chunker | max_size=0 Heading |
| `chunker_unicode_cjk` | functional.rs | chunker | CJK multibyte |
| `chunker_emoji_compound` | functional.rs | chunker | Emoji compuesto |
| `chunker_overlap_greater_than_max_size` | functional.rs | chunker | overlap > max_size |
| `retrieval_remove_by_empty_prefix_removes_all` | functional.rs | retrieval | prefix vacío elimina todo |
| `config_json_wrong_types_returns_error` | functional.rs | config | JSON con tipos incorrectos |
| `config_json_corrupt_returns_error` | functional.rs | config | JSON corrupto |
| `session_estimate_tokens_does_not_panic_on_large` | functional.rs | session | String de 1MB no panic |
| `utils_file_lock_identifies_as_aten_ia` | functional.rs | utils | Lock file identifica como aten-ia |
| `utils_file_lock_stale_detection` | functional.rs | utils | Lock stale con PID muerto |
| `shutdown_flag_default_is_not_requested` | functional.rs | shutdown | Flag por defecto es false |
| `shutdown_request_sets_flag` | functional.rs | shutdown | request_shutdown() setea flag |

## 4. Fallos Conocidos No Resueltos

### 4.1 Error de Linking Preexistente

Los tests de integración (`cargo test`) fallan al linkear con un error de dependencia circular entre `ggml-cpu` y `ggml`:

```
undefined reference to `ggml_backend_cpu_reg'
```

**Solución requerida**: Agregar `--start-group`/`--end-group` al linker o cambiar el orden de las librerías en `build.rs`.

**Workaround actual**: Ejecutar `cargo test --lib -- --test-threads=1` (solo tests unitarios).

### 4.2 Clang Requerido para Build

`bindgen` (generador de bindings FFI) requiere `clang` y `stdbool.h`. En el sistema actual sin clang instalado, se requiere:

```bash
BINDGEN_EXTRA_CLANG_ARGS="-I/usr/lib/gcc/x86_64-linux-gnu/15/include" cargo build
```

## 5. Comandos de Verificación

| Comando | Descripción |
|--------|-------------|
| `cargo build` | Compilar el binario |
| `cargo clippy --lib` | Lint de la librería |
| `cargo fmt --all -- --check` | Verificar formato |
| `cargo test --lib -- --test-threads=1` | Tests unitarios (sin GGUF) |
| `./scripts/validate-compat.sh` | Validar compatibilidad glibc |

## 6. Plan de Mejora Continua

### Fase 1: Corregir Linking (Prioridad Alta)
- [ ] Agregar `--start-group`/`--end-group` en `build.rs` para resolver dependencia circular
- [ ] Instalar `clang` en CI para que bindgen funcione

### Fase 2: Tests de Integración API (Prioridad Media)
- [ ] Agregar test de API con mock de TcpListener
- [ ] Agregar test de request malformado con caracteres nulos
- [ ] Agregar test de body exactamente en el límite (10MB + 1 byte)

### Fase 3: Tests de Estrés (Prioridad Baja)
- [ ] Test de búsqueda en índice con 100K+ entradas
- [ ] Test de chunking con texto de 10MB
- [ ] Test de concurrencia en API server