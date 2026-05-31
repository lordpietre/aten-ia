# Plan de Desarrollo: `memvid-agent-core`

## Análisis Técnico Profundo

### Stack Tecnológico

| Capa | Tecnología | Detalle |
|---|---|---|
| **Lenguaje** | Rust edition 2024 (min 1.75 → actual 1.95+) | FFI con C via bindgen, `unsafe extern "C"` |
| **LLM** | llama.cpp fork (turboquant) | cmake + bindgen, ~100 FFI functions, CPU-only |
| **Persistencia** | memvid-core v2.0.139 | Contenedores `.mv2` con atomic rename + fsync en todos los paths |
| **Serialización** | serde + serde_json | ConversationBatch, KnowledgeEntry, Manifest, Config |
| **HTTP cliente** | ureq 3 + rustls | Descarga de modelos, fetch de catálogos, sin openssl |
| **HTTP servidor** | Raw TCP (TcpListener) | HTTP/1.1 manual, sin crate HTTP, single-threaded |
| **Búsqueda** | Keyword word-matching | Scored sobre JSONL, split por whitespace, substring match |
| **Templates chat** | ChatML, Llama3, Mistral, Raw | Implementación propia, Raw ignora todo |
| **Auth** | Bearer token | UUID v4 generado, validación manual en headers |
| **Chunking** | Heading/Paragraph/Fixed | 1024 chars default, 200 overlap, semántico por secciones/párrafos |
| **Build** | cmake crate + bindgen | Linkea estáticamente 5 libs: llama, llama-common, ggml-cpu, ggml, ggml-base |

### Modelos Soportados (20 en catálogo)

| Rango | Parámetros | Modelos |
|---|---|---|
| Micro | 360M | SmolLM2-360M |
| Pequeño | 0.5B-1.5B | Qwen2.5-Coder-0.5B/1.5B, Qwen2.5-0.5B/1.5B, Llama-3.2-1B |
| Mediano | 2B-4B | Gemma3-2B/4B, SmolLM3-3B, Qwen3-4B, Qwen3.5-4B, MiniCPM3-4B, Llama-3.2-3B, Phi-4-mini-3.8B |
| Grande | 6.7B-14B | DeepSeek-Coder-6.7B, Qwen2.5-Coder-7B, Qwen3-7B, Mistral-7B, CodeLlama-7B, Gemma2-9B, Phi-4-14B |

Todos Q4_K_M, CPU-only, n_ctx_recommended 4096-8192.

Modelo por defecto en `config.json`: `Qwen2.5-0.5B-Instruct` (chatml, n_ctx: 8192).

### Arquitectura de Persistencia

```
memvid_data/
├── .lock                          ← FileLock (singleton instance)
├── core.mv2                       ← Identity + metadata inicial
├── manifest.json                  ← Segment registry (conversation + knowledge)
├── knowledge_index.jsonl          ← Append-only O(1) por entrada
├── conversations/                 ← .mv2 segments named conv_YYYYMMDD_NNN.mv2
├── knowledge/                     ← .mv2 segments named know_YYYYMMDD_NNN.mv2
└── archive/                       ← Segmentos retirados
```

Atomic write pattern en todos los paths: `write → fsync → rename → fsync(parent)`.

### Estado de Componentes (Mayo 2026)

