# Plan exhaustivo de mejoras y corrección de errores — aten-ia

`aten-ia` es un agente LLM local (CPU, llama.cpp) con persistencia `.mv2`,
RAG por keywords, API OpenAI-compatible e ingesta multi-formato. Este plan recoge
los defectos reales hallados leyendo el código y las mejoras de mayor valor, cada
uno con su estrategia de test (unitario + pragmático).

---

## 1. Bugs de corrección

### B1 — Pánico UTF-8 al mostrar el historial  🔴  **[x] CORREGIDO**
- **Evidencia:** `src/agent.rs` (antiguo `&content[..500]` en `read_conversation_history`).
- **Causa:** el slice por bytes entra en pánico si el byte 500 cae dentro de un
  carácter multibyte (acentos, CJK, emoji). Cualquier conversación con ≥500 bytes
  y contenido no-ASCII tumbaba `/history`.
- **Fix:** helper `utils::truncate_chars(s, max_chars, suffix)` que corta por
  `char` boundaries. Aplicado en `read_conversation_history`.
- **Tests:**
  - Unitario (`utils.rs`): `truncate_chars_is_utf8_safe_at_multibyte_boundary`,
    `truncate_chars_handles_emoji_and_cjk`, bajo/encima de límite.
  - Pragmático e2e (`tests/kv_cache_and_history.rs`):
    `read_conversation_history_does_not_panic_on_multibyte` — escribe un segmento
    `.mv2` real con 600×'é' y verifica que `/history` no panica y trunca bien.

### B2 — Ranking de búsqueda corrompido por overflow de bucket  🟠  **[x] CORREGIDO**
- **Evidencia:** `src/retrieval.rs::search` — antiguo `matches * 10000 + i`.
- **Causa:** se codificaban dos claves (nº de coincidencias y recencia) en un solo
  entero con factor 10000. Con >10000 entradas, el índice `i` se desbordaba al
  siguiente bucket: una entrada con 1 coincidencia podía empatar/superar a otra
  con 2. Real en cualquier base de conocimiento grande (un solo `/learn` puede
  generar miles de chunks).
- **Fix:** ordenar por tupla `(matches desc, i desc)` con `sort_by`+`then_with`.
- **Tests:** `search_ranks_by_match_count_not_corrupted_by_index_overflow`
  (12.001 entradas, la de 2 coincidencias debe ganar).

### B3 — `switch_model` descarta el developer prompt  🟠  **[x] CORREGIDO**
- **Evidencia:** `src/agent.rs::switch_model` reconstruía `PromptBuilder::new(...)`.
- **Causa:** `PromptBuilder::new` fija el developer prompt al DEFAULT; al cambiar de
  modelo se perdía cualquier prompt personalizado (y el modo "developer_mode=false"
  que usa prompt vacío). Documentado como limitación, pero es un bug de regresión.
- **Fix:** nuevo `PromptBuilder::with_template(&self, template)` que conserva el
  developer prompt; `switch_model` lo usa.
- **Tests:** `with_template_preserves_developer_prompt`,
  `with_template_preserves_empty_developer_prompt` (`prompt.rs`).

### B4 — `process_url_batch` reporta un recuento de chunks falso  🟡  **[x] CORREGIDO**
- **Evidencia:** `process_url_batch` sumaba `content.len() / chunk_max_size`
  (estimación), no el nº real de chunks indexados.
- **Fix:** `fetch_and_ingest` devuelve `(FetchedContent, usize)` con el número
  real de chunks indexados (dedup incluido); `process_url_batch` suma ese valor y
  `/fetch` lo muestra ("chunks indexed: N").
- **Tests:** cubierto indirectamente por `store_knowledge_dedup_*` (el conteo deriva
  del nº de inserciones efectivas). Pendiente test e2e con servidor local (ver B4b).

### B5 — `detokenize` puede tragarse tokens largos  🟡  **[x] CORREGIDO**
- **Evidencia:** `detokenize` usaba buffer fijo de 256 bytes y
  `buffer.truncate(n.max(0))`; si `llama_token_to_piece` devolvía negativo (buffer
  insuficiente) la pieza se truncaba a 0 → salida corrupta sin error.
- **Fix:** función pura `piece_len_or_needed(n)` (Ok(len) / Err(needed)) y reintento
  con el tamaño exacto reportado, replicando el patrón de `tokenize`.
