# PLAN — Auditoría y hoja de ruta de aten-ia

Documento de trabajo derivado de la auditoría de noviembre de 2026.
Estados: ✅ hecho, 🚧 en curso, ⏳ pendiente, ⚠️ requiere decisión.

## 0. Licencia y metadatos

- [x] ✅ `LICENSE` Apache 2.0 con copyright de **Dinámicas Electrónicas**
      (`dinamicas.electronicas@gmail.com`).
- [x] ✅ `LICENSE` y contacto en `README.md`.
- [x] ✅ Metadatos Cargo: `license`, `authors`, `description`, `readme`,
      `keywords`, `categories`, `rust-version`.
- [x] ✅ `SECURITY.md` con correo de contacto.
- [ ] ⏳ Añadir `NOTICE` si se incorpora código de terceros con atribución.

## 1. Fase 0 — Contención de seguridad

- [x] ✅ Token de API del `config.json` de ejemplo eliminado; `api.token` ahora
      es `null` y `config.json` está ignorado por git.
- [x] ✅ `config.example.json` como plantilla versionada.
- [x] ✅ `atomic_write` crea ficheros con `0600` en Unix.
- [x] ✅ Directorio de datos con `0700`.
- [x] ✅ `/token` respeta `api.host` (127.0.0.1 por defecto) en lugar de forzar
      `0.0.0.0`.
- [x] ✅ Verificación SHA-256 del modelo configurado, tanto al descargar como si
      el fichero ya existe.
- [ ] ⏳ Publicar checksums por defecto en `models_catalog.json`.
- [ ] ⏳ Verificar el tarball de librerías precompiladas de llama.cpp.
- [ ] ⏳ Añadir `SECURITY.md` a la documentación de release.

## 2. Fase 1 — Dependencias y cadena de suministro

- [x] ✅ `cargo update` semver: rustls, rustls-webpki, webpki-roots, url,
      anyhow, quick-xml, zstd, memmap2, rustix, memvid-core y otros.
- [x] ✅ `time` 0.3.47 (RUSTSEC-2026-0009).
- [x] ✅ `Cargo.lock` regenerado (raíz `0.3.12`) y `--locked` en CI/release.
- [x] ✅ Eliminada la dependencia directa `zip` (no se usaba en `src/`).
- [x] ✅ Toolchain local a Rust 1.100; `rust-version = 1.89`.
- [x] ✅ `action-gh-release@v2`, `permissions` mínimos y Dependabot.
- [x] ✅ `pdf-extract` 0.12.1 / `lopdf` 0.42 en la ruta directa (RUSTSEC-2026-0187); `memvid-core` conserva `lopdf` 0.39 como dependencia transitiva.
- [ ] ⏳ `feed-rs` 3, `bindgen` 0.73, `indicatif` 0.18, `sha2` 0.11.
- [ ] ⏳ Resolver `zip` 7.x transitivo (memvid-core) o documentar que no se usan
      sus APIs de extracción.
- [ ] ⏳ Revisar avisos transitivos de `memvid-core` (`calamine`, `bincode`,
      `lopdf`, `quick-xml`).
- [x] ✅ `cargo audit` en CI con ignores documentados para dependencias transitivas de `memvid-core`.
- [ ] ⏳ `cargo deny` y política de licencias.
- [ ] ⚠️ Migrar imágenes EOL (`node:20-bullseye`, `ubuntu:20.04`) o asumir
      explícitamente glibc 2.31.
- [ ] ⏳ Proceso de sincronización del fork vendorizado de llama.cpp.

## 3. Fase 2 — Correctitud funcional

- [x] ✅ Pánico UTF-8 en el preview de `/debug`.
- [x] ✅ Validación `max_tokens < n_ctx` y resta saturante.
- [x] ✅ Toggle real de `/debug` (`Agent::is_debug`).
- [x] ✅ Escritura JSONL en lote (un `fsync` por documento en vez de por chunk).
- [x] ✅ `/ingest` de texto guarda un resumen, no el documento completo, en la sesión.
- [x] ✅ `/unlearn` durable con `knowledge_tombstones.jsonl` (incluye libros).
- [x] ✅ Deduplicación en `/learn` y `/download-books`.
- [x] ✅ Deduplicación en `/ingest` y reingesta idempotente de chunks.
- [x] ✅ Recuperación de cola: `processing` huérfanas vuelven a `pending`.
- [x] ✅ `/batch`: barra de progreso incremental.
- [x] ✅ API: `max_tokens`, `temperature`, `top_p`, `top_k`, `usage` real y
      contador separado del REPL.
