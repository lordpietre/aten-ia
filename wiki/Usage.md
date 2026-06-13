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
# Activar API con token
> /token
API server starting on http://0.0.0.0:8080
Token: tu-token-aqui
Use: Authorization: Bearer tu-token-aqui
```

## Configurar OpenCode para usar aten-ia como backend

### 1. Generar token en aten-ia

```bash
cargo run
> /token
# Anotar el token que aparece
```

### 2. Crear archivo de configuración de OpenCode

Crea `~/.config/opencode/opencode.json` (Linux) o `%APPDATA%\opencode\opencode.json` (Windows):

```json
{
  "instructions": "Eres un asistente de IA. Usa el backend local aten-ia para consultas.",
  "llm": {
    "provider": "openai",
    "model": "local",
    "api_base": "http://localhost:8080/v1",
    "api_key": "tu-token-aqui"
  }
}
```

### 3. Usar

Ejecuta opencode y configurará automáticamente el backend hacia aten-ia.

## Memoria y persistencia

- Conversaciones se guardan en `memvid_data/`
- Cada 5 interacciones se persisten a disco
- El conocimiento indexado se guarda en `knowledge_index.jsonl`

## Variables de entorno para runtime

| Variable | Descripción |
|----------|-------------|
| `MODEL_PATH` | Override ruta del modelo |
| `MODEL_NAME` | Override nombre del modelo |