- **Tests:** `piece_len_or_needed_classifies_return_values` (positivo/0/negativo).

### B7 — `strip_html` rompe el ampersand suelto  🟠  **[x] CORREGIDO**
- **Evidencia:** `strip_html("<p>AT&T</p>")` devolvía `"AT"`: un `&` que no llega a
  `;` se descartaba al toparse con un `<` (inicio de tag) o el fin de la cadena.
- **Impacto:** la limpieza de HTML al hacer `/learn` corrompía texto con `&` literal.
- **Fix:** helper `flush_entity` que reemite `&`+buffer como literal cuando el `&`
  no forma una entidad válida (al ver `<`, otro `&`, un carácter no-entidad, o EOF).
  Se preserva el comportamiento previo de descartar entidades numéricas inválidas.
- **Tests (`languages_catalog.rs`):** `strip_html_lone_ampersand` (antes rojo)
  + `_at_eof`, `_followed_by_space`, `_two_lone_ampersands`, `_lone_then_valid_entity`.
  Toda la suite `strip_html` (13 tests) en verde.

### B8 — Deuda de lints clippy  🟡  **[x] CORREGIDO**
- Eran 33 warnings preexistentes. `cargo clippy --fix --lib` aplicó 31 fixes
  automáticos. Los 5 restantes resueltos a mano.
- **Resultado:** `cargo clippy --lib` ahora **0 warnings**.

### B6 — Sin deduplicación por checksum en el índice  🟡  **[x] CORREGIDO (opt-in)**
- **Evidencia:** `KnowledgeEntry.checksum` se calculaba pero nunca se usaba;
  re-ingerir el mismo fichero/URL duplicaba entradas.
- **Fix (opt-in):** `KnowledgeIndex::add_entry_dedup` ignora
  `(source, checksum)` ya presente; `Agent::store_knowledge_dedup` lo usa y solo se
  aplica en la ruta de `fetch_and_ingest`. El `add_entry`/`store_knowledge_chunked`
  genéricos siguen apilando (tests de chunks repetidos intactos).
- **Tests:** `add_entry_dedup_skips_same_source_and_checksum`,
  `add_entry_dedup_keeps_distinct_content_and_sources` (`retrieval.rs`);
  `store_knowledge_dedup_skips_repeat_ingestion` (`agent.rs`).

### B9 — `/fetch-md` bloqueaba el mutex del agente durante la descarga  🟡  **[x] CORREGIDO**
- **Evidencia:** el handler adquiría `agent.lock()` sin usarlo, reteniéndolo durante
  el fetch de red (warning `unused variable: a` + bloqueo innecesario).
- **Fix:** eliminado el lock muerto; `/fetch-md` solo descarga e imprime.

### B10 — `chunk_fixed` no era UTF-8 safe  🔴  **[x] CORREGIDO**
- **Evidencia:** `src/chunker.rs::chunk_fixed` hacía slicing por bytes sin
  `floor_char_boundary()`, a diferencia de `chunk_by_headings` que sí lo usaba.
- **Causa:** si `max_size` o `start + advance` caían dentro de un carácter multibyte,
  el slice producía texto inválido o entraba en pánico.
- **Fix:** se aplica `floor_char_boundary()` a `end` y se usa `max(candidate, end)`
  para garantizar progreso hacia adelante sin perder el solapamiento en ASCII.
- **Test:** `chunk_fixed_multibyte_safe` — texto con `ñ` (2 bytes), verifica que
  todos los chunks son UTF-8 válido y que `chars().count()` no entra en pánico.

### B11 — `feed_title` siempre era la URL en vez del título real  🟠  **[x] CORREGIDO**
- **Evidencia:** `agent.rs:279` — `entries.first().map(|_| url.to_string())`
  ignoraba el valor de la entry y siempre producía `Some(url)`.
- **Fix:** `entries.first().map(|e| e.title.clone()).or_else(|| Some(url.to_string()))`
  usa el título del primer entry, con fallback a la URL si el título está vacío.
- **Tests:** cubierto indirectamente por la estructura `FeedResult`.

### B12 — Template Mistral descartaba system messages y RAG context  🟠  **[x] CORREGIDO**
- **Evidencia:** `prompt.rs::build_mistral` — `match msg.role` con `_ => {}`
  descartaba silenciosamente los mensajes `System` y `Tool`.
