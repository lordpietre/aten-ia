# Arquitectura

## Visión general

```
┌──────────────────────────────────────────────┐
│           Interfaz de usuario                │
│   REPL (main.rs)      API HTTP (api.rs)      │
└───────────────┬───────────────┬──────────────┘
                │               │
                └───────┬───────┘
                        ▼
┌──────────────────────────────────────────────┐
│               Agent (agent.rs)               │
│  ┌──────────┐ ┌─────────────┐ ┌────────────┐ │
│  │ Session  │ │ LlamaContext│ │ Knowledge  │ │
│  │(mensajes)│ │   (LLM)     │ │   Index    │ │
│  └──────────┘ └─────────────┘ └────────────┘ │
│  PromptBuilder · ContextPolicy · Finetune    │
└──────────────────────┬───────────────────────┘
                       ▼
┌──────────────────────────────────────────────┐
│        MemvidWriter (memvid/)                │
│  .mv2 + manifest.json + knowledge_index.jsonl│
└──────────────────────────────────────────────┘
```

El REPL y el servidor API comparten el mismo `Agent` protegido por `Arc<Mutex<_>>`. La inferencia se serializa, aunque cada conexión HTTP se atienda en su propio hilo.

## Módulos (24)

| Módulo | Archivo | Responsabilidad |
|---|---|---|
| Library | `lib.rs` | Declara los módulos públicos (`memvid_agent_core`) |
| REPL | `main.rs` | Interfaz, asistente inicial, `/token`, UI de fine-tune |
| Agent | `agent.rs` | Orquestación de chat, ingesta, búsqueda y cambio de modelo |
| API | `api.rs` | Servidor HTTP OpenAI-compatible |
| Books | `books_catalog.rs` | Catálogo free-programming-books |
| Chunker | `chunker.rs` | Estrategias Fixed / Paragraph / Heading |
| Config | `config.rs` | `config.json`, validación y overrides de entorno |
| Context Policy | `context_policy.rs` | Presupuesto de prompt y recorte de historial |
| Extractor | `extractor.rs` | PDF/EPUB/HTML/Markdown/texto |
| Feeds | `feeds.rs` | Parseo RSS/Atom |
| Fine-tune | `finetune.rs` | Corpus, estimación de recursos y ejecución |
| Generation | `generation.rs` | Pipeline RAG → prompt → inferencia |
| Languages | `languages_catalog.rs` | Catálogo y descarga de documentación por lenguaje |
| Llama | `llama/` | FFI (`ffi.rs`) y contexto/inferencia (`context.rs`) |
| Memvid | `memvid/` | Escritura, lectura, manifiesto y playlist |
| Models | `models.rs` | Descarga del modelo activo |
| Model catalog | `models_catalog.rs` | Catálogo de 21 modelos GGUF |
| Prompt | `prompt.rs` | Plantillas ChatML, Llama 3, Mistral y Raw |
| Queue | `queue.rs` | Cola persistente de URLs |
| Retrieval | `retrieval.rs` | Índice de conocimiento y búsqueda |
| Self-check | `selfcheck.rs` | Verificación de runtime para `--self-test` |
| Session | `session.rs` | Buffer de mensajes y flush |
| Shutdown | `shutdown.rs` | SIGINT/SIGTERM |
| Types | `types.rs` | Tipos compartidos y defaults |
| Utils | `utils.rs` | FileLock, atomic_write, SHA-256, UTF-8 |
| Web fetcher | `web_fetcher.rs` | HTTP con rate limit, timeout y reintentos |

## Flujo de una consulta

1. **RAG** — `KnowledgeIndex::search_with_scores()` puntúa las entradas y toma hasta 10 candidatos.
2. **Presupuesto RAG** — se añaden mientras quepan en `prompt_budget − system − entrada − 128`.
3. **Recorte de contexto** — `ContextPolicy::trim_messages()` conserva los mensajes más recientes que caben y mantiene los `system`.
4. **Prompt** — `PromptBuilder::build()` aplica la plantilla y añade el contexto RAG.
5. **Generación** — `LlamaContext::generate()` decodifica en trozos de `n_batch` (`min(2048, n_ctx)`).
6. **Persistencia** — el par usuario/asistente va a la sesión; cada 5 interacciones se vuelca a `.mv2`.
7. **Debug** — con `/debug` activo se guardan fuente, score y tokens de cada entrada usada.

Si el prompt supera `n_ctx − max_tokens`, la generación falla con un error explícito en lugar de truncar en silencio.

## RAG por palabras clave

No hay embeddings. La búsqueda (`search_with_filter`) exige que al menos una palabra de la consulta aparezca en el contenido y puntúa cada entrada:

