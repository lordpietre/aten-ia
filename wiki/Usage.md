# Usage

## Ejecución

```bash
cargo run
```

Esto inicia el REPL interactivo.

## Comandos del REPL

| Comando | Descripción |
|---------|-------------|
| `/help` | Mostrar ayuda |
| `/quit` o `/exit` | Salir |
| `/config` | Ver configuración |
| `/debug` | Activar modo debug RAG |
| `/kv [k] [v]` | Configurar KV cache (ej: `/kv f16 turbo3`) |
| `/models` | Listar modelos disponibles |
| `/model <nombre>` | Cambiar modelo |
| `/languages` | Ver idiomas disponibles |
| `/ingest <archivo>` | Ingestar archivo |
| `/learn <lang>` | Indexar documentación de idioma |
| `/search <query>` | Buscar en conocimiento |

## Ejemplos de uso

### Ingestar un documento

```
> /ingest mi_documento.pdf
```

### Hacer una pregunta

```
> dime como usar structs en rust
```

### Activar debug para ver RAG

```
> /debug
> hazme una calculadora en rust
```

## API OpenAI-compatible

El servidor HTTP corre en el puerto configurado (default: 8080).

```bash
# POST request
curl -X POST http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"messages":[{"role":"user","content":"Hola"}]}'
```

## Memoria y persistencia

- Conversaciones se guardan en `memvid_data/`
- Cada 5 interacciones se persisten a disco
- El conocimiento indexado se guarda en `knowledge_index.jsonl`

## Variables de entorno para runtime

| Variable | Descripción |
|----------|-------------|
| `MODEL_PATH` | Override ruta del modelo |
| `MODEL_NAME` | Override nombre del modelo |