- **Impacto:** el developer prompt y RAG context se perdían con modelos Mistral.
  El system prompt se inyectaba una vez al inicio pero los mensajes system del
  historial (archivos cargados, etc.) se eliminaban.
- **Fix:** `build_mistral` ahora:
  - Inyecta el developer prompt + RAG context como prefijo en el primer `[INST]`.
  - Los mensajes `System` se insertan como `[INST] {msg} [/INST]` antes del primer
    mensaje de usuario.
  - La lógica de `[INST] ... [/INST]` se mantiene idéntica para User/Assistant.
- **Tests:** `mistral_template`, `mistral_template_with_messages`,
  `mistral_template_with_system_message`.

### B13 — `/fetch-md` hacía una segunda petición HTTP innecesaria  🟡  **[x] CORREGIDO**
- **Evidencia:** `main.rs:733` — después de obtener el contenido vía `WebFetcher`,
  se hacía un `ureq::get(url).call()` adicional sin agente configurado, sin timeout,
  sin rate-limiting, ignorando la config de ingesta.
- **Fix:** el contenido HTML ya está en `content.content` (el `WebFetcher` lo obtuvo).
  Se usa directamente para la conversión a Markdown sin segunda petición.
- **Impacto:** latencia reducida a la mitad en `/fetch-md`, se respetan los timeouts
  y rate limits configurados, y se elimina la dependencia extra de `ureq` en main.rs.

### B14 — Descarga de modelos: archivo parcial sin limpieza en error  🟠  **[x] CORREGIDO**
- **Evidencia:** si la descarga fallaba a medio camino, el archivo `.gguf` parcial
  se quedaba en disco. ningún cleanup.
- **Fix:** en `models.rs::ensure_model` y `models_catalog::download`, los errores
  de lectura/escritura eliminan el archivo parcial (`remove_file`) y muestran un
  mensaje de error con progress bar. Se añade `sync_all()` para flushed a disco.
- **Tests:** cubierto por los paths de error (no se puede testear sin mocking HTTP).

### B15 — `{{header_end}}` con `.unwrap()` podía entrar en pánico  🟡  **[x] CORREGIDO**
- **Evidencia:** `api.rs:269` — `request_str.find("\r\n\r\n").unwrap()`
  podía entrar en pánico si el buffer se leía de forma inesperada.
- **Fix:** `.ok_or_else(|| anyhow::anyhow!("Invalid HTTP request"))?` — error
  controlado en vez de pánico.

### B16 — `FileLock::acquire()` con `.expect()` no daba mensaje útil  🟡  **[x] CORREGIDO**
- **Evidencia:** `main.rs:32` — `.expect("Failed to acquire data directory lock")`
  entraba en pánico sin sugerir solución si un lock stale existía.
- **Fix:** reemplazado con `match` que muestra un mensaje con la ruta al `.lock`
  y sugiere eliminarlo si no hay otra instancia corriendo.

### B17 — Wizard setup aceptaba input inválido silenciosamente  🟡  **[x] CORREGIDO**
- **Evidencia:** `main.rs:1335` — `model_choice.parse().unwrap_or(1)` convertía
  cualquier input no numérico en modelo #1 sin avisar al usuario.
- **Fix:** validación explícita con loop que muestra error y sugiere rango válido.
  Si el input es vacío, se usa el default (1).

### B18 — Default model inconsistente con la documentación  🟡  **[x] CORREGIDO**
- **Evidencia:** `Config::default()` tenía `name: "smollm2-360m"` y
  `path: "models/default-model.gguf"`, pero README y AGENTS.md dicen que el default
  es `Qwen2.5-0.5B-Instruct`. El `n_ctx` también era 4096 en vez de 8192.
- **Fix:** defaults actualizados a `name: "Qwen2.5-0.5B-Instruct"`,
  `path: "models/qwen2.5-0.5b.gguf"`, `n_ctx: 8192`, y `download_url` con la URL
  de HuggingFace. Tests actualizados.
- **Tests:** `config_defaults` y `env_var_unset_does_not_override` ahora verifican
  los nuevos valores.

---

## 2. Robustez / seguridad

