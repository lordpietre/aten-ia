# aten-ia

Agente de IA interactivo de línea de comandos con **inferencia LLM local**, **memoria persistente** y **RAG por palabras clave**. Escrito en Rust, ejecuta modelos GGUF en CPU con llama.cpp (fork **TurboQuant**) y expone una API compatible con OpenAI.

No requiere GPU, API keys ni servicios externos para inferir. Solo se necesita red para descargar el modelo la primera vez o para ingerir contenido de Internet.

## Características

- **LLM local** — llama.cpp (fork TurboQuant), CPU only, plantillas ChatML / Llama 3 / Mistral / Raw.
- **KV-cache configurable** — `f32`, `f16`, `bf16`, `q8_0`, `q4_0` y codecs TurboQuant `turbo2/3/4` (con flash-attn automático).
- **Memoria persistente** — conversaciones y conocimiento en segmentos `.mv2` (memvid-core v2) con escrituras atómicas.
- **RAG keyword** — scoring ponderado por fuente, palabra exacta, coincidencia parcial e ID, con presupuesto de tokens dinámico.
- **Multi-formato** — ingesta de PDF, EPUB, HTML, Markdown, TXT, URLs, lotes de URLs y feeds RSS/Atom.
- **Aprendizaje de lenguajes** — `/learn` descarga e indexa documentación de programación desde free-programming-books.
- **Fine-tuning** — full fine-tune del modelo activo sobre los docs aprendidos, con estimación previa de RAM y tiempo.
- **API OpenAI-compatible** — `/v1/chat/completions` (con SSE opcional), `/v1/models`, `/health` y `/token`, con autenticación Bearer.
- **Integración** — `/token` genera la configuración de OpenCode y muestra la de VS Code (Continue).
- **Robustez** — FileLock contra instancias concurrentes, flush cada 5 interacciones y apagado graceful (SIGINT/SIGTERM).

## Tecnologías

| Componente | Tecnología |
|---|---|
| Lenguaje | Rust (edition 2024, mínimo 1.85) |
| LLM | llama.cpp (fork TurboQuant) vía FFI |
| FFI | bindgen + CMake |
| Persistencia | memvid-core v2 (`.mv2`) + JSONL |
| HTTP servidor | `TcpListener` propio, un hilo por conexión, mutex de inferencia |
| HTTP cliente | `ureq` (rustls), timeouts, rate limiting y reintentos |
| Extracción | `pdf-extract`, `epub`, HTML→texto/Markdown propio |
| Feeds | `feed-rs` (RSS/Atom) |

## Inicio rápido

```bash
cd memvid-agent-core
cargo build      # primera vez: ~30 min (compila llama.cpp)
cargo run
```

La primera ejecución lanza un asistente para elegir modelo, directorio de datos y API. Si falta el GGUF, se descarga automáticamente.

## Enlaces

- [Installation](Installation) — instalación, paquetes y variables de entorno
- [Usage](Usage) — comandos del REPL, API y configuración
- [Architecture](Architecture) — módulos, flujo de consulta y persistencia
- [Development](Development) — build, tests, CI y releases

## Versión y licencia

Versión actual: **0.3.12**. Licencia: **Apache 2.0**.
