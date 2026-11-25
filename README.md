# aten-ia

**Agente de IA local con memoria persistente, RAG y API compatible con OpenAI.**

aten-ia es un agente conversacional de línea de comandos escrito en Rust. Ejecuta modelos GGUF en CPU mediante [llama.cpp](https://github.com/ggml-org/llama.cpp) (fork **TurboQuant**), guarda conversaciones y conocimiento en segmentos `.mv2` ([memvid-core](https://crates.io/crates/memvid-core) v2), busca en su propio índice por palabras clave y expone una API HTTP compatible con OpenAI para usarlo desde OpenCode, VS Code u otros clientes.

Inferencia 100% local: no requiere GPU, API keys ni servicios externos. Solo necesita red para descargar el modelo la primera vez o para ingerir contenido de Internet.

- **Versión:** 0.3.12
- **Lenguaje:** Rust 2024
- **Licencia:** Apache 2.0 © 2026 Dinámicas Electrónicas

---

## Características

| Área | Qué incluye |
|---|---|
| **LLM local** | llama.cpp compilado desde el fork TurboQuant, CPU only, plantillas ChatML / Llama 3 / Mistral / Raw |
| **KV-cache configurable** | `f32`, `f16`, `bf16`, `q8_0`, `q4_0` y codecs TurboQuant `turbo2/3/4` (activan flash-attn automáticamente) |
| **Catálogo de modelos** | 21 modelos GGUF Q4_K_M (0.36B–14B) descargables desde el REPL |
| **Memoria persistente** | Conversaciones y conocimiento en `.mv2` + `manifest.json` + índice JSONL, con escrituras atómicas |
| **RAG keyword** | Búsqueda ponderada por coincidencias en fuente, contenido e ID, con presupuesto de tokens dinámico |
| **Ingesta multi-fuente** | Archivos PDF/EPUB/Markdown/HTML/TXT, URLs, lotes de URLs, feeds RSS/Atom y catálogo de free-programming-books |
| **Aprendizaje de lenguajes** | `/learn <lang>` descarga e indexa documentación de programación; `/unlearn` la elimina |
| **Fine-tuning** | Full fine-tune del modelo activo sobre los docs aprendidos, con estimación previa de RAM y tiempo |
| **API OpenAI-compatible** | `/v1/chat/completions` (SSE y parámetros por petición), `/v1/models`, `/health`, autenticación Bearer |
| **Integración con clientes** | `/token` genera configuración para OpenCode y muestra la de VS Code (Continue) |
| **Gestión de estado** | FileLock contra instancias concurrentes, flush de sesión cada 5 interacciones y apagado graceful (SIGINT/SIGTERM) |

---

## Stack tecnológico

| Componente | Tecnología |
|---|---|
| Lenguaje | Rust (edition 2024, mínimo 1.89) |
| Inferencia | llama.cpp (fork TurboQuant) vía FFI generado con `bindgen` + CMake |
| Persistencia | `memvid-core` v2 (`.mv2`) + JSONL |
| HTTP cliente | `ureq` (rustls), con timeouts, rate limiting y reintentos |
| HTTP servidor | `TcpListener` propio, un hilo por conexión, mutex de inferencia |
| Extracción | `pdf-extract`, `epub`, HTML→texto/Markdown propio |
| Feeds | `feed-rs` (RSS/Atom) |
| Serialización | `serde` / `serde_json` |
| Utilidades | `sha2`, `uuid`, `chrono`, `regex`, `indicatif`, `colored`, `dotenvy`, `libc` |
| Tests | `cargo test` + `tempfile` (sin GGUF) |

---

## Requisitos

- **Rust** >= 1.89.0 (edition 2024, fijado en `rust-toolchain.toml`)
- **Dependencias de build:** `cmake libssl-dev clang libclang-dev libgomp-dev`
- **Dependencias de empaquetado:** añadir `fakeroot`
- Opcional: Docker (solo para el build portable reproducible)

---

## Inicio rápido

```bash
cd memvid-agent-core
cargo build
cargo run
```

La **primera compilación tarda ~30 min** porque compila llama.cpp desde fuente; las siguientes son incrementales (<1 s si no hay cambios). Alternativas para evitar la compilación:

1. `LLAMA_LOCAL_LIBS=/ruta/con/libs` — copia los `.a` desde un directorio local.
2. Descarga automática de `llama-libs-{target}.tar.gz` desde GitHub Releases.
3. Compilación con CMake (paralelismo con `CMAKE_BUILD_PARALLEL_LEVEL`, por defecto 2).

Comprobar la versión:

```bash
cargo run -- --version      # aten-ia 0.3.12
cargo run -- --self-test    # verifica sintaxis y uso sin modelo ni red
```

### Primer arranque

Si no existe `config.json`, se lanza un asistente interactivo que configura:

1. Directorio de datos (por defecto `~/.aten-ia`, o `memvid_data` si `$HOME` no existe).
2. Modelo inicial del catálogo (21 opciones).
3. Documentación de un lenguaje (opcional).
4. Servidor API: puerto, token generado y configuración de OpenCode (global y de proyecto).

Al terminar, el modelo se descarga automáticamente con barra de progreso si aún no está en disco (por defecto `Qwen2.5-0.5B-Instruct`, ~350 MB).

El repositorio incluye `memvid-agent-core/config.example.json` como plantilla (usa `memvid_data` como directorio de datos). Copia ese archivo a `config.json` o bórralo para repetir el asistente. `config.json` está en `.gitignore` porque puede contener el token de la API.

Opcionalmente se carga `.env` (vía `dotenvy`) antes de leer la configuración.

---

## Comandos del REPL

Los comandos aceptan mayúsculas en muchos casos (`/MODELS`, `/MODEL`, `/SEARCH`, `/TOKEN`, `/LOAD`, `/INGEST`, etc.).

### Conversación y estado

| Comando | Descripción |
|---|---|
| `<mensaje>` | Chatea con el agente (usa RAG e historial de la sesión) |
| `/help` | Muestra la ayuda completa |
| `/exit`, `/quit` | Sale (también Ctrl+C con apagado graceful) |
| `/stats` | Modelo activo, interacciones, memoria y archivos cargados |
| `/history`, `/ls` | Historial de conversaciones persistidas |
| `/config` | Configuración actual |
| `/debug` | Activa/desactiva el modo debug y muestra el RAG de la última consulta |

### Modelos y generación

| Comando | Descripción |
|---|---|
| `/models` | Lista el catálogo con tamaños, descripción y modelo activo |
| `/model` | Muestra el modelo actual |
| `/model current` | Igual que `/model` |
| `/model <id>` | Descarga (si falta) y cambia al modelo indicado |
| `/kv` | Muestra los tipos de KV-cache actuales |
| `/kv <k> <v>` | Cambia los tipos K/V (p. ej. `/kv f16 turbo3`) y recarga el contexto |

### Conocimiento e ingesta

| Comando | Descripción |
|---|---|
| `/search <query> [from:<source>]` | Busca en el índice (top 5), con filtro opcional por fuente |
| `/load <file>` | Carga un archivo de texto en la sesión (no lo indexa) |
| `/ingest <file>` | Extrae e indexa un archivo (PDF/EPUB/MD/HTML/TXT); los de texto se resumen al indexar |
| `/ingest-pdf <file>` | Igual, forzando extracción de PDF/EPUB |
| `/fetch <url>` | Descarga una URL, extrae texto y lo indexa (con deduplicación) |
| `/fetch-md <url>` | Descarga una URL y la muestra convertida a Markdown |
| `/batch <file>` | Procesa un archivo con una URL por línea (`#` para comentarios) |
| `/reindex` | Reconstruye el índice desde los segmentos `.mv2` |
| `/languages` | Lista lenguajes con documentación disponible |
| `/languages-installed` | Lista lenguajes instalados |
| `/learn <lang>` | Descarga hasta 10 recursos del lenguaje y los indexa |
| `/unlearn <lang>` | Elimina el conocimiento asociado al lenguaje |
| `/books` | Lista lenguajes de free-programming-books |
| `/download-books <lang> [limit]` | Indexa hasta `limit` recursos del catálogo de libros de ese lenguaje (metadatos: título, formato y URL) |
| `/finetune <lang>` | Fine-tune del modelo activo con los docs de ese lenguaje |

### Feeds y cola

| Comando | Descripción |
|---|---|
| `/feed <url>` | Descarga un feed RSS/Atom e indexa hasta 20 entradas |
| `/queue` | Estado de la cola (`pending`, `processing`, `done`, `failed`) |
| `/queue-add <url>` | Añade una URL a la cola persistente |
| `/queue-process` | Procesa todas las URLs pendientes |

### API

| Comando | Descripción |
|---|---|
| `/token` | Habilita la API, genera token si falta, arranca el servidor y ofrece configurar OpenCode/VS Code |

> Nota: `/token` genera un token si falta y escucha en `api.host` (**127.0.0.1** por defecto). Para acceder desde la red, cambia `api.host` a `0.0.0.0` y usa TLS/reverse proxy: el token viaja en claro por HTTP.

---

## API HTTP compatible con OpenAI

El servidor se inicia desde el REPL con `/token`. La sección `api` de `config.json` define host, puerto y token; marcar `api.enabled: true` no lo arranca automáticamente.

### Endpoints

| Método | Ruta | Auth | Descripción |
|---|---|---|---|
| `GET` | `/health` | No | Estado, modelo y uptime |
| `GET` | `/v1/models` | Bearer | Lista de modelos (el activo) |
| `POST` | `/v1/chat/completions` | Bearer | Chat completion; soporta `"stream": true` (SSE) |
| `GET` / `POST` | `/token` | Bearer | Devuelve el token configurado |

- **Autenticación:** `Authorization: Bearer <token>`.
- **Límites:** cuerpo máximo 10 MB, cabeceras 64 KB, timeout de conexión 120 s.
- **Concurrencia:** un hilo por conexión; la inferencia se serializa con un mutex, por lo que las peticiones se atienden de una en una.
- **SSE:** `stream: true` devuelve un evento `role`, un evento `content` y un evento `finish` con `[DONE]`. El texto se genera completo antes de emitirse, no token a token.
- **Memoria:** las peticiones de la API **no** se guardan en la sesión persistente del REPL.
- **Uso de tokens:** `usage.prompt_tokens` se cuenta con el tokenizador y `completion_tokens` se estima (chars/4).

### Ejemplo

```bash
curl http://localhost:8787/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{
    "model": "Qwen2.5-0.5B-Instruct",
    "messages": [{"role": "user", "content": "¿Qué es un struct en Rust?"}]
  }'
```

```bash
curl http://localhost:8787/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $TOKEN" \
  -d '{"messages":[{"role":"user","content":"hola"}],"stream":true}'
```

### Clientes

`/token` ofrece instrucciones y, en el caso de OpenCode, escribe la configuración:

- **OpenCode:** `~/.config/opencode/opencode.json` y, opcionalmente, `<padre-del-cwd>/opencode.json`. El proveedor se configura como `openai` con `baseURL: http://localhost:<puerto>/v1`.
- **VS Code (Continue):** muestra el bloque `openai-compatible` con `api_base` y `api_key`.

---

## Configuración

`config.json` (en el directorio desde el que se ejecuta el binario):

| Campo | Default | Descripción |
|---|---|---|
| `version` | `1` | Versión del formato de configuración |
| `data_dir` | `~/.aten-ia` (o `memvid_data`) | Directorio de persistencia |
| `developer_mode` | `true` | Activa el system prompt de desarrollo |
| `developer_prompt` | `null` | System prompt personalizado (si es `null`, usa el de por defecto) |
| `model.path` | `models/qwen2.5-0.5b.gguf` | Ruta al GGUF |
| `model.name` | `Qwen2.5-0.5B-Instruct` | Nombre visible del modelo |
| `model.n_ctx` | `8192` | Tamaño de contexto |
| `model.n_gpu_layers` | `0` | Capas en GPU (los builds no incluyen backends GPU) |
| `model.chat_template` | `chatml` | `chatml`, `llama3`, `mistral` o `raw` |
| `model.kv_type_k` / `model.kv_type_v` | `f16` | Tipo de KV-cache K/V (ver tabla siguiente) |
| `model.download_url` | HuggingFace | URL de descarga si falta el modelo |
| `model.sha256` | `null` | Checksum opcional del GGUF |
| `generation.top_k` | `40` | Muestreo top-k |
| `generation.top_p` | `0.95` | Muestreo nucleus |
| `generation.temp` | `0.8` | Temperatura |
| `generation.max_tokens` | `2048` | Tokens máximos por respuesta |
| `api.enabled` | `false` | Marca la API como habilitada (el arranque es vía `/token`) |
| `api.host` | `127.0.0.1` | Host de escucha |
| `api.port` | `8787` | Puerto |
| `api.token` | `null` | Token Bearer (se genera con `/token`) |
| `languages.installed` | `[]` | Lenguajes aprendidos |
| `ingestion.*` | ver abajo | Parámetros de descarga e indexación |
| `finetune.*` | ver abajo | Parámetros de fine-tuning |

**KV-cache admitida:** `f32`, `f16`, `bf16`, `q8_0`, `q4_0`, `turbo2`/`turbo2_0`, `turbo3`/`turbo3_0`, `turbo4`/`turbo4_0`.
Por fidelidad se recomienda mantener `K` con precisión igual o mayor que `V` (por ejemplo `K=f16`, `V=turbo3`); los codecs `turbo*` activan flash-attn automáticamente.

**Ingesta (defaults):**

| Campo | Default |
|---|---|
| `timeout_seconds` | `30` |
| `max_size_bytes` | `5242880` (5 MB) |
| `rate_limit_per_second` | `2` |
| `max_retries` | `3` |
| `retry_backoff_seconds` | `5` |
| `chunk_max_size` | `1024` |
| `chunk_overlap` | `200` |
| `user_agent` | `aten-ia/0.1.0` |

**Fine-tune (defaults):** `binary_path: null`, `epochs: 3`, `output_dir: models/finetuned`.

`Config::validate()` comprueba: `n_ctx > 0`, `max_tokens > 0`, `temp >= 0`, `0 <= top_p <= 1`, `top_k >= 0`, `port > 0` y tipos de KV válidos.

### Variables de entorno

| Variable | Default | Descripción |
|---|---|---|
| `MODEL_PATH` | — | Sobrescribe `model.path` |
| `MODEL_NAME` | — | Sobrescribe `model.name` |
| `MODEL_CTX` | — | Sobrescribe `model.n_ctx` |
| `MODEL_URL` | — | Sobrescribe `model.download_url` |
| `LLAMA_LOCAL_LIBS` | — | Directorio con `.a` precompiladas (evita CMake) |
| `LLAMA_LIBS_REPO` | `lordpietre/aten-ia` | Repo de GitHub para libs precompiladas |
| `LLAMA_FINETUNE_BIN` | — | Binario `llama-finetune` para `/finetune` |
| `ATEN_PORTABLE` | — | `1` enlaza estáticamente `libstdc++` y `libgomp` |
| `ATEN_THREADS` | Núcleos disponibles | Hilos de inferencia (llama.cpp) |
| `CMAKE_BUILD_PARALLEL_LEVEL` | `2` | Paralelismo de CMake |
| `TARGET` | Auto | Target triple para cross-compilation |
| `HOME` | — | Determina el `data_dir` por defecto |

`.env` se carga automáticamente si existe. Las variables de modelo se aplican al cargar la configuración.

---

## Persistencia

```
<data_dir>/
  core.mv2                  # identidad del agente
  manifest.json             # catálogo de segmentos
  conversations/            # conv_YYYY-MM-DD_NNN.mv2
  knowledge/                # know_YYYY-MM-DD_NNN.mv2
  knowledge_index.jsonl     # índice de búsqueda por keywords
  knowledge_tombstones.jsonl # fuentes borradas con /unlearn
  feed_queue.jsonl          # cola persistente de URLs
  languages_catalog.json    # caché del catálogo de lenguajes
  .lock                     # PID de la instancia activa
```

- Todas las escrituras son **atómicas**: escribir temporal → `fsync` → `rename` → `fsync` del directorio padre.
- El **FileLock** rechaza instancias concurrentes y elimina locks obsoletos de procesos muertos (`/proc/<pid>`).
- La sesión se vuelca a `.mv2` **cada 5 interacciones** y al salir (`Drop`), con recuento real de tokens vía el tokenizador del modelo.
- El conocimiento se indexa en caliente en `knowledge_index.jsonl` y, además, se persiste en segmentos `.mv2`; `/reindex` reconstruye el índice desde estos últimos.

---

## Arquitectura

```
REPL (main.rs) ───────────────┐
                              ├──> Agent (agent.rs)
API HTTP (api.rs) ────────────┘         │
                                        ├── LlamaContext  (llama/)
                                        ├── PromptBuilder (prompt.rs)
                                        ├── ContextPolicy (context_policy.rs)
                                        ├── Session       (session.rs)
                                        ├── KnowledgeIndex(retrieval.rs)
                                        └── MemvidWriter  (memvid/)
```

**Flujo de una consulta:**

1. `KnowledgeIndex::search_with_scores()` recupera los 10 candidatos mejor puntuados.
2. Se descartan los que no caben en el presupuesto de RAG (contexto − system − entrada − margen).
3. `ContextPolicy::trim_messages()` recorta el historial conservando los mensajes más recientes.
4. `PromptBuilder::build()` aplica la plantilla del modelo con el contexto RAG.
5. `LlamaContext::generate()` infiere y se devuelve la respuesta (+ debug opcional).

**Scoring RAG** (keyword, sin embeddings): coincidencia en fuente ×20, palabra exacta en contenido ×50, coincidencia parcial ×30 normalizada por densidad, e ID ×5.

### Módulos (24)

```
memvid-agent-core/src/
  lib.rs               — crate library (memvid_agent_core)
  main.rs              — REPL, asistente de primer arranque, /token, finetune UI
  agent.rs             — Orquestación: chat, ingesta, búsqueda, modelos
  api.rs               — Servidor HTTP OpenAI-compatible
  books_catalog.rs     — Catálogo free-programming-books
  chunker.rs           — Chunking Fixed / Paragraph / Heading
  config.rs            — Configuración persistente y validación
  context_policy.rs    — Presupuesto de contexto y recorte de historial
  extractor.rs         — Extracción PDF/EPUB/HTML/Markdown/texto
  feeds.rs             — Parseo RSS/Atom
  finetune.rs          — Corpus, estimación y ejecución de fine-tuning
  generation.rs        — Pipeline prompt + RAG + generación
  languages_catalog.rs — Catálogo y descarga de docs por lenguaje
  llama/               — FFI de llama.cpp (context.rs, ffi.rs)
  memvid/              — Persistencia .mv2 (writer/reader/manifest/playlist)
  models.rs            — Descarga del modelo activo
  models_catalog.rs    — Catálogo de 21 modelos GGUF
  prompt.rs            — Plantillas ChatML/Llama3/Mistral/Raw
  queue.rs             — Cola de URLs persistente
  retrieval.rs         — Índice de conocimiento y chunking de texto
  selfcheck.rs         — Verificación de runtime (`--self-test`)
  session.rs           — Buffer de sesión y flush a memoria
  shutdown.rs          — Señales SIGINT/SIGTERM
  types.rs             — Tipos compartidos y defaults
  utils.rs             — FileLock, atomic_write, SHA-256, truncado UTF-8
  web_fetcher.rs       — HTTP con rate limit, timeouts y reintentos
```

---

## Fine-tuning (`/finetune <lang>`)

Después de `/learn <lang>`, se puede afinar el modelo activo con esos mismos documentos (también se ofrece automáticamente al terminar `/learn`):

1. **Corpus:** concatena los chunks indexados cuyo `source` empieza por `"<lang>/"`.
2. **Estimación previa:** ~16 bytes/parámetro (peso maestro, gradiente y dos momentos AdamW en f32) + ~1,5 GB de overhead; también estima el tiempo según parámetros, épocas y threads.
3. **Comprobación de RAM:** lee la RAM disponible del sistema y avisa si el run no cabe.
4. **Ejecución:**
   - Si encuentra `llama-finetune` (`LLAMA_FINETUNE_BIN`, `finetune.binary_path` o `PATH`), lo ejecuta y ofrece cambiar al GGUF resultante.
   - Si no lo encuentra, escribe el corpus y un script `finetune_<lang>.sh` para ejecutarlo en otra máquina.

> ⚠️ Es un **full fine-tune** (todos los pesos), no LoRA. En CPU es lento y consume mucha RAM: está pensado para preparar el trabajo en local y ejecutarlo en una máquina mayor. aten-ia **no compila** `llama-finetune`; constrúyelo desde el fork:
>
> ```bash
> cmake -B build -DLLAMA_BUILD_EXAMPLES=ON && cmake --build build --target llama-finetune -j
> ```

---

## Build portable y releases

### Binario portable (glibc antiguo)

```bash
./scripts/build-portable.sh
./scripts/validate-compat.sh dist/aten-ia
```

Usa Docker (`docker/Dockerfile.build`) con **Ubuntu 20.04** y compila con `ATEN_PORTABLE=1`, enlazando `libstdc++` y `libgomp` estáticamente. El binario resultante requiere como máximo **GLIBC_2.31** y solo depende de bibliotecas base del sistema (`libc`, `libm`, `libgcc_s`, `libpthread`, `libdl` y, en algunos casos, `librt`).

| Distribución | Versión | glibc | Compatible |
|---|---|---|---|
| Ubuntu | 20.04 LTS | 2.31 | ✅ |
| Ubuntu | 22.04 / 24.04 / 26.04 | 2.35 / 2.39 / 2.41 | ✅ |
| Debian | 11 (bullseye) / 12 / 13 | 2.31 / 2.36 / 2.38 | ✅ |

Archivos implicados: `docker/Dockerfile.build`, `scripts/build-portable.sh`, `scripts/validate-compat.sh`, `memvid-agent-core/.cargo/config.toml` (linker aarch64).

### Release

```bash
git tag v0.3.12 && git push --tags
```

Los workflows (`.github/workflows/release.yml` y `.gitea/workflows/release.yml`) compilan en un contenedor `node:20-bullseye` (glibc 2.31) para:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu` (cross-compilado)

Artefactos: `aten-ia-<target>.tar.gz`, paquete `.deb` (`amd64`/`arm64`) y checksums SHA-256, publicados en GitHub Releases y en Gitea.

También existe un `snapcraft.yaml` que empaqueta el binario ya compilado.

---

## Tests y CI

```bash
cd memvid-agent-core
cargo test --locked -- --test-threads=1   # ~556 tests (unitarios + integración)
cargo test --locked --lib -- --test-threads=1
cargo fmt --all -- --check
cargo clippy --locked --lib
```

Los tests de integración (`tests/functional.rs`, `tests/writer_integration.rs`, `tests/kv_cache_and_history.rs`, `tests/cli.rs`, `tests/network_ingest.rs`, `tests/api_server.rs`) usan `tempfile::tempdir()` y `LlamaContext::null()`: **no necesitan GGUF**. Los de red usan un servidor HTTP local. Se requiere `--test-threads=1` por las variables de entorno (`MODEL_PATH`) y el flag global de shutdown.

Suite completa (local y CI):

```bash
./scripts/ci/verify.sh
```

Incluye `metadata --locked`, `fmt`, `clippy`, todos los tests, `--version`, `--self-test` y comprobación de documentación. CI añade el job de docs y verifica el binario; el release ejecuta los mismos gates antes de empaquetar.

Documentación adicional: [`SDD.md`](SDD.md) (especificación y sistema de verificación), [`PORTABILITY_SPEC.md`](PORTABILITY_SPEC.md).

---

## Limitaciones conocidas

- **RAG por keywords**, no embeddings semánticos: no encuentra sinónimos ni paráfrasis.
- La API **serializa la inferencia** con un mutex: no hay paralelismo real de generación.
- El streaming SSE envía el contenido en un único chunk (no token a token).
- Las peticiones a la API no se guardan en la memoria persistente.
- La plantilla `raw` ignora historial y contexto RAG.
- `n_gpu_layers` existe en la configuración, pero los builds no incluyen backends GPU (CUDA/Metal/Vulkan).
- `/batch` no incrementa la barra por URL individual (sí procesa todas y muestra el resumen final).

---

## Documentación

- [`wiki/`](wiki/) — instalación, uso, arquitectura y desarrollo
- [`man/aten-ia.1`](man/aten-ia.1) — página de manual
- [`PORTABILITY_SPEC.md`](PORTABILITY_SPEC.md)
- [`SECURITY.md`](SECURITY.md) — cómo reportar vulnerabilidades
- [`PLAN.md`](PLAN.md) — plan de auditoría y mejoras
- [`SDD.md`](SDD.md) — especificación y sistema de verificación/CI
- [`.opencode/agents/verifier.md`](.opencode/agents/verifier.md) — agente verificador
- [`AGENTS.md`](AGENTS.md) — notas para agentes/desarrolladores

## Licencia

Apache 2.0. Copyright 2026 **Dinámicas Electrónicas** <dinamicas.electronicas@gmail.com>.

Contacto: dinamicas.electronicas@gmail.com.