| Componente | Líneas | Tests | Estado |
|---|---|---|
| `main.rs` | 931 | 0 | ✅ REPL completo con setup wizard |
| `agent.rs` | 690 | 22 | ✅ Orchestrador con chat, ingest, reindex, switch model, fetch, batch, books |
| `config.rs` | 366 | 20 | ✅ Load/save/validate + env overrides + ingestion config |
| `prompt.rs` | 251 | 9 | ✅ 4 templates + developer prompt + RAG injection |
| `context_policy.rs` | 234 | 13 | ✅ Token-budget trimming con tokenización real |
| `generation.rs` | 90 | 2 | ✅ Pipeline: search → trim → build → generate |
| `types.rs` | 455 | 26 | ✅ Tipos serializables + Format tests + defaults + Chunk/IngestionConfig |
| `retrieval.rs` | 501 | 24 | ✅ JSONL append-only, word-match search, chunking, rebuild |
| `session.rs` | 236 | 15 | ✅ Batch + flush cada 5 interacciones |
| `api.rs` | 508 | 15 | ✅ HTTP/1.1 raw TCP, /health, /v1/models, /v1/chat, /token |
| `models.rs` | 70 | 0 | ✅ Auto-download con progress bar |
| `models_catalog.rs` | 196 | 5 | ✅ Catálogo desde JSON externo, 20 modelos, SHA-256 verify |
| `languages_catalog.rs` | 509 | 21 | ✅ Fetch free-programming-books, HTML strip, chunking |
| `books_catalog.rs` | 339 | 13 | ✅ Fetch EbookFoundation, prepare metadata + ingest |
| `llama/context.rs` | 262 | 0 | ✅ Init, tokenize, generate, sample, Drop safe |
| `memvid/writer.rs` | 471 | 13 | ✅ .mv2 con atomic rename + fsync + segment rollover |
| `memvid/reader.rs` | 280 | 9 | ✅ .mv2 read con timeline query + frame enumeration |
| `memvid/manifest.rs` | 192 | 9 | ✅ Manifest atomic save/load |
| `memvid/playlist.rs` | 344 | 14 | ✅ Segment path generation, rollover logic |
| `utils.rs` | 201 | 13 | ✅ atomic_write, FileLock, SHA-256 |
| `web_fetcher.rs` | 226 | 9 | ✅ Fetch HTTP con rate limiting, retry, global throttle |
| `feeds.rs` | 138 | 5 | ✅ Parser RSS/Atom con feed-rs, filtrado de entries sin link |
| `queue.rs` | 258 | 9 | ✅ Cola persistente JSONL, estados, persistencia atómica |
| `extractor.rs` | 898 | 62 | ✅ HTML→text, HTML→markdown, metadata + entity parsing + PDF/EPUB + tempfile tests |
| `chunker.rs` | 473 | 23 | ✅ Chunking por headings/paragraphs/fixed + dedup |
| `lib.rs` | 21 | 0 | ✅ Module declarations + pub exports |
| `llama/mod.rs` + `ffi.rs` | 4 | 0 | ✅ FFI module re-exports |
| `memvid/mod.rs` | 4 | 0 | ✅ Memvid module re-exports |

**Total: ~10,800 líneas, ~499 tests (377 unit + 122 integración)**

### Limitaciones Actuales

1. **RAG sin embeddings** — puro keyword matching, no captura semántica
2. **HTML parsing básico** — `html_to_text` y `html_to_markdown` basados en regex, no manejan estructuras complejas (tablas, formularios)
3. **Sin deduplicación en índice** — `chunk_and_deduplicate` existe pero el índice KnowledgeIndex no deduplica por checksum/URL
4. **Sin streaming** — API bloqueante
5. **Sin GPU** — solo CPU
6. **Sin catálogo offline** — `languages_catalog` requiere fetch remoto siempre
7. **Sin feeds RSS/Atom** — no hay `/feed <url>`
8. **`switch_model` no preserva `developer_mode`**
9. **`add_entries()` batch rewritea full JSONL** (solo `add_entry()` individual es O(1) append)

---

## Nueva Visión: Sistema de Ingestión de Contenido

### Objetivo

Construir un **pipeline unificado de ingestión de contenido** que permita alimentar a memvid con:
- Páginas web individuales (URLs)
- Documentación técnica online
- Libros (PDF, EPUB, HTML)
- Feeds RSS/Atom
- Archivos locales (txt, md, pdf, epub)
- Batch de URLs desde archivo

### Pipeline de Ingestión Propuesto

```
Fuente → Fetcher → Extractor → Chunker → Indexer → Almacenamiento
                                                        ↓
                                                   .mv2 + JSONL
                                                        ↓
                                                   RAG (keyword)
```

### Componentes a Implementar

#### 1. Web Fetcher (`web_fetcher.rs`)
- `/fetch <url>` — fetch + extract + chunk + index en un solo comando
- Rate limiting, timeout configurable (30s default)
- User-Agent configurable
- Cache HTTP (ETag, Last-Modified)
- Límite de tamaño (5MB default)
- Soporte para URLs relativas → absolutas