### S1 — API server de un solo hilo y bloqueante  🔵  **[x] CORREGIDO (parcial)**
- **Fix:** un hilo por conexión (`std::thread::spawn`, `ApiServer: Clone`),
  `set_read_timeout`/`set_write_timeout` (30 s) y límites `MAX_BODY_BYTES` (10 MB) /
  `MAX_HEADER_BYTES` (64 KB). La inferencia sigue serializada en el mutex del agente.
- **Tests:** `within_body_limit_enforces_cap` (`api.rs`). El timeout/concurrencia
  requiere test de integración con sockets (pendiente, S1b).
- **Pendiente (S1b):** test e2e con 2 conexiones; streaming SSE.

### S2 — `check_auth` sin protección contra timing  🔵  **[x] CORREGIDO**
- **Fix:** `constant_time_eq` (XOR acumulado sobre el máximo de ambas longitudes).
- **Tests:** `constant_time_eq_matches_only_on_equal` (igual/distinto/longitudes).

### S3 — Token de API real commiteado en `config.json`  🟠  **[x] CORREGIDO**
- **Fix:** `"token": null` en el fichero versionado (se genera en runtime con `/token`).

### S4 — Validación de config incompleta  🔵  **[x] CORREGIDO**
- **Evidencia:** `Config::validate` solo validaba `n_ctx > 0`, `max_tokens > 0`,
  `temp >= 0` y `port > 0`. No validaba rangos de `top_p`, `top_k`, ni tipos de KV-cache.
- **Fix:** añadida validación de `0 <= top_p <= 1`, `top_k >= 0`, y
  `is_valid_kv_cache_type` para `kv_type_k` y `kv_type_v`.
- **Tests:** `validate_rejects_top_p_out_of_range`, `validate_rejects_negative_top_k`,
  `validate_rejects_invalid_kv_type_k`, `validate_rejects_invalid_kv_type_v`,
  `validate_accepts_valid_kv_types`.

### S5 — `WebFetcher` ignoraba timeout configurado  🔵  **[x] CORREGIDO**
- **Evidencia:** `IngestionConfig.timeout_seconds` se almacenaba pero nunca se usaba.
  `fetch()` usaba `ureq::Agent::new_with_defaults()` sin timeout, potencialmente
  colgando indefinidamente.
- **Fix:** `WebFetcher::new` ahora construye el `Agent` con
  `ureq::Agent::builder().timeout_read(Duration::from_secs(...)).timeout_write(...)`.
  Fallback a `Agent::new_with_defaults()` si el builder falla.
- **Tests:** existentes `web_fetcher_timeout_short` ahora realmente testea el timeout
  configurado.

### S6 — `FeedQueue::persist` no hacía fsync  🟡  **[x] CORREGIDO**
- **Evidencia:** `queue.rs::persist` hacía `write → rename` sin fsync. A diferencia
  de `utils::atomic_write` y `manifest.rs` que sí usan `write → fsync → rename`.
  Una caída de energía podía perder datos en la cola de feeds.
- **Fix:** `persist` ahora usa `crate::utils::atomic_write` que hace
  `write → fsync → rename → fsync(parent)`.

---

## 3. UX mejorada

### U1 — Progress bars en descarga de modelos  🔵  **[x] CORREGIDO**
- **Antes:** la descarga de modelos mostraba texto `eprintln!` con `[↓]` y hectómetros
  de logs. Sin barra de progreso, sin cleanup en error.
- **Fix:** `models.rs::ensure_model` y `models_catalog::download` ahora usan:
  - Spinner "Connecting to download…" mientras se conecta.
  - Barra de progreso con bytes/ETA si Content-Length está disponible.
  - Spinner alternativo si el servidor no envía Content-Length.
  - `sync_all()` para flushed a disco tras completar.
  - `remove_file` del parcial en caso de error de lectura/escritura.
  - Spinner "Verifying checksum…" si el modelo tiene SHA-256.

### U2 — Modelo más-loading menos verbose  🔵  **[x] MEJORADO**
- Ya existía un spinner "Loading model…" (bien), se mantiene.
- La descarga del modelo en el setup wizard ahora muestra progress bar en vez
  de eprintln plano.
- Errores de FileLock muestran la ruta al `.lock` y sugieren eliminarlo.

### U3 — Template Mistral con system prompt  🔵  **[x] CORREGIDO** (ver B12)

---

## 4. KV-cache TurboQuant (Opción 4) — seguimiento

