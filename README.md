# aten-ia

Agente de IA interactivo CLI con inferencia LLM local via [llama.cpp](https://github.com/ggml-org/llama.cpp) (fork TurboQuant), persistencia `.mv2` ([memvid-core](https://crates.io/crates/memvid-core)), RAG por keywords, API OpenAI-compatible e ingesta multi-formato.

## Inicio rapido

### Requisitos

- **Rust** >= 1.85.0 (edition 2024, via `rust-toolchain.toml`)
- **System deps (build)**: `cmake libssl-dev clang libgomp-dev`
- **System deps (packaging)**: + `fakeroot`

### Build y ejecucion

La primera compilacion tarda ~30 min (compila llama.cpp desde fuente). Posteriores <1s con cache.

```bash
cd memvid-agent-core
cargo build
cargo run
```

El primer lanzamiento muestra un wizard interactivo para elegir modelo, directorio de datos y config API. Si no existe un modelo GGUF, se descarga automaticamente `Qwen2.5-0.5B-Instruct` (~350 MB) con barra de progreso.

### Tests

```bash
cd memvid-agent-core
cargo test -- --test-threads=1    # todos los tests
cargo test --lib -- --test-threads=1  # solo unit (mas rapido)
cargo fmt --all -- --check
cargo clippy --lib
```

Los tests de integracion usan `tempfile::tempdir()` + `LlamaContext::null()` — no necesitan archivo GGUF.

Ver [`PLAN_TESTS.md`](PLAN_TESTS.md) para el plan detallado de tests y fallos potenciales identificados.

## Build portable

Construye un binario portable en un contenedor Ubuntu 20.04 (glibc 2.31), compatible con las siguientes distribuciones:

| Distribucion | Version | glibc | Compatibilidad |
|---|---|---|---|
| Ubuntu | 20.04 LTS | 2.31 | Compatible |
| Ubuntu | 22.04 LTS | 2.35 | Compatible |
| Ubuntu | 24.04 LTS | 2.39 | Compatible |
| Ubuntu | 26.04 LTS | 2.41 | Compatible |
| Debian | 12 (bookworm) | 2.36 | Compatible |
| Debian | 13 (trixie) | 2.38 | Compatible |

```bash
./scripts/build-portable.sh
./scripts/validate-compat.sh dist/aten-ia
```

Requiere Docker. El binario resultante:

- glibc maximo requerido: GLIBC_2.31
- `libstdc++` y `libgomp` estaticamente linkeados
- Solo depende de: `libc.so.6`, `libm.so.6`, `libgcc_s.so.1`, `libpthread.so.0`, `libdl.so.2`

Archivos relevantes:

| Archivo | Descripcion |
|---|---|
| `docker/Dockerfile.build` | Contenedor Ubuntu 20.04 para build portable |
| `scripts/build-portable.sh` | Script de build automatizado con Docker |
| `scripts/validate-compat.sh` | Validacion de compatibilidad glibc |
| `memvid-agent-core/.cargo/config.toml` | Flags de static linking |
| `.github/workflows/release.yml` | CI/CD con contenedor Ubuntu 20.04 |

### Release

```bash
git tag v0.1.0 && git push --tags
```

Esto dispara GitHub Actions: compila binarios + `.deb` para x86_64 y ARM64, usando contenedor Ubuntu 20.04 para compatibilidad con glibc antiguo.

## Variables de entorno

| Variable | Default | Descripcion |
|---|---|---|
| `MODEL_PATH` | `models/qwen2.5-0.5b.gguf` | Ruta al modelo GGUF |
| `MODEL_NAME` | `Qwen2.5-0.5B-Instruct` | Nombre del modelo |
| `MODEL_CTX` | `8192` | Tamano de contexto |
| `MODEL_URL` | URL de HuggingFace | URL de descarga alternativa |
| `LLAMA_LOCAL_LIBS` | — | Directorio con `.a` precompiladas (salta cmake) |
| `LLAMA_LIBS_REPO` | `lordpietre/aten-ia` | Repo de GitHub para descargar libs precompiladas |
| `LLAMA_FINETUNE_BIN` | — | Ruta a un binario `llama-finetune` precompilado (para `/finetune`) |
| `ATEN_PORTABLE` | — | Si es `1`, linkea `libstdc++` y `libgomp` estaticamente |
| `CMAKE_BUILD_PARALLEL_LEVEL` | `2` | Nivel de paralelismo para cmake |
| `TARGET` | Auto | Target triple para cross-compilation |

Opcionalmente se carga `.env` via `dotenvy` antes de la configuracion.

**Prebuilt libs fallback chain** (en `build.rs`):
1. `LLAMA_LOCAL_LIBS=/path` — copia `.a` desde un directorio local
2. Descarga `llama-libs-{target}.tar.gz` desde GitHub Releases
3. CMake build (nivel de paralelismo via `CMAKE_BUILD_PARALLEL_LEVEL`, default 2)

Override del repo de descarga: `LLAMA_LIBS_REPO=user/repo`.

## Comandos del REPL

| Comando | Descripcion |
|---|---|
| `<mensaje>` | Chatea con el agente |
| `/models` | Lista modelos disponibles |
| `/model [id\|current]` | Muestra / descarga / cambia modelo |
| `/learn <lang>` | Descarga e indexa docs de un lenguaje |
| `/unlearn <lang>` | Elimina conocimiento de un lenguaje |
| `/finetune <lang>` | Fine-tune real del modelo activo con los docs de `/learn` (estima RAM/tiempo antes) |
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

## Shutdown graceful

aten-ia maneja senales SIGINT (Ctrl+C) y SIGTERM:

- Al presionar Ctrl+C, el proceso detecta la senal y sale limpiamente
- El API server se detiene al recibir la senal de shutdown
- La sesion se flusha automaticamente antes de salir (via `Agent::Drop`)
- El file lock se elimina automaticamente al salir
- El mensaje de despedida indica si fue por senal o salida normal

## Modelo por defecto

`Qwen2.5-0.5B-Instruct` (`n_ctx: 8192`, `chat_template: chatml`). El catalog incluye 20 modelos GGUF Q4_K_M — se seleccionan con `/model <id>`.

## Arquitectura

- **CPU only** — `n_gpu_layers = 0`, sin flags CUDA/Metal/Vulkan
- **RAG keyword-only** — busqueda por substrings sobre `knowledge_index.jsonl`, sin embeddings
- **API single-threaded** — `TcpListener`, conexiones secuenciales, sin streaming SSE
- **Persistencia atomica** — write -> fsync -> rename -> fsync(parent)
- **Chunker UTF-8 safe** — `floor_char_boundary()` en overlaps, no puede entrar en panico con caracteres multibyte
- **Session flush cada 5 interacciones** — `Session::flush()` -> `MemvidWriter`
- **FileLock** — `data_dir/.lock` con `aten-ia <PID>`, rechaza instancias concurrentes, detecta locks stale de procesos muertos
- **Shutdown graceful** — SIGINT/SIGTERM via `libc::signal` + `AtomicBool`, API server se detiene limpiamente, sesion se flusha al salir
- **llama.cpp verbose suprimido** — `llama_log_set(noop_log)` al inicio

### Estructura de modulos

```
src/
  lib.rs              — 22 modulos publicos
  main.rs             — REPL interactivo + setup wizard + shutdown handler
  agent.rs            — Agente principal (chat, ingest, RAG)
  api.rs              — Servidor HTTP OpenAI-compatible (detenible via AtomicBool)
  books_catalog.rs    — Catalogo de libros gratuitos
  chunker.rs          — Chunking de texto (Fixed/Paragraph/Heading)
  config.rs           — Configuracion persistente (config.json)
  context_policy.rs   — Politica de tamano de contexto
  extractor.rs        — Extraccion de PDF/EPUB/HTML/MD
  feeds.rs            — Parseo de feeds RSS/Atom
  generation.rs       — Generacion de texto via LLM
  languages_catalog.rs — Catalogo de lenguajes
  llama/              — FFI bindings para llama.cpp (TurboQuant)
  memvid/             — Persistencia .mv2 (writer/reader/manifest/playlist)
  models.rs           — Descarga de modelos GGUF
  models_catalog.rs   — Catalogo de 20 modelos
  prompt.rs           — Templates de chat (ChatML/Llama/Alpaca/Mistral/Raw)
  queue.rs            — Cola de feeds (JSONL persistente)
  retrieval.rs        — Indice de conocimiento (keyword RAG)
  session.rs          — Sesion de chat (flush cada 5 interacciones)
  shutdown.rs         — Manejo de senales SIGINT/SIGTERM
  types.rs            — Tipos compartidos (KnowledgeEntry, ConversationBatch, etc.)
  utils.rs           — FileLock, atomic_write, sha256, truncacion UTF-8
  web_fetcher.rs      — Fetch HTTP con rate limiting y reintentos
```

### Estructura de datos

```
~/.aten-ia/
  core.mv2
  manifest.json
  feed_queue.jsonl
  knowledge_index.jsonl
  conversations/
  knowledge/
  archive/
```

Por defecto los datos se guardan en `~/.aten-ia`. Puedes cambiarlo con `/config` o la variable `DATA_DIR`.

## Configuracion

`config.json` en `memvid-agent-core/`:

- `data_dir`: directorio de persistencia (default `~/.aten-ia`, cuando `$HOME` no esta disponible usa `memvid_data`)
- `model.path`: ruta al archivo GGUF
- `model.name`: nombre del modelo activo
- `model.n_ctx`: tamano de contexto
- `model.kv_type_k` / `model.kv_type_v`: tipo de KV-cache (default `f16`; `f32`, `bf16`, `q8_0`, `q4_0`, o TurboQuant `turbo2/3/4` — los codecs TurboQuant activan flash-attn automaticamente)
- `generation.top_k`, `top_p`, `temp`, `max_tokens`
- `api.enabled`, `api.host`, `api.port`, `api.token`
- `languages.installed`
- `ingestion.timeout_seconds`, `chunk_max_size`, `chunk_overlap`, etc.

Validacion en `config.validate()`: `n_ctx > 0`, `max_tokens > 0`, `temp >= 0`, `0 <= top_p <= 1`, `top_k >= 0`, `port > 0`, y KV cache types validos.

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

## Fine-tuning (`/finetune <lang>`)

Tras `/learn <lang>` puedes afinar el modelo activo con esos mismos docs. El flujo:

1. **Corpus**: concatena los chunks indexados cuyo `source` empieza por `"<lang>/"`.
2. **Estimacion previa**: calcula RAM pico (~16 bytes/parametro, full fine-tune
   AdamW) y tiempo aproximado segun nº de parametros, epocas y threads. Si no cabe
   en la RAM disponible (`/proc/meminfo`), avisa antes de continuar.
3. **Ejecucion**: si hay binario `llama-finetune` (via `LLAMA_FINETUNE_BIN`,
   `config.finetune.binary_path` o `PATH`) lo lanza y produce un GGUF afinado, y
   ofrece cambiar a el. Si **no** hay binario, escribe el corpus y un script
   `finetune_<lang>.sh` listo para ejecutar en una maquina con mas RAM.

> ⚠️ Es **full fine-tune** (todos los pesos, salida GGUF del tamaño del modelo),
> no LoRA. En CPU es lento y consume mucha RAM: pensado para portar a una maquina
> capaz. aten-ia no compila `llama-finetune`; constrúyelo desde el fork:
> `cmake -B build -DLLAMA_BUILD_EXAMPLES=ON && cmake --build build --target llama-finetune -j`.

Config en `config.json` → `finetune`: `binary_path`, `epochs` (def. 3),
`output_dir` (def. `models/finetuned`). Se dispara tambien automaticamente (oferta
y/N) al terminar un `/learn`.

## Limitaciones conocidas

- RAG es keyword-based (sin embeddings semanticos)
- API: un hilo por conexion, inferencia serializada por mutex, sin streaming SSE
- Template `Raw` ignora historial y RAG context
- No hay `MODEL_GPU_LAYERS` env var (CPU only por ahora)
- Token counts en la API son siempre 0 (no se tokeniza la request)
- `/batch` no incrementa la barra de progreso por URL individual

## Licencia

Apache 2.0