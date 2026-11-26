# Uso

## Ejecución

```bash
cd memvid-agent-core
cargo run              # desarrollo
cargo run --release    # optimizado
```

Esto inicia el REPL interactivo (`>`). Los comandos empiezan por `/`; cualquier otro texto se envía al modelo.

## Comandos del REPL

### Conversación y estado

| Comando | Descripción |
|---|---|
| `<mensaje>` | Chatea con el agente (historial de sesión + RAG) |
| `/help` | Ayuda completa |
| `/exit`, `/quit` | Salir (Ctrl+C también, con apagado ordenado) |
| `/stats` | Modelo, interacciones, memoria y archivos cargados |
| `/history`, `/ls` | Historial de conversaciones persistidas |
| `/config` | Configuración efectiva |
| `/debug` | Activa/desactiva debug y muestra el RAG de la última consulta |

### Modelos y generación

| Comando | Descripción |
|---|---|
| `/models` | Catálogo de 21 modelos con tamaño, descripción y activo |
| `/model` | Modelo actual |
| `/model current` | Igual que `/model` |
| `/model <id>` | Descarga (si falta) y cambia de modelo |
| `/kv` | Tipos de KV-cache actuales |
| `/kv <k> <v>` | Cambia K/V y recarga el contexto (ej. `/kv f16 turbo3`) |

Tipos de KV válidos: `f32`, `f16`, `bf16`, `q8_0`, `q4_0`, `turbo2`, `turbo3`, `turbo4` (y sus alias `turbo2_0`, `turbo3_0`, `turbo4_0`). Los `turbo*` activan flash-attn automáticamente. Se recomienda `K` con precisión igual o mayor que `V`.

### Conocimiento e ingesta

| Comando | Descripción |
|---|---|
| `/search <query> [from:<source>]` | Busca en el índice (top 5), filtro opcional por fuente |
| `/load <file>` | Carga texto en la sesión (no indexa) |
| `/ingest <file>` | Extrae e indexa PDF/EPUB/MD/HTML/TXT |
| `/ingest-pdf <file>` | Fuerza extracción de PDF/EPUB |
| `/fetch <url>` | Descarga, extrae texto e indexa (deduplicado) |
| `/fetch-md <url>` | Descarga y muestra como Markdown |
| `/batch <file>` | Procesa un archivo con una URL por línea (`#` comenta) |
| `/reindex` | Reconstruye el índice desde los `.mv2` |
| `/languages` | Lenguajes con documentación disponible |
| `/languages-installed` | Lenguajes instalados |
| `/learn <lang>` | Descarga hasta 10 recursos y los indexa |
| `/unlearn <lang>` | Elimina el conocimiento del lenguaje |
| `/books` | Lenguajes de free-programming-books |
| `/download-books <lang> [limit]` | Indexa metadatos (título, formato, URL) de hasta `limit` recursos (10 por defecto) |
| `/finetune <lang>` | Fine-tune del modelo con los docs del lenguaje |

### Feeds y cola

| Comando | Descripción |
|---|---|
| `/feed <url>` | Procesa un feed RSS/Atom (hasta 20 entradas) |
| `/queue` | Estado de la cola |
| `/queue-add <url>` | Añade URL a la cola persistente |
| `/queue-process` | Procesa las URLs pendientes |

### API

| Comando | Descripción |
|---|---|
| `/token` | Habilita la API, genera token, arranca el servidor y ofrece configurar clientes |

## Ejemplos

### Ingesta y consulta

```
> /ingest apuntes.pdf
> /search ownership
> explica el concepto de ownership según mis apuntes
```

### Aprender un lenguaje

```
> /languages
> /learn rust
> /languages-installed
> /unlearn rust
```

`/learn` indexa documentación de programación y, al terminar, ofrece lanzar el fine-tune sobre lo aprendido.

### Ingesta web

```
> /fetch https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html
> /fetch-md https://example.com/articulo
> /batch urls.txt
> /feed https://blog.rust-lang.org/feed.xml
> /queue-add https://example.com/pendiente
> /queue-process
```

### Debug del RAG

```
> /debug
> hazme una calculadora en rust
```

Cada respuesta muestra fuente, score y tokens de las entradas recuperadas.

### Ajustar la KV-cache

```
> /kv
> /kv f16 turbo3
```

Reduce el uso de memoria de la caché de valores sin tocar la de claves.

## API compatible con OpenAI

Se arranca con `/token`. Por defecto escucha en `api.host` (**127.0.0.1**) y el puerto **8787**. Para exponerlo en la red cambia `api.host` a `0.0.0.0` y usa TLS o un reverse proxy.

### Endpoints

| Método | Ruta | Auth | Descripción |
|---|---|---|---|
| `GET` | `/health` | No | Estado, modelo y uptime |
| `GET` | `/v1/models` | Bearer | Modelo activo |
| `POST` | `/v1/chat/completions` | Bearer | Chat completion (soporta `stream: true`) |
| `GET`/`POST` | `/token` | Bearer | Token configurado |

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

Notas:

- Un hilo por conexión (máximo 64, `ATEN_API_MAX_CONNS`); la inferencia se serializa con un mutex.
- SSE emite `role`, `content` y `finish` (el texto se genera completo antes de enviarse).
- Las peticiones de la API no se guardan en la sesión persistente.
- `usage.prompt_tokens` es real; `completion_tokens` se estima (chars/4).
- Límites: cuerpo 10 MB, cabeceras 64 KB, timeout 120 s.

## Configurar OpenCode

`/token` **no escribe archivos**: muestra la URL base y el token para que los
configures manualmente.

```
URL base: http://127.0.0.1:8787/v1
Token: <token>
```

En OpenCode, crea un proveedor OpenAI-compatible con esa `baseURL` y `apiKey`
según la documentación de tu versión. No reutilices un `opencode.json` generado
por versiones anteriores de aten-ia; revísalo o elimínalo.

## Configurar VS Code (Continue)

`/token` muestra este bloque; adáptalo al formato de configuración de Continue:

```json
{
  "openai-compatible": {
    "api_base": "http://localhost:8787/v1",
    "api_key": "<token>",
    "title": "aten-ia"
  }
}
```

## Memoria y persistencia

- Directorio por defecto: `~/.aten-ia` (configurable en `config.json` con `data_dir`).
- La sesión se vuelca a `.mv2` cada **5 interacciones** y al salir.
- El conocimiento se indexa en `knowledge_index.jsonl` y en segmentos `knowledge/*.mv2`.
- Todas las escrituras son atómicas.
- Un `.lock` impide ejecutar dos instancias sobre el mismo directorio.

## Variables de entorno en runtime

| Variable | Descripción |
|---|---|
| `MODEL_PATH` | Ruta del modelo GGUF |
| `MODEL_NAME` | Nombre visible del modelo |
| `MODEL_CTX` | Tamaño de contexto |
| `MODEL_URL` | URL de descarga alternativa |
| `LLAMA_FINETUNE_BIN` | Binario `llama-finetune` |

El resto de la configuración vive en `config.json`; consulta el README del repositorio para la tabla completa: https://github.com/lordpietre/aten-ia/blob/main/README.md
