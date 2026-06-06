# aten-ia

Interactive AI agent CLI con inferencia LLM local via [llama.cpp](https://github.com/ggml-org/llama.cpp) (fork TurboQuant), persistencia `.mv2` con [memvid-core](https://crates.io/crates/memvid-core), RAG por keywords, API OpenAI-compatible e ingesta multi-formato.

## Quick start

### Requisitos

- **Rust** >= 1.85.0 (edition 2024, usar `rust-toolchain.toml`)
- **System deps**: `cmake libssl-dev clang libgomp-dev` (build) | `fakeroot` (packaging)

### Build & run

La primera compilacion tarda ~30 min (compila llama.cpp desde fuente). Las siguientes son <1s si hay cache.

```bash
cd memvid-agent-core
cargo build
cargo run
```

En el primer lanzamiento aparece un wizard interactivo para elegir modelo, directorio de datos y configuracion API. Si no existe un modelo GGUF, se descarga automaticamente `Qwen2.5-0.5B-Instruct` (~350 MB) con barra de progreso.

Para compilar en CI sin modelo (solo tests, sin GGUF):
```bash
cargo test --lib -- --test-threads=1
```

### Build portable (compatible con Ubuntu 20.04+ / Debian 12+)

Para construir un binario portable compatible con glibc 2.31+ (Ubuntu 20.04, Debian 12):

```bash
./scripts/build-portable.sh
./scripts/validate-compat.sh dist/aten-ia
```

Requiere Docker. El binario resultante:
- Compatible con Ubuntu 20.04+ (glibc 2.31+)
- Compatible con Debian 12+ (glibc 2.36+)
- `libstdc++` y `libgomp` estaticamente linkeados
- Solo depende de `libc.so.6`, `libm.so.6`, `libgcc_s.so.1`, `libpthread.so.0`, `libdl.so.2`

### Release

```bash
git tag v0.1.0 && git push --tags
```

Esto dispara GitHub Actions: compila binarios + `.deb` para x86_64 y ARM64, usando contenedor Ubuntu 20.04 para compatibilidad con glibc antiguo.

### Variables de entorno

| Variable | Default | Descripcion |
|---|---|---|
| `MODEL_PATH` | `models/qwen2.5-0.5b.gguf` | Ruta al modelo GGUF |
| `MODEL_NAME` | `Qwen2.5-0.5B-Instruct` | Nombre del modelo |
| `MODEL_CTX` | `8192` | Tamano de contexto |
| `MODEL_URL` | URL de HuggingFace | URL de descarga alternativa |
| `LLAMA_LOCAL_LIBS` | — | Directorio con `.a` precompiladas (salta cmake) |
| `LLAMA_LIBS_REPO` | `lordpietre/aten-ia` | Repo de GitHub para descargar libs precompiladas |
| `ATEN_PORTABLE` | — | Si es `1`, linkea `libstdc++` y `libgomp` estaticamente |
| `CMAKE_BUILD_PARALLEL_LEVEL` | `2` | Nivel de paralelismo para cmake |
| `TARGET` | Auto | Target triple para cross-compilation |

Opcionalmente se carga `.env` via `dotenvy` antes de la configuracion.

## Comandos del REPL

| Comando | Descripcion |
|---|---|
| `<mensaje>` | Chatea con el agente |
| `/models` | Lista modelos disponibles |
| `/model [id\|current]` | Muestra / descarga / cambia modelo |
| `/learn <lang>` | Descarga e indexa docs de un lenguaje |
| `/unlearn <lang>` | Elimina conocimiento de un lenguaje |
| `/fetch <url>` | Descarga URL, extrae texto e indexa |
| `/fetch-md <url>` | Descarga URL y muestra como Markdown |
| `/ingest <file>` | Indexa archivo local (PDF/EPUB/MD/HTML/txt) |
| `/ingest-pdf <file>` | Extrae e indexa PDF/EPUB explicitamente |
| `/search <query> [from:<source>]` | Busca en el knowledge index |
| `/reindex` | Reconstruye indice desde `.mv2` |
| `/load <file>` | Carga archivo en sesion (sin indexar) |
| `/batch <file>` | Procesa lote de URLs desde archivo |
| `/feed <url>` | Descarga e indexa feed RSS/Atom |
| `/queue-add <url>` | Anade URL a la cola de feeds |
| `/queue-process` | Procesa todas las URLs pendientes |
| `/queue` | Muestra estado de la cola |
| `/books` | Lista lenguajes de free-programming-books |
| `/download-books <lang> [limit]` | Descarga e indexa libros de un lenguaje |
| `/languages` | Lista lenguajes disponibles para `/learn` |
| `/languages-installed` | Lista lenguajes instalados |
| `/token` | Inicia servidor API OpenAI-compatible |
| `/kv [k] [v]` | Muestra/cambia tipos de KV-cache |
| `/stats` | Estadisticas del agente |
| `/history` | Historial de conversaciones |
| `/config` | Muestra configuracion actual |
| `/help` | Ayuda completa |
| `/exit` | Salir (Ctrl+C tambien funciona con shutdown graceful) |

## Shutdown Graceful

aten-ia maneja senales SIGINT (Ctrl+C) y SIGTERM:

- Al presionar Ctrl+C, el proceso detecta la senal y sale limpiamente
- El API server se detiene al recibir la senal de shutdown
- La sesion se flusha automaticamente antes de salir (via `Agent::Drop`)
- El file lock se elimina automaticamente al salir
- El mensaje de despedida indica si fue por senal o salida normal

## Modelo por defecto

El modelo por defecto es `Qwen2.5-0.5B-Instruct` (`n_ctx: 8192`, `chat_template: chatml`). El catalog incluye 20 modelos GGUF Q4_K_M — se seleccionan con `/model <id>`.

## Arquitectura

- **CPU only** — `n_gpu_layers = 0`, sin flags CUDA/Metal/Vulkan
- **RAG keyword-only** — busqueda por substrings sobre `knowledge_index.jsonl`, sin embeddings
- **API single-threaded** — `TcpListener`, conexiones secuenciales, sin streaming SSE
- **Persistencia atomica** — write -> fsync -> rename -> fsync(parent)
- **Chunker UTF-8 safe** — `floor_char_boundary()` en overlaps, no puede entrar en panico con caracteres multibyte
- **Session flush cada 5 interacciones** — `Session::flush()` -> `MemvidWriter`
- **FileLock** — `data_dir/.lock` con `aten-ia <PID>`, rechaza instancias concurrentes, detecta locks stale de procesos muertos
- **Shutdown graceful** — SIGINT/SIGTERM detectado via `libc::signal`, API server se detiene limpiamente, sesion se flusha al salir
- **llama.cpp verbose suprimido** — `llama_log_set(noop_log)` al inicio

### Estructura de modulos

```
src/
  lib.rs          — 22 modulos publicos
  main.rs         — REPL interactivo + setup wizard + shutdown handler
  agent.rs        — Agente principal (chat, ingest, RAG)
  api.rs           — Servidor HTTP OpenAI-compatible (detenible via AtomicBool)
  books_catalog.rs — Catalogo de libros gratuitos
  chunker.rs       — Chunking de texto (Fixed/Paragraph/Heading)
  config.rs        — Configuracion persistente (config.json)
  context_policy.rs — Politica de tamanho de contexto
  extractor.rs     — Extraccion de PDF/EPUB/HTML/MD
  feeds.rs         — Parseo de feeds RSS/Atom
  generation.rs    — Generacion de texto via LLM
  languages_catalog.rs — Catalogo de lenguajes
  llama/            — FFI bindings para llama.cpp (TurboQuant)
  memvid/           — Persistencia .mv2 (writer/reader/manifest/playlist)
  models.rs         — Descarga de modelos GGUF
  models_catalog.rs — Catalogo de 20 modelos
  prompt.rs         — Templates de chat (ChatML/Llama/Alpaca/Mistral/Raw)
  queue.rs          — Cola de feeds (JSONL persistente)
  retrieval.rs      — Indice de conocimiento (keyword RAG)
  session.rs        — Sesion de chat (flush cada 5 interacciones)
  shutdown.rs        — Manejo de senales SIGINT/SIGTERM
  types.rs           — Tipos compartidos (KnowledgeEntry, ConversationBatch, etc.)
  utils.rs           — FileLock, atomic_write, sha256, truncacion UTF-8
  web_fetcher.rs     — Fetch HTTP con rate limiting y reintentos
```

### Estructura de memoria

```
$DATA_DIR/
  core.mv2
  manifest.json
  feed_queue.jsonl
  knowledge_index.jsonl
  conversations/
  knowledge/
  archive/
```

## Configuracion

El proyecto usa `config.json` en `memvid-agent-core/`:

- `data_dir`: directorio de persistencia (default `memvid_data`)
- `model.path`: ruta al archivo GGUF
- `model.name`: nombre del modelo activo
- `model.n_ctx`: tamano de contexto
- `model.kv_type_k` / `model.kv_type_v`: tipo de KV-cache (default `f16`; `f32`, `bf16`, `q8_0`, `q4_0`, o TurboQuant `turbo2/3/4` — los codecs TurboQuant activan flash-attn automaticamente)
- `generation.top_k`, `top_p`, `temp`, `max_tokens`
- `api.enabled`, `api.host`, `api.port`, `api.token`
- `languages.installed`
- `ingestion.timeout_seconds`, `chunk_max_size`, `chunk_overlap`, etc.

Validacion en `config.validate()`: `n_ctx > 0`, `max_tokens > 0`, `temp >= 0`, `0 <= top_p <= 1`, `top_k >= 0`, `port > 0`, y KV cache types validos.

## Test

```bash
cd memvid-agent-core
cargo test --lib -- --test-threads=1   # unit tests (no GGUF needed)
cargo test -- --test-threads=1         # all tests (requires linking fix)
cargo fmt --all -- --check
cargo clippy --lib                     # lib only, not main.rs
```

Tests de integracion (`tests/functional.rs`, `tests/writer_integration.rs`, `tests/kv_cache_and_history.rs`) usan `tempfile::tempdir()` + `LlamaContext::null()` — no necesitan archivo GGUF.

Ver `PLAN_TESTS.md` para el plan detallado de tests y fallos potenciales identificados.

## Portabilidad

El binario portable se construye en un contenedor Ubuntu 20.04 (glibc 2.31):

- **glibc maximo requerido**: GLIBC_2.29
- **Compatible con**: Ubuntu 20.04+, Debian 12+, y cualquier distribucion con glibc >= 2.29
- **Dependencias dinamicas**: solo `libc.so.6`, `libm.so.6`, `libgcc_s.so.1`, `libpthread.so.0`, `libdl.so.2`
- **`libstdc++` y `libgomp`**: estaticamente linkeados (no dependen de versiones del sistema)

Archivos relevantes:
- `docker/Dockerfile.build` — contenedor Ubuntu 20.04 para build portable
- `scripts/build-portable.sh` — script de build automatizado
- `scripts/validate-compat.sh` — validacion de compatibilidad glibc
- `memvid-agent-core/.cargo/config.toml` — configuracion de static linking
- `.github/workflows/release.yml` — CI/CD con contenedor Ubuntu 20.04

## Dependencias principales

| Crate | Uso |
|---|---|
| `memvid-core` | Persistencia `.mv2` |
| `llama-cpp-turboquant` (bindgen) | Inferencia LLM via FFI |
| `ureq` | Descargas HTTP + timeout |
| `indicatif` | Barras de progreso y spinners |
| `pdf-extract` / `epub` | Extraccion de texto |
| `feed-rs` | Parseo de feeds RSS/Atom |
| `sha2` | Checksums SHA-256 |
| `colored` | Salida de terminal colorida |
| `dotenvy` | Carga de `.env` |
| `libc` | Signal handling para shutdown graceful |

## Limitaciones conocidas

- RAG es keyword-based (sin embeddings semanticos)
- API: un hilo por conexion, inferencia serializada por mutex, sin streaming SSE
- Template `Raw` ignora historial y RAG context
- No hay `MODEL_GPU_LAYERS` env var (CPU only por ahora)
- Token counts en la API son siempre 0 (no se tokeniza la request)
- `/batch` no incrementa la barra de progreso por URL individual

## Licencia

Apache 2.0