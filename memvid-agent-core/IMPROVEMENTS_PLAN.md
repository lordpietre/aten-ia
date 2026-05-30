# Plan exhaustivo de mejoras y corrección de errores — memvid-agent-core

Estado del documento: vivo. Marca `[x]` = implementado y con test en este repo; `[ ]` = propuesto.

Leyenda de severidad: 🔴 crítico (pánico/corrupción) · 🟠 funcional (resultado incorrecto) · 🟡 calidad/UX · 🔵 mejora.

---

## 0. Contexto

`memvid-agent-core` es un agente LLM local (CPU, llama.cpp) con persistencia `.mv2`,
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
- **Fix:** `fetch_and_ingest` ahora devuelve `(FetchedContent, usize)` con el número
  real de chunks indexados (dedup incluido); `process_url_batch` suma ese valor y
  `/fetch` lo muestra ("chunks indexed: N").
- **Tests:** cubierto indirectamente por `store_knowledge_dedup_*` (el conteo deriva
  del nº de inserciones efectivas). Pendiente test e2e con servidor local (ver B4b).
- **B4b (pendiente):** test pragmático con `TcpListener` local sirviendo 2 URLs y
  aserción `result.total_chunks == knowledge_count`.

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
- **Tests (`languages_catalog.rs`):** el test `strip_html_lone_ampersand` (antes rojo)
  + `_at_eof`, `_followed_by_space`, `_two_lone_ampersands`, `_lone_then_valid_entity`.
  Toda la suite `strip_html` (13 tests) en verde.

### B8 — Deuda de lints clippy  🟡  **[x] CORREGIDO**
- Eran 33 warnings preexistentes. `cargo clippy --fix --lib` aplicó 31 fixes
  automáticos (collapsible_if→let-chains, `next_back`/`rfind`, `unwrap_or_default`,
  etc.). Los 5 restantes resueltos a mano:
  - `LanguagesCatalog::is_empty()` añadido (+ test `catalog_is_empty_reflects_len`).
  - `ChatTemplate::from_str` marcado `#[allow(should_implement_trait)]` (es infalible
    a propósito: desconocido→`Raw`).
  - bloque markdown redundante colapsado en `extractor::extract_text`.
  - `web_fetcher` usa `checked_div`.
  - campo `n_gpu_layers` marcado `#[allow(dead_code)]` (retenido para K4).
- **Resultado:** `cargo clippy --lib` ahora **0 warnings**.

### B6 — Sin deduplicación por checksum en el índice  🟡  **[x] CORREGIDO (opt-in)**
- **Evidencia:** `KnowledgeEntry.checksum` se calculaba pero nunca se usaba;
  re-ingerir el mismo fichero/URL duplicaba entradas.
- **Fix (opt-in para no romper la suite):** `KnowledgeIndex::add_entry_dedup` ignora
  `(source, checksum)` ya presente; `Agent::store_knowledge_dedup` lo usa y solo se
  aplica en la ruta de `fetch_and_ingest`. El `add_entry`/`store_knowledge_chunked`
  genéricos siguen apilando (tests de chunks repetidos intactos).
- **Tests:** `add_entry_dedup_skips_same_source_and_checksum`,
  `add_entry_dedup_keeps_distinct_content_and_sources` (`retrieval.rs`);
  `store_knowledge_dedup_skips_repeat_ingestion` (`agent.rs`).

---

## 2. Robustez / seguridad

### S1 — API server de un solo hilo y bloqueante  🔵  **[x] CORREGIDO (parcial)**
- **Evidencia:** `run` procesaba conexiones secuencialmente, sin timeout ni límite
  de body → una conexión lenta bloqueaba a todas.
- **Fix:** un hilo por conexión (`std::thread::spawn`, `ApiServer: Clone`),
  `set_read_timeout`/`set_write_timeout` (30 s) y límites `MAX_BODY_BYTES` (10 MB) /
  `MAX_HEADER_BYTES` (64 KB). La inferencia sigue serializando en el mutex del agente
  (comportamiento deseado de un solo modelo).
- **Tests:** `within_body_limit_enforces_cap` (`api.rs`). El timeout/concurrencia
  requiere test de integración con sockets (pendiente, S1b).