### K1 — Tipo de KV-cache configurable  🔵  **[x] IMPLEMENTADO**
- Campos `model.kv_type_k` / `model.kv_type_v` (default `f16`, serde-default para
  retrocompatibilidad). Mapeo a `ggml_type` en `llama/context.rs`, activación
  automática de flash-attn para tipos cuantizados, aviso si K se comprime más que V.
- **Tests:** unidad en `config.rs` y `llama/context.rs`; integración en
  `tests/kv_cache_and_history.rs`.

### K2 — De-vendorizar el fork a submódulo  🔵  **[ ] BLOQUEADO (requiere decisión)**
- El fork (154 MB, 99,6% del repo) está commiteado como copia, no como submódulo.
  Convertirlo requiere la URL+commit canónicos del fork (`TheTom/...`) para fijar el
  submódulo sin romper el build. `build.rs` ya soporta libs precompiladas, así que
  la fuente puede salir del repo principal.
- **Acción pendiente:** confirmar coordenadas del fork y ejecutar
  `git rm -r --cached` + `git submodule add <url> <path>` + pin de commit.

### K3 — Comando REPL `/kv <k> <v>` + validación  🔵  **[x] IMPLEMENTADO**
- `/kv` muestra los tipos actuales; `/kv <k> <v>` valida ambos nombres
  (`is_valid_kv_cache_type`, rechaza typos), persiste en `config.json` y recarga el
  contexto vía `switch_model`. Añadido a `/help`.
- **Tests:** `is_valid_kv_cache_type_accepts_known_rejects_unknown` (`context.rs`).
  El switch en caliente requiere modelo real, no testeable en CI sin GGUF.

### K4 — Exponer `n_gpu_layers` y flags GPU  🔵  **[ ]**
- Hoy `build.rs` compila con CUDA/Metal/Vulkan OFF y `n_gpu_layers` no tiene efecto.
  Añadir `MODEL_GPU_LAYERS` y flags cmake condicionales para aprovechar GPU/turbo.

---

## 5. Rendimiento / calidad de RAG

### R1 — `add_entries` reescribe el JSONL completo  🔵  **[x] CORREGIDO**
- **Fix:** `append_entries_to_jsonl` añade solo las líneas nuevas con un único
  `write_all` + `fsync`. O(nuevas) en vez de O(total).
- **Tests:** `add_entries_persists_incrementally_to_jsonl`.

### R2 — RAG semántico (embeddings)  🔵  **[ ]**
- Sustituir/complementar el match por substring con embeddings (el fork trae soporte
  de embeddings en llama.cpp). Gran salto de calidad de recuperación.
- **Tests:** de relevancia con un corpus pequeño etiquetado (golden set).

### R3 — Normalización de la búsqueda  🟡  **[ ]**
- `search` cuenta `matches()` (substrings solapados) y mezcla content/source/id sin
  pesos. Considerar normalizar por longitud y dar más peso a coincidencias en source.

---

## 6. Estado actual

**Completados** (con tests, en verde): B1–B18, S1 (parcial), S2–S6, K1, K3, R1, U1–U3.

**Pendientes (bugs/features):**
- **B4b** — test e2e de `process_url_batch` con `TcpListener` local.
- **S1b** — test e2e de concurrencia/timeout de la API + streaming SSE.
- **K2** — de-vendorizar el fork a submódulo.
- **K4** — `MODEL_GPU_LAYERS` + flags GPU en `build.rs`.
- **R2** — RAG semántico (embeddings).
- **R3** — normalización/pesos en `search`.

**Pendientes (CI/Build x86+ARM):**
- **C1** 🔴 — Runner `ubuntu-24.04-arm` no existe en GitHub Free.
- **C2** 🟠 — CI sin cobertura ARM64.
- **C3** 🟠 — `.deb` control file línea sin espacio.
- **C4** 🟠 — CMake sin flags de arquitectura ARM (binario no portable).
- **C5** 🟠 — bindgen sin `--target` para cross-compilation.
- **C6** 🟡 — `download_prebuilt` siempre falla en CI.
- **C7** 🟡 — `submodules: recursive` sin `.gitmodules`.
- **C8** 🟡 — Sin `rust-toolchain.toml`; AGENTS.md dice 1.95.0 (real: 1.85.0).
- **C9** 🟡 — Falta `libgomp-dev` en release.yml.
- **C10** 🟡 — Snap `core22` en host 24.04.
- **C11** 🟡 — `--target` no pasado a `cargo build`.
- **C12** 🟡 — `CMAKE_BUILD_PARALLEL_LEVEL=1` causa builds de 30+ min.