| Coincidencia | Peso |
|---|---|
| En la fuente (`source`) | ×20 |
| Palabra exacta en contenido | ×50 |
| Substring en contenido | ×30, dividido por densidad (`(palabras+99)/200`) |
| En el ID | ×5 |

Desempate por orden de inserción más reciente (`sort_by score desc, índice desc`). Límite configurable por llamada (`5` en `/search`, `10` en generación).

Ventajas: sin dependencias, rápido, determinista y explicable. Limitación: no encuentra sinónimos ni paráfrasis.

## Contexto y plantillas

`ContextPolicy::new(n_ctx, max_tokens)` reserva `max_tokens` para la respuesta y deja el resto al prompt, con 64 tokens de margen. El historial se recorta desde el final (se conservan los mensajes más nuevos).

Plantillas (`prompt.rs`): `chatml`, `llama3`, `mistral`, `raw`. La plantilla `raw` ignora historial y RAG y envía la entrada tal cual.

## Persistencia

### Estructura

```
<data_dir>/
  core.mv2
  manifest.json
  conversations/conv_YYYY-MM-DD_NNN.mv2
  knowledge/know_YYYY-MM-DD_NNN.mv2
  knowledge_index.jsonl
  feed_queue.jsonl
  languages_catalog.json
  .lock
```

- `manifest.json` cataloga segmentos, tamaños y número de mensajes.
- `knowledge_index.jsonl` es el índice en caliente para búsquedas.
- `/reindex` reconstruye el índice desde `knowledge/*.mv2` y reescribe el JSONL.

### Atomicidad

Toda escritura sigue: temporal `.tmp_<uuid>` → `fsync` → `rename` → `fsync` del directorio padre.

### Consistencia

- **FileLock:** `data_dir/.lock` contiene `aten-ia <PID>`; se rechaza otra instancia y se limpian locks de procesos muertos (comprobando `/proc/<pid>`).
- **Sesión:** flush cada 5 interacciones y en `Drop`, con tokens reales del tokenizador (fallback a `len/4`).
- **Deduplicación:** las ingestas web usan `(source, checksum)` para no repetir chunks.

## Chunking

| Ruta | Estrategia | Tamaño | Solape |
|---|---|---|---|
| `/ingest` (texto/PDF/EPUB) | Heading | 4000 | 600 |
| `/fetch`, `/feed`, cola | Paragraph | 1024 | 200 |
| `/learn` | Heading | 4000 | 600 |
| `/download-books` | Entrada única con metadatos del catálogo | — | — |

El chunker es UTF-8 seguro: usa `floor_char_boundary()` en los cortes para no partir caracteres multibyte.

## API HTTP

- Servidor `TcpListener` con `set_nonblocking` y bucle que comprueba el flag de shutdown.
- Un hilo por conexión; timeout de lectura/escritura de 120 s.
- Autenticación Bearer con comparación en tiempo constante.
- Rutas: `GET /health`, `GET /v1/models`, `POST /v1/chat/completions`, `GET|POST /token`.
- `stream: true` responde `text/event-stream` con tres eventos y `[DONE]`.
- Cuerpo máximo 10 MB, cabeceras 64 KB.
- Las peticiones no se persisten en la sesión del REPL.

## Fine-tuning

`finetune.rs` separa la lógica pura (testeable) de la ejecución:

- **Corpus:** chunks cuyo `source` empieza por `<lang>/`.
- **Estimación:** 16 B/parámetro (AdamW: peso maestro + gradiente + 2 momentos) + 1,5 GB de overhead.
- **Tiempo:** heurística de tokens/s según parámetros, threads y épocas.
- **Pre-flight:** compara con la RAM disponible antes de continuar.
- **Ejecución:** usa `llama-finetune` si existe; si no, escribe corpus y script portable.

Es un full fine-tune, no LoRA; en CPU es lento y consume mucha RAM.

## Decisiones de diseño

| Decisión | Motivo |
|---|---|
| RAG keyword | Cero dependencias, rápido y explicable en local |
| `.mv2` + JSONL | Memoria binaria compacta más índice de búsqueda simple |
| Sin GPU por defecto | Portabilidad y builds reproducibles |
| HTTP propio | Evitar un framework async pesado para un solo endpoint |
| Un mutex para el LLM | El contexto de llama.cpp no es seguro entre hilos |
| CPU only en CI | Matriz x86_64/aarch64 sin runners con GPU |

## Limitaciones estructurales

- RAG sin semántica.
- Inferencia serializada.
- SSE sin streaming token a token.
- `usage` de la API a cero.
- `n_gpu_layers` presente en config pero sin backends GPU compilados.