- [ ] ⏳ API: cambio de modelo por petición (hoy usa el modelo activo).
- [ ] ⏳ `data_dir` independiente del CWD o flag `--data-dir`.

## 4. Fase 3 — Rendimiento

- [x] ✅ Hilos de inferencia = núcleos disponibles (`ATEN_THREADS` para fijarlos).
- [x] ✅ `GGML_NATIVE=ON` en builds nativos (OFF en portable/cross).
- [x] ✅ Paralelismo de CMake por defecto = núcleos.
- [x] ✅ Perfil release: `lto = "thin"`, `codegen-units = 1`, `strip = true`.
- [ ] ⏳ Cache de prefijo KV entre peticiones (mayor ganancia en OpenCode).
- [ ] ⏳ RAG: texto normalizado cacheado e índice invertido.
- [x] ✅ Dedup O(1) con `HashSet<(source, checksum)>` en la ingesta.
- [x] ✅ Pool acotado a 64 conexiones HTTP (`ATEN_API_MAX_CONNS`).
- [ ] ⏳ Persistencia de cola por lote.

## 5. Fase 4 — Calidad y operación

- [x] ✅ `cargo audit` en CI.
- [ ] ⏳ Cobertura y fuzzing (HTTP, feeds, PDF, EPUB).
- [ ] ⏳ SBOM y firma de releases.
- [ ] ⏳ `CHANGELOG.md` y notas de release.
- [x] ✅ `.deb` sin `libssl3`; dependencia real `libc6 >= 2.31` y herramientas en `Suggests`.
- [ ] ⏳ Probar instalación en la distro más antigua soportada.
- [ ] ⚠️ Decidir si el snap se construye en CI o se retira.

## 6. Fase 5 — Evolución (opcional)

- RAG híbrido (embeddings + keyword).
- Streaming token a token.
- API con herramientas/function calling y `/v1/embeddings`.
- Backend GPU opcional.
- Cifrado en reposo.

## 6. Fase 6 — Verificación y CI (SDD)

- [x] ✅ `--self-test` / `--verify`: 12 checks de runtime sin modelo ni red.
- [x] ✅ `tests/cli.rs`: flags del binario.
- [x] ✅ `tests/network_ingest.rs`: fetch, feed, lote y límite de tamaño con
      servidor local.
- [x] ✅ `tests/api_server.rs`: API TCP real (health, auth, models, 404, JSON).
- [x] ✅ `scripts/ci/verify.sh` y `scripts/ci/check-docs.sh`.
- [x] ✅ Agente OpenCode `verifier` con permisos restringidos.
- [x] ✅ Job `docs` y CLI smoke en CI; `verify.yml` en tags/manual; gate nativo
      en release.
- [x] ✅ `SDD.md` con especificación, trazabilidad y criterios de release.
- [x] ✅ `cargo audit` en CI.
- [ ] ⏳ `cargo deny` y SBOM/firma.
- [ ] ⏳ Job nightly con GGUF real y benchmarks.
- [ ] ⏳ Fuzzing de parser HTTP/feeds/PDF.

## Registro de verificación

| Fecha | Comprobación | Resultado |
|---|---|---|
| 2026-11-25 | `cargo update` + `cargo metadata --locked` | ✅ lock 0.3.12 |
| 2026-11-25 | `cargo test --locked -- --test-threads=1` | ✅ 541 tests, 0 fallos |
| 2026-11-25 | `cargo fmt --all -- --check` | ✅ |
| 2026-11-25 | `cargo clippy --locked --lib` | ✅ 10 avisos de estilo preexistentes |
| 2026-11-25 | `time 0.3.47` + suite completa | ✅ 541 tests, 0 fallos |
| 2026-11-25 | `--self-test` (12 checks) | ✅ |
| 2026-11-25 | `scripts/ci/verify.sh` completo | ✅ |
| 2026-11-25 | Suite con CLI, red local y API TCP | ✅ 551 tests, 0 fallos |
| 2026-11-25 | CI GitHub `80754c6` | ✅ x86_64, aarch64 y Docs |
| 2026-11-25 | Tombstones + pdf-extract 0.12 + cola | ✅ 555 tests, 0 fallos |
| 2026-11-25 | Dedup O(1) e ingesta idempotente | ✅ 556 tests, 0 fallos |
| 2026-11-25 | `cargo audit` + `.cargo/audit.toml` | ✅ 0 vulnerabilidades, 6 warnings |
| 2026-11-25 | API: parámetros por petición y `usage` | ✅ 558 tests, 0 fallos |
| 2026-11-25 | Límite de conexiones HTTP y progreso `/batch` | ✅ build + tests |