#### 2. Content Extractor (`extractor.rs`)
- **HTML → Markdown** con `html2md` o regex-based mejorado
- **PDF → texto** con crate `pdf-extract` o `lopdf`
- **EPUB → texto** con crate `epub`
- **Metadata extraction** (title, author, date, description)
- Language detection (opcional)

#### 3. Smart Chunker (`chunker.rs`)
- Chunking semántico (por párrafos, headers, secciones)
- Overlap configurable (default 200 chars)
- Tamaño configurable (default 1024 tokens)
- Preservación de metadata por chunk (source, section, position)

#### 4. Feed Reader (`feeds.rs`)
- `/feed <rss-url>` — fetch RSS/Atom, extraer entries, chunkear cada una
- Soporte para RSS 2.0, Atom 1.0
- Rate limiting entre entries
- Deduplicación por entry GUID/link

#### 5. Batch Processor (`batch.rs`)
- `/batch <file>` — procesar archivo con URLs (una por línea)
- `/batch-dir <dir>` — procesar todos los archivos en un directorio
- Progreso visible con indicatif
- Reporte de éxito/fallo por item
- Reanudable (checkpoint file)

#### 6. URL Queue (`queue.rs`)
- Cola persistente de URLs pendientes
- Priorización (alto/bajo)
- Estado: pending, downloading, processing, done, failed
- Reintentos con backoff exponencial (3 intentos max)

### Nuevos Comandos CLI

| Comando | Descripción |
|---|---|
| `/fetch <url>` | Fetch + extract + chunk + index una URL ✅ |
| `/fetch-md <url>` | Fetch, convertir a markdown, mostrar sin indexar ✅ |
| `/feed <rss-url>` | Fetch RSS/Atom, indexar todos los entries ❌ |
| `/batch <file>` | Procesar archivo con URLs (una por línea) ✅ |
| `/queue` | Mostrar estado de la cola de URLs ❌ |
| `/queue-add <url>` | Agregar URL a la cola ❌ |
| `/queue-process` | Procesar todas las URLs en cola ❌ |
| `/ingest-pdf <file>` | Extraer texto de PDF e indexar ✅ |
| `/sources` | Listar fuentes de conocimiento indexadas ❌ |
| `/books` | Listar lenguajes de EbookFoundation ✅ |
| `/download-books <lang>` | Descargar + ingestar libros de un lenguaje ✅ |

### Nuevos Módulos

```
src/
├── web_fetcher.rs       ← Fetch HTTP con rate limiting, cache, UA configurable ✅
├── extractor.rs         ← HTML→MD, PDF→text, EPUB→text, metadata extraction ✅ (HTML→MD + metadata)
├── chunker.rs           ← Chunking semántico por secciones/párrafos ✅
├── feeds.rs             ← RSS/Atom parser + ingestion ❌ Pendiente Fase 4
├── batch.rs             ← Procesamiento batch con checkpoint ❌ (integrado en agent.rs + main.rs con progress bar)
├── queue.rs             ← Cola persistente de URLs ❌ Pendiente Fase 4
└── config/
    └── ingestion.rs     ← Config de ingestión ✅ (integrado en types.rs como IngestionConfig + serde defaults en config.json)
```

### Esquema de Configuración

```json
{
  "ingestion": {
    "user_agent": "aten-ia/0.1.0",
    "timeout_seconds": 30,
    "max_size_bytes": 5242880,
    "rate_limit_per_second": 2,
    "chunk_size_tokens": 1024,
    "chunk_overlap_chars": 200,
    "max_retries": 3,
    "retry_backoff_seconds": 5
  }
}
```

### Plan de Implementación por Fases

#### Fase 1 — Base ✅ (Completada)
- [x] `web_fetcher.rs` con `/fetch <url>` básico (GET, HTML, extract text, rate limiting, retry)
- [x] `extractor.rs` con HTML→text y HTML→markdown, extracción de metadata (title, description, lang)
- [x] `/batch <file>` simple (leer URLs, fetch secuencial con progress bar)
- [x] Tests unitarios para cada módulo

#### Fase 2 — Chunking Inteligente ✅ (Completada)
- [x] `chunker.rs` con chunking por headings, párrafos, fixed + deduplicación
- [x] Preservación de metadata en chunks (heading, source, index)
- [x] Reemplazar `KnowledgeIndex::chunk_text()` fijo por smart chunker
- [x] Integrar con pipeline existente de `/load` e `/ingest`