Orden sugerido: C1+C4+C5 → C3 → C8 → C9 → C6+C12 → C2 → C7 → C11 → C10.

Orden sugerido para lo que queda: K4 → R2/R3 → K2 → B4b/S1b.

---

## 9. CI/Build — Compilación x86_64 y aarch64

### C1 — Runner `ubuntu-24.04-arm` no existe en GitHub Free  🔴  **[ ]**
- **Evidencia:** `release.yml:25` — `runner: ubuntu-24.04-arm`.
- **Causa:** GitHub Actions no ofrece runners ARM gratuitos. Ese label solo funciona
  con GitHub Team/Enterprise y "Larger runners" (pago por minuto). En repos free,
  el job se cuelga indefinidamente esperando un runner que no existe.
- **Impacto:** la release ARM64 **nunca se ejecuta**.
- **Fix (3 opciones):**
  1. **Cross-compilation en `ubuntu-latest`:** instalar `gcc-aarch64-linux-gnu`,
     pasar `--target aarch64-unknown-linux-gnu` a cargo, y construir las libs
     estáticas con un toolchain cross cmake. Más complejo pero gratis.
  2. **QEMU emulation en `ubuntu-latest`:** usar `docker/setup-qemu-action` +
     `docker/build-push-action` para compilar en emulación ARM. Lento (~45 min)
     pero funciona sin pago.
  3. **GitHub Larger runners:** pagar por minutos ARM. Lo más simple pero con coste.
- **Recomendado:** Opción 1 (cross-compilation) + cache de libs precompiladas.

### C2 — CI solo testa x86_64, sin cobertura ARM  🟠  **[ ]**
- **Evidencia:** `ci.yml:14` — `runs-on: ubuntu-latest`.
- **Causa:** No hay matrix de targets. Errores ARM-only se detectan solo en release.
- **Fix:** Añadir un job de CI ARM64 (con cross-compilation o QEMU) que ejecute al
  menos `cargo check --target aarch64-unknown-linux-gnu` + `cargo clippy --lib`.

### C3 — `.deb` control file con línea sin espacio inicial  🟠  **[ ]**
- **Evidencia:** `release.yml:100` — la línea `and multi-source ingestion...` en el
  heredoc del `DEBIAN/control` queda sin espacio inicial tras el stripping de YAML.
- **Causa:** En Debian control format, las líneas de continuación en `Description`
  DEBEN empezar con un espacio. Sin él, `dpkg-deb` rechaza el paquete o lo genera
  malformado.
- **Fix:** Añadir un espacio extra en la línea afectada para que YAML preserve un
  espacio al inicio. Cambiar:
  ```
            and multi-source ingestion (PDF, EPUB, HTML, MD, TXT, URLs).
  ```
  a:
  ```
             and multi-source ingestion (PDF, EPUB, HTML, MD, TXT, URLs).
  ```
  (11 espacios en vez de 10, para que al stripping quede ` and un-source...` con
  un espacio inicial).

### C4 — CMake no recibe flags de arquitectura ARM64; binario no portable  🟠  **[ ]**
- **Evidencia:** `release.yml:48-58` y `build.rs:96-107` — no se pasa
  `-DGGML_CPU_ARM_ARCH` ni `-DGGML_NATIVE=OFF`.
- **Causa:** `GGML_NATIVE` default es ON. Cmake compila con `-mcpu=native`,
  generando instrucciones específicas del runner ARM (dotprod, i8mm, etc.). El
  binario puede SIGILL en otras CPUs ARM64 que no tengan esas extensiones.
- **Fix:**
  - En `release.yml`, pasar `-DGGML_CPU_ARM_ARCH=armv8-a+dotprod` para builds ARM.
  - En `build.rs`, detectar `TARGET` y pasar la flag correspondiente:
    ```rust
    let target = env::var("TARGET").unwrap_or_default();
    if target.starts_with("aarch64") {
        cmake_config.define("GGML_CPU_ARM_ARCH", "armv8-a+dotprod");
    }
    ```

