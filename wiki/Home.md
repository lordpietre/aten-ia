# aten-ia

Agente de IA interactivo CLI con inferencia LLM local, memoria persistente y búsqueda semántica.

## Características Principales

- **LLM Local**: Inferencia via llama.cpp (fork TurboQuant), sin depender de APIs externas
- **Memoria Persistente**: Conversaciones almacenadas en formato `.mv2` (memvid-core v2)
- **RAG por Keywords**: Búsqueda de información relevante en documentos ingestados
- **API OpenAI-compatible**: Servidor HTTP/1.1 para integración con otras herramientas
- **Multi-formato**: Ingesta PDF, EPUB, HTML, Markdown, TXT y URLs

## Tecnologías

| Componente | Tecnología |
|------------|------------|
| Lenguaje | Rust (edition 2024) |
| LLM | llama.cpp (TurboQuant fork) |
| Persistencia | memvid-core v2 |
| FFI | bindgen + CMake |
| HTTP Server | TcpListener (custom, single-threaded) |

## Inicio Rápido

```bash
cd memvid-agent-core
cargo build
cargo run
```

## Enlaces

- [Installation](Installation)
- [Usage](Usage)
- [Architecture](Architecture)
- [Development](Development)