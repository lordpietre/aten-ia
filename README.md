# memvid-agent-core

Interactive AI agent CLI con inferencia LLM local vía [llama.cpp](https://github.com/ggml-org/llama.cpp) (TurboQuant fork) y persistencia en `.mv2` usando [memvid-core](https://crates.io/crates/memvid-core).

Este proyecto ya integra un flujo completo de:
- configuración persistente con `config.json`
- selección y descarga de modelos desde catálogo
- prompt templates (
  `chatml`, `llama3`, `mistral`, `raw`)
- persistencia atómica de conversaciones y knowledge en `.mv2`
- índice ligero `knowledge_index.jsonl` para búsquedas rápidas
- API HTTP compatible OpenAI con token auth
- catálogo de lenguajes + descarga de docs online como conocimiento indexado
- **tipo de KV-cache configurable** (`f16` por defecto; codecs TurboQuant `turbo2/3/4`)

Para el roadmap de mejoras y cambios recientes, ver [`IMPROVEMENTS_PLAN.md`](memvid-agent-core/IMPROVEMENTS_PLAN.md).

## Quick start

### Prerrequisitos

- **Rust** ≥ 1.95.0 (edition 2024)
- **System deps**: `build-essential cmake libssl-dev clang libgomp1`

### Build & run

La primera compilación tarda ~30 min (compila llama.cpp desde fuente). Las siguientes son <1 s gracias al caché.

```bash
cd memvid-agent-core
cargo build
cargo run
```

La compilación en CI usa un fallback de **librerías precompiladas**: si existe un release en GitHub con `llama-libs-{target}.tar.gz`, lo descarga en vez de compilar. Si no, compila desde fuente con `CMAKE_BUILD_PARALLEL_LEVEL=1` para evitar OOM.

### Release

```bash
git tag v0.1.0 && git push --tags
```

Esto dispara GitHub Actions para compilar binarios + `.deb` para x86_64 y ARM64, y los sube a un GitHub Release.

Si no existe un modelo GGUF en `config.json`, el agente descarga automáticamente `Qwen2.5-0.5B-Instruct` (~350 MB) desde la URL configurada en el catálogo.

### Descarga precompilada

| Archivo | Plataforma |
|---|---|
| `memvid-agent-core-x86_64-unknown-linux-gnu.tar.gz` | Linux x86_64 |
| `memvid-agent-core-aarch64-unknown-linux-gnu.tar.gz` | Linux ARM64 |
| `memvid-agent-core_<version>_amd64.deb` | Debian/Ubuntu x86_64 |
| `memvid-agent-core_<version>_arm64.deb` | Debian/Ubuntu ARM64 |

Los `.deb` incluyen el binario + documentación. Dependencias: `libc6 libstdc++6 libgomp1`.

### Test

```bash
cd memvid-agent-core
cargo test
```

## Comandos disponibles

**Chat**
| Comando | Descripción |
|---|---|
| `<mensaje>` | Chatea con el agente |

**Modelo**
| Comando | Descripción |
|---|---|
| `/model` | Muestra modelo activo |
| `/model <id>` | Descarga y cambia a un modelo |
| `/models` | Lista catálogo de modelos disponibles |

**Conocimiento**
| Comando | Descripción |
|---|---|
| `/learn <lang>` | Descarga e indexa documentación de un lenguaje |
| `/unlearn <lang>` | Elimina todo el conocimiento de un lenguaje |
| `/fetch <url>` | Descarga URL, extrae texto, chunkea e indexa |
| `/ingest <file>` | Indexa archivo local (PDF/EPUB/MD/HTML/txt) |
| `/search <query>` | Busca en el knowledge index |
| `/reindex` | Reconstruye índice desde archivos `.mv2` |

**Utilidades**
| Comando | Descripción |
|---|---|
| `/token` | Inicia servidor API OpenAI-compatible |
| `/batch <file>` | Procesa lote de URLs desde archivo |
| `/load <file>` | Carga archivo en sesión (sin indexar) |
| `/stats` | Estadísticas del agente |
| `/history` | Historial de conversaciones |
| `/config` | Muestra configuración actual |
| `/kv [k] [v]` | Consulta o cambia los tipos de KV-cache (ej. `/kv f16 turbo3`) |
| `/help` | Muestra ayuda completa |
| `/exit` | Salir |

## Modelos disponibles

El catálogo incluye 20 modelos GGUF Q4_K_M listos para descargar y usar. Selecciona con `/MODEL <id>`:

| # | Modelo | Tamaño | ID (para `/MODEL`) | ChatML Template |
|---|---|---|---|---|
| 1 | Qwen2.5-Coder-0.5B-Instruct | 340 MB | `qwen2.5-coder-0.5b` | ✓ |
| 2 | Qwen2.5-Coder-1.5B-Instruct | 900 MB | `qwen2.5-coder-1.5b` | ✓ |
| 3 | Qwen2.5-0.5B-Instruct | 350 MB | `qwen2.5-0.5b` | ✓ |
| 4 | Qwen2.5-1.5B-Instruct | 950 MB | `qwen2.5-1.5b` | ✓ |
| 5 | Gemma-3-2B-Instruct | 1.2 GB | `gemma3-2b` | ✓ |
| 6 | SmolLM3-3B-Instruct | 1.9 GB | `smollm3-3b` | ✓ |
| 7 | Phi-4-mini-Instruct | 2.3 GB | `phi4-mini` | ✓ |
| 8 | Qwen3-4B-Instruct | 2.4 GB | `qwen3-4b` | ✓ |
| 9 | Qwen3.5-4B-Instruct | 2.4 GB | `qwen3.5-4b` | ✓ |
| 10 | Gemma-3-4B-Instruct | 2.4 GB | `gemma3-4b` | ✓ |
| 11 | Llama-3.2-1B-Instruct | 780 MB | `llama-3.2-1b` | Llama3 |
| 12 | Llama-3.2-3B-Instruct | 1.9 GB | `llama-3.2-3b` | Llama3 |
| 13 | MiniCPM3-4B | 2.4 GB | `minicpm3-4b` | ✓ |
| 14 | DeepSeek-Coder-6.7B-Instruct | 4.0 GB | `deepseek-coder-6.7b` | ✓ |
| 15 | Qwen2.5-Coder-7B-Instruct | 4.2 GB | `qwen2.5-coder-7b` | ✓ |
| 16 | Qwen3-7B-Instruct | 4.2 GB | `qwen3-7b` | ✓ |
| 17 | Mistral-7B-Instruct-v0.3 | 4.2 GB | `mistral-7b` | Mistral |
| 18 | CodeLlama-7B-Instruct | 4.2 GB | `codellama-7b` | Llama3 |
| 19 | Phi-4-14B | 8.4 GB | `phi4-14b` | ✓ |
| 20 | Gemma-2-9B-It | 5.4 GB | `gemma2-9b` | ✓ |

## Ejemplos de uso

### 1. Chatear con el agente
```bash
# Una vez dentro del REPL, escribe directamente:
¿cómo ordenas un vector en Rust?
# El agente responde aplicando el chat template del modelo activo
```

### 2. Cambiar de modelo
```bash
/MODELS              # lista todos los modelos disponibles
/MODEL qwen3-4b      # descarga Qwen3-4B Q4_K_M y lo activa
/MODEL current       # muestra el modelo activo
```

### 3. Cargar documentación como conocimiento
```bash
/LOAD-ONLINE rust    # descarga docs de Rust, las chunkea y las indexa
/SEARCH unsafe       # busca en el knowledge indexado
```

### 4. Ingestar un archivo local (txt, md, html, pdf, epub)
```bash
/INGEST manual.txt       # carga texto y lo indexa como knowledge
/INGEST paper.pdf        # auto-detecta PDF, extrae texto, indexa
/INGEST book.epub        # auto-detecta EPUB, extrae texto, indexa
/INGEST-PDF paper.pdf    # explícitamente extrae PDF/EPUB e indexa
/SEARCH contenido        # busca en todo el knowledge indexado
```

### 5. Iniciar el servidor API
```bash
/TOKEN               # genera un token e inicia el servidor HTTP
```
```bash
# Desde otro terminal:
curl http://localhost:8787/v1/chat/completions \
  -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"messages":[{"role":"user","content":"Hola"}]}'
```

### 6. Historial de conversaciones
```bash
/HISTORY             # muestra todas las conversaciones almacenadas en .mv2
/STATS               # muestra estadísticas: interacciones, knowledge, modelo
```

### 7. Reindexar knowledge desde archivos .mv2
```bash
/REINDEX             # reconstruye knowledge_index.jsonl desde los segmentos .mv2
```

## Configuración

El proyecto usa `config.json` en la raíz de `memvid-agent-core`.

Valores importantes:

- `data_dir`: directorio de persistencia (`memvid_data` por defecto)
- `model.path`: ruta al archivo GGUF
- `model.name`: nombre de modelo activo
- `model.n_ctx`: tamaño de contexto
- `model.n_gpu_layers`: capas GPU (actualmente CPU only / 0)
- `model.chat_template`: template de prompt
- `model.kv_type_k` / `model.kv_type_v`: tipo de KV-cache (`f16` por defecto;
  `f32`, `bf16`, `q8_0`, `q4_0`, o codecs TurboQuant `turbo2`/`turbo3`/`turbo4`).
  Recomendado mantener K en mayor o igual precisión que V ("V is free, K is everything").
- `generation.top_k`, `top_p`, `temp`, `max_tokens`
- `api.enabled`, `api.host`, `api.port`, `api.token`
- `languages.installed`

### Variables de entorno

Las siguientes variables son compatibles como overrides:

| Variable | Default | Descripción |
|---|---|---|
| `MODEL_PATH` | `models/default-model.gguf` | Ruta a modelo GGUF |
| `MODEL_NAME` | `smollm2-360m` | Nombre de modelo |
| `MODEL_CTX` | `4096` | Tamaño de contexto |
| `MODEL_URL` | `https://...` | URL de descarga de modelo |

## Estructura de memoria

```
$DATA_DIR/
├── core.mv2
├── manifest.json
├── conversations/
├── knowledge/
└── archive/
```

- Conversaciones y knowledge se guardan como segmentos `.mv2`
- `manifest.json` indexa segmentos y metadatos
- `knowledge_index.jsonl` es un índice local para búsquedas
- La escritura es atómica: temp file + fsync + rename

## Funcionalidades actuales

- Configuración persistente con archivo JSON
- Descarga y selección de modelos desde catálogo
- Cambio dinámico de modelo en tiempo real
- Prompt templates para distintos formatos
- Persistencia segura de `.mv2` para conversaciones y knowledge
- Buscador de knowledge indexado (`/SEARCH`)
- Reindexado desde `.mv2` (`/REINDEX`)
- API local compatible OpenAI con token auth
- Descarga de documentación de lenguajes desde free-programming-books
- `FileLock` para evitar instancias múltiples en el mismo `data_dir`
- Extracción de texto de PDFs y EPUBs (`pdf-extract` + `epub`)
- Detección automática de formato por extensión de archivo

## Limitaciones actuales

- RAG es keyword-based (word matching), sin embeddings semánticos
- API: un hilo por conexión (inferencia serializada por mutex); aún sin streaming SSE
- Template `Raw` ignora historial y RAG context
- Dedup por checksum solo en la ruta de `/fetch` (opt-in); `add_entry` genérico sigue apilando
- Sin catálogo de idiomas offline embebido
- Sin `MODEL_GPU_LAYERS` env var (CPU only)

## Archivo `models_catalog.json`

El catálogo de modelos esta embebido en el binario y contiene información de descarga, contexto recomendado y template de prompt.

## llama-cpp-turboquant

Este repositorio incluye el submódulo `llama-cpp-turboquant`, un fork de llama.cpp con TurboQuant. Se compila con CMake y bindgen para generar el wrapper Rust.

## Dependencias principales

- `memvid-core` | `.mv2` persistence
- `serde` / `serde_json` | serialización JSON
- `chrono` | timestamps
- `uuid` | IDs UUID v4
- `anyhow` | manejo de errores
- `sha2` | checksums SHA-256
- `ureq` | descargas HTTP
- `pdf-extract` | extracción de texto de PDFs
- `epub` | extracción de texto de EPUBs
- `indicatif` | barras y spinners
- `colored` | salida de terminal colorida
- `dotenvy` | carga de `.env`

## Cómo contribuir

- Prueba primero con `cargo test`. En máquinas con poca RAM (~8 GB) la suite a
  máxima paralelización puede agotar memoria/espacio temporal (muchos tests
  escriben segmentos `.mv2` con `fsync` a la vez); usa
  `cargo test -- --test-threads=2` para una ejecución estable.
- Mantén los cambios en `src/` separados de `llama-cpp-turboquant/`
- No modifiques la lógica de build de llama.cpp sin entender `build.rs`
- El roadmap de mejoras y correcciones vive en
  [`IMPROVEMENTS_PLAN.md`](memvid-agent-core/IMPROVEMENTS_PLAN.md)

## Licencia

Apache 2.0