#### Fase 3 — Formats Rich ✅ (Completada)
- [x] Soporte PDF (crate `pdf-extract` 0.10)
- [x] Soporte EPUB (crate `epub` 2.1.5)
- [x] `/ingest-pdf` comando (también acepta EPUB)
- [x] Detección automática de formato por extensión (`Format::from_extension()`)
- [x] Tests unitarios (11 nuevos: Format + extractor error paths)

#### Fase 4 — Feeds y Cola ✅ Completada
- [x] `feeds.rs` con parser RSS/Atom (feed-rs, soporte RSS 2.0 + Atom 1.0)
- [x] `/feed <url>` comando (fetch feed + fetch+chunk cada entry, hasta 20)
- [x] `queue.rs` con cola persistente (JSONL, estados pending/processing/done/failed)
- [x] `/queue`, `/queue-add`, `/queue-process` comandos
- [x] Rate limiting global (función `global_throttle` ya existe en web_fetcher.rs)

#### Fase 5 — Mejoras RAG ✅ Completada (parcial)
- [x] Scoring ponderado (source 4x, contenido normalizado por densidad, id 1x)
- [x] Deduplicación por checksum (`add_entry_dedup`, existente) + por URL (`add_entry_dedup_url`)
- [x] Fuentes como filtro de búsqueda (`/search <q> from:<source>`)
- [ ] Embeddings semánticos + búsqueda híbrida (keyword + vector) — futuro

#### Fase 6 — API y UX ❌ Pendiente
- [ ] Streaming SSE en API
- [ ] Multi-threading en API server
- [ ] Web UI básica (opcional)
- [ ] Plugins/extensions (futuro)

### Arquitectura de Datos — Knowledge (actual vs propuesto)

**Actual** (`types.rs`):
```rust
pub struct KnowledgeEntry {
    pub id: String,
    pub source: String,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub checksum: String,
}
```

**Propuesto** (para Fase 5):
```rust
// Futuro: extender KnowledgeEntry con:
//   title: Option<String>,
//   url: Option<String>,
//   content_type: String,    // "text", "html", "markdown", "pdf", "epub"
//   language: Option<String>,
//   metadata: HashMap<String, String>,
//   chunk_index: u32,
//   parent_id: Option<String>,
```

### Integración con Sistema Existente

El pipeline se integra con:

1. **KnowledgeIndex** (`retrieval.rs`) — los chunks van al JSONL + .mv2 ✅
2. **Session** (`session.rs`) — contexto en sesión via `ingest_raw()` como system message ✅
3. **Config** (`config.rs`) — sección `ingestion` en config.json con serde defaults ✅
4. **Agent** (`agent.rs`) — métodos: `fetch_and_ingest()`, `process_url_batch()`, `ingest_file()` ✅
5. **main.rs** — comandos `/fetch`, `/fetch-md`, `/ingest`, `/ingest-pdf`, `/batch` ✅
6. **API** (`api.rs`) — endpoint `/v1/ingest` ❌ Pendiente

### Principios de Diseño

1. **Atomic writes** — mismo patrón temp + rename + fsync
2. **Graceful degradation** — si falla un recurso, continuar con los demás
3. **Progreso visible** — indicatif progress bars para todas las ops largas
4. **Deduplication first** — checksum + URL antes de indexar
5. **Testabilidad** — unit tests con mock HTTP server
6. **Backward compatibility** — no romper comandos existentes

### Riesgos y Mitigaciones

| Riesgo | Mitigación |
|---|---|
| HTML malformado | Usar crate `html2md` o `scraper` en vez de strip_html casero |
| PDFs muy grandes | Límite de tamaño configurable, chunking progresivo |
| Rate limiting bloquea | Retry con backoff, respetar `Retry-After` header |
| URLs maliciosas | Sanitizar URLs, no ejecutar scripts, timeout estricto |
| Dependencias grandes | Evaluar impacto en binary size antes de agregar crates |
| Memoria con muchos chunks | Streaming processing, no cargar todo en RAM |

---

*Documento mantenido en `plan.md`. Última actualización: Junio 2026.*
