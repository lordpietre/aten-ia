# Architecture

## Visión General

```
┌─────────────────────────────────────────────────────┐
│                    REPL (main.rs)                    │
│              OpenAI API (api.rs)                     │
└─────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────┐
│                     Agent (agent.rs)                 │
│  ┌─────────┐  ┌────────────┐  ┌─────────────────┐  │
│  │ Session │  │ LlamaContext│  │ KnowledgeIndex │  │
│  │(messages)│  │   (LLM)    │  │   (RAG search) │  │
│  └─────────┘  └────────────┘  └─────────────────┘  │
└─────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────┐
│           MemvidWriter (persistence)                 │
│    .mv2 segments + knowledge_index.jsonl            │
└─────────────────────────────────────────────────────┘
```

## Módulos principales

| Módulo | Archivo | Descripción |
|--------|---------|-------------|
| Agent | `agent.rs` | Orquestación, chat, ingestión |
| Generation | `generation.rs` | Pipeline LLM, RAG, prompt building |
| Context Policy | `context_policy.rs` | Gestión del contexto, trimming |
| Prompt | `prompt.rs` | Plantillas de chat (ChatML, Llama3, Mistral) |
| Retrieval | `retrieval.rs` | Índice de conocimiento, búsqueda keyword |
| Session | `session.rs` | Buffer de mensajes en memoria |
| LlamaContext | `llama/context.rs` | FFI con llama.cpp |

## Flujo de una query

1. **RAG Search**: `KnowledgeIndex::search()` encuentra documentos relevantes
2. **Context Trimming**: `ContextPolicy::trim_messages()` ajusta historial
3. **Prompt Building**: `PromptBuilder::build()` assembla prompt con template
4. **LLM Generation**: `LlamaContext::generate()` ejecuta inferencia
5. **Response**: Texto generado + info de debug (si `/debug` activo)

## Persistencia

### Archivos

- `memvid_data/manifest.json` - catálogo de segmentos
- `memvid_data/core.mv2` - identidad del agente
- `memvid_data/conversations/conv_*.mv2` - conversaciones
- `memvid_data/knowledge/know_*.mv2` - conocimiento
- `memvid_data/knowledge_index.jsonl` - índice para búsqueda

### Atomicidad

Todas las escrituras:
1. Escribir a archivo `.tmp_<uuid>`
2. `fsync` del archivo
3. `rename` atómico
4. `fsync` del directorio padre

## RAG (Retrieval Augmented Generation)

- **Búsqueda**: Keyword substring matching
- **Scoring**: Ponderación por:
  - Coincidencias en nombre de archivo (20x)
  - Palabras exactas en contenido (50x)
  - Coincidencias parciales (30x, normalizado)
- **Límite**: Tokens de RAG limitados por budget disponible

## API HTTP

Servidor single-threaded TCP:
- Puerto configurable (default 8080)
- Endpoints OpenAI-compatible: `/v1/chat/completions`, `/v1/models`
- Sin streaming SSE