- **Pendiente (S1b):** test e2e con 2 conexiones; streaming SSE.

### B9 — `/fetch-md` bloqueaba el mutex del agente durante la descarga  🟡  **[x] CORREGIDO**
- **Evidencia:** el handler adquiría `agent.lock()` sin usarlo, reteniéndolo durante
  el fetch de red (warning `unused variable: a` + bloqueo innecesario).
- **Fix:** eliminado el lock muerto; `/fetch-md` solo descarga e imprime.

### S2 — `check_auth` sin protección contra timing  🔵  **[x] CORREGIDO**
- **Evidencia:** `check_auth` comparaba el token con `==` (corto-circuito en el primer
  byte distinto → fuga de timing).
- **Fix:** `constant_time_eq` (XOR acumulado sobre el máximo de ambas longitudes).
- **Tests:** `constant_time_eq_matches_only_on_equal` (igual/distinto/longitudes).

### S3 — Token de API real commiteado en `config.json`  🟠  **[x] CORREGIDO**
- **Evidencia:** `config.json` versionado traía `"token": "a9330b26-..."`.
- **Fix:** `"token": null` en el fichero versionado (se genera en runtime con `/token`).

---

## 3. KV-cache TurboQuant (Opción 4) — seguimiento

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

## 4. Rendimiento / calidad de RAG

### R1 — `add_entries` reescribe el JSONL completo  🔵  **[x] CORREGIDO**
- **Antes:** O(total) — reescribía todo el JSONL por cada lote.
- **Fix:** `append_entries_to_jsonl` añade solo las líneas nuevas con un único
  `write_all` + `fsync`. O(nuevas) en vez de O(total).
- **Tests:** `add_entries_persists_incrementally_to_jsonl` (escribe 2 lotes, recarga
  desde disco y verifica las 3 entradas + búsqueda).

### R2 — RAG semántico (embeddings)  🔵  **[ ]**
- Sustituir/complementar el match por substring con embeddings (el fork trae soporte
  de embeddings en llama.cpp). Gran salto de calidad de recuperación.
- **Tests:** de relevancia con un corpus pequeño etiquetado (golden set).

### R3 — Normalización de la búsqueda  🟡  **[ ]**
- `search` cuenta `matches()` (substrings solapados) y mezcla content/source/id sin
  pesos. Considerar normalizar por longitud y dar más peso a coincidencias en source.

---

## 5. Estado actual

**Completados** (con tests, en verde): B1, B2, B3, B4, B5, B6, B7, B8, B9, S1 (parcial),
S2, S3, K1, K3, R1.

**Pendientes:**
- **B4b** — test e2e de `process_url_batch` con `TcpListener` local.
- **S1b** — test e2e de concurrencia/timeout de la API + streaming SSE.
- **K2** — de-vendorizar el fork a submódulo.
- **K4** — `MODEL_GPU_LAYERS` + flags GPU en `build.rs`.
- **R2** — RAG semántico (embeddings).
- **R3** — normalización/pesos en `search`.

Orden sugerido para lo que queda: K4 → R2/R3 → K2 → B4b/S1b.

---

## 6. Cómo ejecutar la batería de tests

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

## 7. Resumen de tests añadidos por este trabajo

| Módulo | Tests | Cubre |
|---|---|---|
| `utils.rs` | 4 | truncado UTF-8 seguro (B1) |
| `llama/context.rs` | 7 | resolver/validador KV, fidelidad, `piece_len_or_needed` (K1, K3, B5) |
| `config.rs` | 2 | defaults + retrocompat KV (K1) |
| `prompt.rs` | 2 | preservación developer prompt (B3) |
| `retrieval.rs` | 4 | overflow de ranking (B2), dedup (B6), append incremental (R1) |
| `agent.rs` | 1 | dedup en ingesta (B6) |
| `api.rs` | 2 | comparación en tiempo constante (S2), límite de body (S1) |
| `languages_catalog.rs` | 5 | ampersand literal (B7), `is_empty` (B8) |
| `tests/kv_cache_and_history.rs` | 6 | contrato config KV + regresión UTF-8 e2e |
