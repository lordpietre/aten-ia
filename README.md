# aten-ia

Interactive AI agent CLI con inferencia LLM local vía [llama.cpp](https://github.com/ggml-org/llama.cpp) (fork TurboQuant), persistencia `.mv2` con [memvid-core](https://crates.io/crates/memvid-core), RAG por keywords, API OpenAI-compatible e ingesta multi-formato.

## Quick start

### Requisitos

- **Rust** ≥ 1.95.0 (edition 2024, sin `rust-toolchain.toml`)
- **System deps**: `build-essential cmake libssl-dev clang libgomp1`

### Build & run

La primera compilación tarda ~30 min (compila llama.cpp desde fuente). Las siguientes son <1 s si hay caché.

```bash
cd memvid-agent-core
cargo build
cargo run
```

En el primer lanzamiento aparece un wizard interactivo para elegir modelo, directorio de datos y configuración API. Si no existe un modelo GGUF, se descarga automáticamente `Qwen2.5-0.5B-Instruct` (~350 MB) con barra de progreso.

Para compilar en CI sin modelo (solo tests, sin GGUF):
```bash
cargo test -- --test-threads=1
```

### Release

```bash
git tag v0.1.0 && git push --tags
```

Esto dispara GitHub Actions: compila binarios + `.deb` + `.snap` para x86_64 y ARM64, y los sube a un GitHub Release. Se requieren `fakeroot` para empaquetar.

### Variables de entorno

| Variable | Default | Descripción |
|---|---|---|
| `MODEL_PATH` | `models/qwen2.5-0.5b.gguf` | Ruta al modelo GGUF |
| `MODEL_NAME` | `Qwen2.5-0.5B-Instruct` | Nombre del modelo |
| `MODEL_CTX` | `8192` | Tamaño de contexto |
| `MODEL_URL` | URL de HuggingFace | URL de descarga alternativa |
| `LLAMA_LOCAL_LIBS` | — | Directorio con `.a` precompiladas (salta cmake) |
| `LLAMA_LIBS_REPO` | `lordpietre/aten-ia` | Repo de GitHub para descargar libs precompiladas |

Opcionalmente se carga `.env` vía `dotenvy` antes de la configuración.

## Comandos del REPL

| Comando | Descripción |
|---|---|
| `<mensaje>` | Chatea con el agente |
| `/models` | Lista modelos disponibles |
| `/model [id\|current]` | Muestra / descarga / cambia modelo |
| `/learn <lang>` | Descarga e indexa docs de un lenguaje |
| `/unlearn <lang>` | Elimina conocimiento de un lenguaje |
| `/fetch <url>` | Descarga URL, extrae texto e indexa |
| `/fetch-md <url>` | Descarga URL y muestra como Markdown |
| `/ingest <file>` | Indexa archivo local (PDF/EPUB/MD/HTML/txt) |
| `/ingest-pdf <file>` | Extrae e indexa PDF/EPUB explícitamente |
| `/search <query> [from:<source>]` | Busca en el knowledge index |
| `/reindex` | Reconstruye índice desde `.mv2` |
| `/load <file>` | Carga archivo en sesión (sin indexar) |
| `/batch <file>` | Procesa lote de URLs desde archivo |
| `/feed <url>` | Descarga e indexa feed RSS/Atom |
| `/queue-add <url>` | Añade URL a la cola de feeds |
| `/queue-process` | Procesa todas las URLs pendientes |
| `/queue` | Muestra estado de la cola |
| `/books` | Lista lenguajes de free-programming-books |
| `/download-books <lang> [limit]` | Descarga e indexa libros de un lenguaje |
| `/languages` | Lista lenguajes disponibles para `/learn` |
| `/languages-installed` | Lista lenguajes instalados |
| `/token` | Inicia servidor API OpenAI-compatible |
| `/kv [k] [v]` | Muestra/cambia tipos de KV-cache |
| `/stats` | Estadísticas del agente |
| `/history` | Historial de conversaciones |
| `/config` | Muestra configuración actual |
| `/help` | Ayuda completa |
| `/exit` | Salir |

## Modelo por defecto

El modelo por defecto es `Qwen2.5-0.5B-Instruct` (`n_ctx: 8192`, `chat_template: chatml`). El catálogo incluye 20 modelos GGUF Q4_K_M — se seleccionan con `/model <id>`.

## Arquitectura

- **CPU only** — `n_gpu_layers = 0`, sin flags CUDA/Metal/Vulkan
- **RAG keyword-only** — búsqueda por substrings sobre `knowledge_index.jsonl`, sin embeddings
- **API single-threaded** — `TcpListener`, conexiones secuenciales, sin streaming SSE
- **Persistencia atómica** — write → fsync → rename → fsync(parent)
- **Chunker UTF-8 safe** — `floor_char_boundary()` en overlaps, no puede entrar en pánico con caracteres multibyte
- **Session flush cada 5 interacciones** — `Session::flush()` → `MemvidWriter`
- **FileLock** — `data_dir/.lock` con PID, rechaza instancias concurrentes
- **llama.cpp verbose suprimido** — `llama_log_set(noop_log)` al inicio

### Estructura de memoria

```
$DATA_DIR/
├── core.mv2
├── manifest.json
├── feed_queue.jsonl
├── knowledge_index.jsonl
├── conversations/
├── knowledge/
└── archive/
```

## Configuración

El proyecto usa `config.json` en `memvid-agent-core/`:

- `data_dir`: directorio de persistencia (default `memvid_data`)
- `model.path`: ruta al archivo GGUF
- `model.name`: nombre del modelo activo
- `model.n_ctx`: tamaño de contexto
- `model.kv_type_k` / `model.kv_type_v`: tipo de KV-cache (default `f16`; `f32`, `bf16`, `q8_0`, `q4_0`, o TurboQuant `turbo2/3/4` — los codecs TurboQuant activan flash-attn automáticamente)
- `generation.top_k`, `top_p`, `temp`, `max_tokens`
- `api.enabled`, `api.host`, `api.port`, `api.token`
- `languages.installed`
- `ingestion.timeout_seconds`, `chunk_max_size`, `chunk_overlap`, etc.

Validación en `config.validate()`: `n_ctx > 0`, `max_tokens > 0`, `temp >= 0`, `0 <= top_p <= 1`, `top_k >= 0`, `port > 0`, y KV cache types válidos.

## Test

```bash
cd memvid-agent-core
cargo test -- --test-threads=1   # obligatorio: race en env-var MODEL_PATH
cargo fmt --all -- --check
cargo clippy --lib               # solo lib, no main.rs
```

Tests de integración (`tests/functional.rs`, `tests/writer_integration.rs`, `tests/kv_cache_and_history.rs`) usan `tempfile::tempdir()` + `LlamaContext::null()` — no necesitan archivo GGUF.

## Dependencias principales

| Crate | Uso |
|---|---|
| `memvid-core` | Persistencia `.mv2` |
| `llama-cpp-turboquant` (bindgen) | Inferencia LLM vía FFI |
| `ureq` | Descargas HTTP + timeout |
| `indicatif` | Barras de progreso y spinners |
| `pdf-extract` / `epub` | Extracción de texto |
| `feed-rs` | Parseo de feeds RSS/Atom |
| `sha2` | Checksums SHA-256 |
| `colored` | Salida de terminal colorida |
| `dotenvy` | Carga de `.env` |

## Limitaciones conocidas

- RAG es keyword-based (sin embeddings semánticos)
- API: un hilo por conexión, inferencia serializada por mutex, sin streaming SSE
- Template `Raw` ignora historial y RAG context
- No hay `MODEL_GPU_LAYERS` env var (CPU only por ahora)
- Token counts en la API son siempre 0 (no se tokeniza la request)
- `/batch` no incrementa la barra de progreso por URL individual

## Licencia

Apache 2.0