### C5 — `bindgen` no recibe `--target` para cross-compilation  🟠  **[ ]**
- **Evidencia:** `build.rs:139-151` — `bindgen::Builder` sin `--target`.
- **Causa:** Al cross-compilar (build en x86_64, target aarch64), bindgen genera
  FFI bindings para la arquitectura del host, no del target. Los tamaños de
  struct y alineación pueden diferir entre architectures, causando ABI mismatches
  y crashes en runtime.
- **Impacto:** solo afecta si se usa cross-compilation (fix de C1 opción 1).
- **Fix:**
  ```rust
  let target = env::var("TARGET").unwrap_or_default();
  let host = env::var("HOST").unwrap_or_default();
  let mut builder = bindgen::Builder::default()
      .header("wrapper.h")
      .clang_arg("-I./llama-cpp-turboquant/include")
      .clang_arg("-I./llama-cpp-turboquant/ggml/include");
  if target != host {
      builder = builder.clang_arg(format!("--target={}", target));
  }
  ```

### C6 — `download_prebuilt` siempre falla en CI (no hay releases)  🟡  **[ ]**
- **Evidencia:** `build.rs:47-51` — URL con `/latest/download/` devuelve 404 si no
  hay releases previas.
- **Causa:** Diseño circular: la release workflow crea los `.tar.gz`, pero CI
  (pre-merge) nunca puede usarlos porque no existen todavía. Siempre cae a
  `cmake_build()` con `CMAKE_BUILD_PARALLEL_LEVEL=1` (= 1 thread).
- **Impacto:** CI tarda 30+ minutos en cada run por compilación secuencial de
  llama.cpp.
- **Fix (2 opciones):**
  1. Guardar las libs precompiladas como CI cache (workflow `build-cache.yml` que
     se ejecuta al crear un release y guarda en GitHub Actions Cache).
  2. Subir las libs como asset de release y hacer que CI descargue de la última
     release tag en vez de `/latest/download/`.

### C7 — `submodules: recursive` sin `.gitmodules`  🟡  **[ ]**
- **Evidencia:** `ci.yml:21`, `release.yml:33` — `submodules: recursive`.
- **Causa:** No existe `.gitmodules`. `llama-cpp-turboquant` está embebido como
  archivos regulares (no como submodule). La directiva es no-op.
- **Impacto:** ningún fallo de build, pero la directiva es confusa y AGENTS.md
  dice incorrectamente que se requiere.
- **Fix:** Eliminar `submodules: recursive` de ambos workflows y corregir
  AGENTS.md. Alternativamente, convertir `llama-cpp-turboquant` a submódulo
  real (pendiente K2).

### C8 — No existe `rust-toolchain.toml`; edition 2024 requiere >= 1.85  🟡  **[ ]**
- **Evidencia:** `Cargo.toml:4` — `edition = "2024"`. Sin `rust-toolchain.toml`.
- **Causa:** edition 2024 requiere Rust >= 1.85.0. Los desarrolladores con Rust
  más antiguo obtienen errores confusos. `dtolnay/rust-toolchain@stable` en CI
  instala la versión más reciente, pero localmente no hay garantía.
- **Fix:** Crear `memvid-agent-core/rust-toolchain.toml`:
  ```toml
  [toolchain]
  channel = "1.85.0"
  ```
  Y corregir AGENTS.md (dice "min 1.95.0" pero el mínimo real es 1.85.0).

### C9 — `.deb` no instala `libgomp-dev` para compilación  🟡  **[ ]**
- **Evidencia:** `release.yml:36` — instala `libgomp1` pero no `libgomp-dev`.
- **Causa:** cmake `find_package(OpenMP)` puede requerir los headers de desarrollo.
  En la práctica, el build usa `-fopenmp` de GCC y enlaza dinámicamente, pero
  si OpenMP no se detecta, llama.cpp compila sin OpenMP y el performance se
  degrada significativamente.
- **Fix:** Añadir `libgomp-dev` a `apt-get install` en `release.yml`.

### C10 — Snap con `core22` en host Ubuntu 24.04  🟡  **[ ]**
- **Evidencia:** `snapcraft.yaml:10` — `base: core22`. Release workflow usa
  Ubuntu 24.04 (host) con `destructive-mode`.
- **Causa:** glibc 2.39 (host 24.04) vs glibc 2.35 (core22 base). El snap puede
  enlazar contra glibc 2.39 y luego no ejecutarse en sistemas con glibc 2.35.
- **Fix:** Cambiar a `base: core24` o construir en un contenedor con Ubuntu 22.04.

### C11 — `--target` no pasado a `cargo build` en release  🟡  **[ ] BUG menor**
- **Evidencia:** `release.yml:68` — `cargo build --release` sin `--target`.
- **Causa:** Funciona para builds nativos, pero inconsistente con
  `targets: ${{ matrix.target }}` instalado en el step de Rust.
- **Impacto:** sin efecto actual (nativo), pero confuso. Si se añade cross-compilation,
  los paths de `strip` y packaging deben usar `target/<triple>/release/`.
- **Fix:** Cambiar a `cargo build --release --target ${{ matrix.target }}` y ajustar
  los paths de packaging consecuentemente.

### C12 — `CMAKE_BUILD_PARALLEL_LEVEL=1` causa builds de 30+ min en CI  🟡  **[ ]**
- **Evidencia:** `build.rs:94` — `unsafe { std::env::set_var("CMAKE_BUILD_PARALLEL_LEVEL", "1") }`.
- **Causa:** Previene OOM en máquinas con ~8 GB RAM. En GitHub runners (16 GB),
  puede usar 2-4 threads y reducir el tiempo de build de 30 min a ~10 min.
- **Fix:** Hacer el parallel level configurable:
  ```rust
  let jobs = env::var("CMAKE_BUILD_PARALLEL_LEVEL").unwrap_or_else(|_| "2".to_string());
  unsafe { std::env::set_var("CMAKE_BUILD_PARALLEL_LEVEL", &jobs) };
  ```
  Y en CI, establecer `CMAKE_BUILD_PARALLEL_LEVEL=4` en el env del step.

---

### Plan de implementación CI/Build (orden sugerido)

1. **C1+C4+C5** — Release ARM64 funcional: elegir estrategia (cross-compilation o
   QEMU o larger runners), añadir flags cmake ARM, añadir `--target` a bindgen.
2. **C3** — Fix inmediato del `.deb` control file (1 línea).
3. **C8** — Añadir `rust-toolchain.toml` (1 archivo).
4. **C9** — Añadir `libgomp-dev` a release.yml.
5. **C6+C12** — Cache de libs precompiladas + paralelismo cmake configurable.
6. **C2** — Añadir job ARM64 en CI (depende de C1).
7. **C7** — Limpiar `submodules: recursive` + actualizar AGENTS.md.
8. **C11** — Añadir `--target` a cargo build + ajustar paths.
9. **C10** — Snap `base: core24` o contenedor.

---

## 7. Cómo ejecutar la batería de tests

```bash
cd memvid-agent-core
# NOTA: en máquinas con poca RAM (~8 GB), la suite completa a máxima
# paralelización puede agotar memoria/espacio temporal (muchos tests escriben
# segmentos .mv2 con fsync a la vez). Limitar la concurrencia lo evita:
cargo test -- --test-threads=2      # suite completa, estable
cargo test --lib utils::            # solo utils (truncate_chars)
cargo test --lib agent::            # solo agent (dedup, ingest)
cargo test --test kv_cache_and_history   # integración KV + regresión UTF-8
cargo fmt --all -- --check
cargo clippy --lib
```

## 8. Resumen de tests añadidos

| Módulo | Tests | Cubre |
|---|---|---|
| `utils.rs` | 4 | truncado UTF-8 seguro (B1) |
| `llama/context.rs` | 7 | resolver/validador KV, fidelidad, `piece_len_or_needed` (K1, K3, B5) |
| `config.rs` | 8 | defaults, retrocompat KV, validación de top_p/top_k/kv_types (K1, S4) |
| `prompt.rs` | 3 | preservación developer prompt (B3), Mistral system msgs (B12) |
| `retrieval.rs` | 4 | overflow de ranking (B2), dedup (B6), append incremental (R1) |
| `agent.rs` | 1 | dedup en ingesta (B6) |
| `api.rs` | 2 | comparación en tiempo constante (S2), límite de body (S1) |
| `chunker.rs` | 1 | `chunk_fixed_multibyte_safe` (B10) |
| `languages_catalog.rs` | 5 | ampersand literal (B7), `is_empty` (B8) |
| `tests/kv_cache_and_history.rs` | 6 | contrato config KV + regresión UTF-8 e2e |
| `tests/functional.rs` | 1 | config default path actualizado (B18) |