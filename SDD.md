# SDD — Especificación y sistema de verificación de aten-ia

| Campo | Valor |
|---|---|
| Documento | SDD (Software Design Document / Spec-Driven Development) |
| Proyecto | aten-ia 0.3.12 |
| Propietario | Dinámicas Electrónicas `<dinamicas.electronicas@gmail.com>` |
| Estado | Aprobado para ejecución |
| Fecha | 2026-11-25 |
| Relacionado | `README.md`, `PLAN.md`, `SECURITY.md`, `AGENTS.md` |

---

## 1. Propósito

Definir cómo se verifica que aten-ia **compila, arranca y usa correctamente
todas sus funciones**, y cómo se integra esa verificación en CI para que cada
release futura pase por las mismas puertas de calidad.

El documento cubre:

1. Especificación funcional y no funcional.
2. El **agente verificador** (sintaxis + uso) con permisos y herramientas.
3. El **sistema CI** (jobs, matrices, artefactos, gates de release).
4. El **plan de pruebas** y su trazabilidad función → test → job.
5. Criterios de aceptación y riesgos.

**No-goal:** sustituir pruebas manuales de calidad de respuesta del modelo. La
inferencia real, la descarga de GGUF y el fine-tuning requieren un modelo y
recursos externos; se cubren en una capa opcional (Tier 4) y en la checklist de
release.

---

## 2. Actores

| Actor | Responsabilidad |
|---|---|
| Desarrollador | Ejecuta `scripts/ci/verify.sh` antes de commitear |
| Agente `verifier` | Ejecuta la verificación y reporta sin modificar el repo |
| CI (GitHub/Gitea) | Repite la verificación en cada push/PR y en tags |
| Release manager | Comprueba gates, firma/checksums y publica |
| Usuario | Reporta fallos de uso con `--self-test` y logs |

---

## 3. Especificación funcional

### 3.1 Inventario de funciones

| ID | Función | Entrada | Salida / efecto | Cobertura automática |
|---|---|---|---|---|
| F-01 | Configuración | `config.json`, env | Config validada, data dir | Unit + integration + self-test |
| F-02 | Primer arranque | TTY | `config.json` + modelo | Manual (wizard) |
| F-03 | Catálogo de modelos | `/models`, `/model` | Lista/cambio | Unit + self-test |
| F-04 | Descarga de modelo | `download_url`, SHA-256 | GGUF verificado | Unit (checksum) + manual |
| F-05 | Chat LLM | Mensaje | Respuesta | Tier 4 (modelo real) |
| F-06 | Sesión e historial | Mensajes | `.mv2` + flush | Unit + integration |
| F-07 | RAG keyword | Consulta | Chunks puntuados | Unit + integration |
| F-08 | Chunking | Texto + opciones | Chunks UTF-8 | Unit + self-test |
| F-09 | Ingesta de ficheros | PDF/EPUB/MD/HTML/TXT | Conocimiento | Unit (formatos sintéticos) + Tier 4 |
| F-10 | Fetch web | URL | Conocimiento | Integration (servidor local) |
| F-11 | Feeds RSS/Atom | URL/XML | Entradas indexadas | Unit + integration |
| F-12 | Lote de URLs | Fichero | Resumen | Integration |
| F-13 | Cola persistente | URLs | Estados JSONL | Unit + self-test |
| F-14 | `/learn` | Lenguaje | Docs indexadas | Unit (parser) + manual (red) |
| F-15 | `/download-books` | Lenguaje | Metadatos | Unit |
| F-16 | Fine-tuning | Corpus | Estimación/script/binario | Unit (estimación) + Tier 4 |
| F-17 | API HTTP | HTTP | JSON/SSE | Unit + integration TCP |
| F-18 | KV-cache | `/kv` | Contexto recargado | Unit + integration |
| F-19 | Plantillas | Mensajes | Prompt | Unit + self-test |
| F-20 | Utilidades | Paths/datos | Ficheros atómicos, lock | Unit + self-test |
| F-21 | Shutdown | SIGINT/SIGTERM | Salida limpia | Unit (flag) + manual |
| F-22 | CLI | Args | `--version`, `--self-test` | Integration CLI |
| F-23 | Documentación | MD/roff | Docs válidas | CI docs |
| F-24 | Empaquetado | Tag | `.deb`/`.tar.gz` | CI release |
| F-25 | Seguridad | Secretos/permisos | Sin tokens, `0600/0700` | CI docs + unit |

### 3.2 Requisitos de comportamiento

- **R-01** `cargo build --locked` debe funcionar en x86_64 y aarch64.
- **R-02** `cargo test --locked -- --test-threads=1` debe pasar sin GGUF.
- **R-03** `aten-ia --version` y `aten-ia --self-test` deben salir con código 0
  sin descargar modelo ni abrir red.
- **R-04** La API debe escuchar en `api.host` (127.0.0.1 por defecto).
- **R-05** Los ficheros con secretos se escriben con `0600`; el data dir con
  `0700` en Unix.
- **R-06** El modelo configurado con `sha256` debe verificarse.
- **R-07** Un release no se publica si falla cualquier gate de la sección 6.

---

## 4. Requisitos no funcionales

| ID | Requisito | Objetivo |
|---|---|---|
| NFR-01 | Tiempo de CI | < 25 min con caché de llama.cpp |
| NFR-02 | Cobertura funcional | Tiers 0–3 al 100 % de funciones sin modelo |
| NFR-03 | Reproducibilidad | `--locked` en build/test/release |
| NFR-04 | Seguridad | Sin secretos en repo; permisos mínimos en Actions |
| NFR-05 | Portabilidad | glibc 2.31 en binarios portables |
| NFR-06 | Observabilidad | Salida clara `ok/FAIL` por check del self-test |
| NFR-07 | Mantenibilidad | Un solo comando local: `scripts/ci/verify.sh` |

---

## 5. Agente verificador

### 5.1 Definición

Archivo: `.opencode/agents/verifier.md`

| Campo | Valor |
|---|---|
| ID | `verifier` |
| Modo | `subagent` |
| Objetivo | Verificar sintaxis, build, tests, CLI y documentación |
| Modifica archivos | **No** |
| Hace commit/push | **No** |
| Descarga modelos | **No** |

### 5.2 Permisos

| Acción | Recurso | Efecto |
|---|---|---|
| `*` | `*` | `deny` |
| `read` | `**` | `allow` |
| `glob` | `**` | `allow` |
| `grep` | `**` | `allow` |
| `shell` | `cargo *` | `allow` |
| `shell` | `./scripts/ci/*` | `allow` |
| `shell` | `bash scripts/ci/*` | `allow` |
| `shell` | `git status*` | `allow` |
| `shell` | `git diff*` | `allow` |
| `shell` | `git log*` | `allow` |

Cualquier otra acción (edición, push, borrado, red) queda denegada por la regla
inicial.

### 5.3 Flujo

1. `git status --short` — anotar si el árbol está sucio.
2. `./scripts/ci/verify.sh` — suite completa.
3. Si falla, extraer el primer `FAIL`/`error[E…]` y el comando exacto.
4. Repetir solo el subcomando afectado para confirmar (sin editar).
5. Reportar tabla: check, resultado, evidencia, sugerencia.
6. No corregir ni commitear: el mantenimiento lo hace el agente principal.

### 5.4 Salida esperada

```
| Check | Resultado | Evidencia |
|---|---|---|
| metadata --locked | ok | lock 0.3.12 |
| fmt | ok | — |
| clippy | ok (10 warnings) | ... |
| tests | ok | 5xx passed |
| self-test | ok | all checks passed |
| docs | ok | links/secrets/man |
```

Si algo falla, termina con un resumen y **código de salida distinto de 0**.

---

## 6. Sistema CI

### 6.1 Workflows

| Workflow | Disparador | Objetivo |
|---|---|---|
| `ci.yml` | push/PR a `main`, manual | Build x86_64/aarch64, tests, fmt, clippy, CLI smoke, docs |
| `verify.yml` | tags `v*`, manual | Suite completa `scripts/ci/verify.sh` |
| `release.yml` | tags `v*`, manual | Gate nativo + build release + `.deb`/`.tar.gz` + SHA-256 |

Gitea replica `ci.yml`, `verify.yml` y `release.yml` en `.gitea/workflows/`.

### 6.2 Jobs de `ci.yml`

| Job | Runner | Pasos clave |
|---|---|---|
| `build-and-test` (x86_64) | ubuntu-latest | deps → llama libs → build → test → fmt → clippy → `--version` → `--self-test` |
| `build-and-test` (aarch64) | ubuntu-latest | build cruzado (sin tests) |
| `docs` | ubuntu-latest | `groff` + `scripts/ci/check-docs.sh` |

### 6.3 Gates de release

1. `ci.yml` verde en el commit/tag.
2. `verify.yml` verde sobre el tag.
3. `release.yml` ejecuta en nativo: `fmt`, `clippy`, `test`, `--self-test`.
4. Se generan y publican checksums SHA-256.
5. Checklist manual: probar `/help`, `/token`, `/ingest`, `/search` con un GGUF.
6. Sin secretos en el diff ni en `config.example.json`.

### 6.4 Matriz de compatibilidad

| Target | Build | Tests | Release |
|---|---|---|---|
| x86_64-unknown-linux-gnu | ✅ | ✅ | ✅ |
| aarch64-unknown-linux-gnu | ✅ | ❌ (cross) | ✅ |

Tests solo en nativo por velocidad y porque no requieren modelo.

### 6.5 Artefactos

- Log de verificación (`verify.log`, retención 7 días).
- `aten-ia-<target>.tar.gz` + `.sha256`.
- `aten-ia_<versión>_<arch>.deb` + `.sha256`.
- (Futuro) SBOM CycloneDX y firma `cosign`.

---

## 7. Plan de pruebas

### 7.1 Niveles

| Tier | Qué prueba | Requiere | Dónde |
|---|---|---|---|
| T0 | Sintaxis, formato, clippy, metadata | Rust | CI |
| T1 | Unitarios e integración | `cargo test` | CI |
| T2 | CLI: `--version`, `--self-test` | binario | CI |
| T3 | Red local: fetch/feed/batch + API TCP | loopback | CI |
| T4 | Inferencia, descarga GGUF, PDF/EPUB reales, fine-tune | modelo/red | Manual/nightly |

### 7.2 Trazabilidad

| Función | Tests |
|---|---|
| F-01 | `src/config.rs` tests, `tests/functional.rs`, self-test `config` |
| F-03 | `models_catalog` tests, self-test `models` |
| F-04 | `models::tests`, self-test `utils` |
| F-05 | Tier 4 manual |
| F-06 | `memvid::*`, `session::*`, `writer_integration` |
| F-07 | `retrieval::*`, `functional` |
| F-08 | `chunker::*`, self-test `chunker` |
| F-09 | `extractor::*`, Tier 4 |
| F-10 | `tests/network_ingest.rs::fetch_*` |
| F-11 | `feeds::*`, `network_ingest::feed_*` |
| F-12 | `network_ingest::batch_*` |
| F-13 | `queue::*`, self-test `queue` |
| F-14 | `languages_catalog::*` |
| F-15 | `books_catalog::*`, self-test `books` |
| F-16 | `finetune::*`, self-test `finetune` |
| F-17 | `api::*`, `tests/api_server.rs` |
| F-18 | `kv_cache_and_history`, `llama::context` |
| F-19 | `prompt::*`, self-test `prompts` |
| F-20 | `utils::*`, self-test `utils` |
| F-21 | `shutdown`, tests de flag |
| F-22 | `tests/cli.rs` |
| F-23 | `scripts/ci/check-docs.sh` |
| F-24 | `release.yml` |
| F-25 | `utils` permisos, `check_docs.py` secretos |

### 7.3 Estrategia sin modelo

- `LlamaContext::null()` permite probar persistencia, RAG, ingesta y API sin
  cargar pesos.
- Los tests de red usan un `TcpListener` local; no salen a Internet.
- El self-test usa un directorio temporal y no toca datos del usuario.
- Los tests se ejecutan con `--test-threads=1` para evitar carreras de
  `MODEL_PATH` y del flag global de shutdown.

### 7.4 Criterios de cobertura

- Toda función del inventario tiene al menos un test T1–T3 o una justificación
  Tier 4.
- Todo comando del REPL aparece en `README.md` y en `man/aten-ia.1`.
- Todo endpoint aparece en `README.md`, `wiki/Usage.md` y `SDD.md`.
- Cualquier función nueva debe añadir: test, entrada en la matriz y, si aplica,
  check en `selfcheck.rs`.

---

## 8. Seguridad y cadena de suministro

| Control | Estado |
|---|---|
| `config.json` ignorado; ejemplo sin token | ✅ |
| Permisos `0600/0700` | ✅ |
| API en loopback por defecto | ✅ |
| Checksum de modelo | ✅ |
| `cargo update` de advisories altos | ✅ |
| `cargo audit` en CI con ignores documentados | ✅ |
| `cargo deny` / SBOM / firma | ⏳ pendiente |
| Imágenes base EOL | ⏳ decisión pendiente |

---

## 9. Riesgos

| Riesgo | Impacto | Mitigación |
|---|---|---|
| Tests lentos por fsync en cada `.mv2` | CI lenta | Batch JSONL, caché, self-test acotado |
| `--test-threads=1` obligatorio | Más lento | Mantener; aislar env en el futuro |
| Modelo real no testeado en CI | Regresión de inferencia | Tier 4 nightly/manual |
| Fork de llama.cpp desactualizado | Seguridad/rendimiento | Proceso de sync trimestral |
| Advisories transitivos de `memvid-core` | Seguridad | Seguimiento upstream + auditoría |
| Dependencia de red para `/learn`/`/fetch` | Tests frágiles | Servidor local en T3 |
| Agente con shell amplio | Riesgo operativo | Permisos restrictivos |

---

## 10. Roadmap de verificación

| Fase | Entrega | Estado |
|---|---|---|
| V1 | Self-test + CLI tests + scripts + CI docs/CLI smoke | ✅ este incremento |
| V2 | `verify.yml` + gate de release + agente `verifier` | ✅ este incremento |
| V3 | `cargo deny` + SBOM + firma (audit ya activo) | ⏳ |
| V4 | Job nightly con GGUF real y benchmarks | ⏳ |
| V5 | Fuzzing de parser HTTP/feeds/PDF y cobertura | ⏳ |

---

## 11. Criterios de aceptación de este incremento

1. `aten-ia --self-test` pasa y cubre ≥ 10 subsistemas.
2. `tests/cli.rs` valida `--version`, `--self-test` y `--verify`.
3. `tests/network_ingest.rs` cubre fetch, feed, lote y límite de tamaño.
4. `tests/api_server.rs` cubre health, models, token, auth, 404 y JSON inválido.
5. `scripts/ci/verify.sh` ejecuta todo lo anterior y devuelve 0.
6. `scripts/ci/check-docs.sh` valida enlaces, secretos y página de manual.
7. `ci.yml` ejecuta CLI smoke y job de docs; `release.yml` tiene gate nativo.
8. `.opencode/agents/verifier.md` existe con permisos restringidos.
9. `cargo test --locked -- --test-threads=1`, `fmt` y `clippy` en verde.
10. `PLAN.md` y `README.md` enlazan este documento.
11. CI de GitHub en verde para x86_64, aarch64 y Docs.

---

## 12. Anexos

### 12.1 Comandos de verificación

```bash
# Suite completa local
./scripts/ci/verify.sh

# Solo documentación
./scripts/ci/check-docs.sh

# Tests
cd memvid-agent-core
cargo test --locked -- --test-threads=1

# CLI
cargo run --locked -- --version
cargo run --locked -- --self-test
```

### 12.2 Mapa de archivos de verificación

```
.opencode/agents/verifier.md     Agente verificador (subagent)
scripts/ci/verify.sh             Suite completa
scripts/ci/check-docs.sh         Documentación + man
scripts/ci/check_docs.py         Enlaces, secretos, config
scripts/package-deb.sh           Empaquetado .deb + self-test
memvid-agent-core/src/selfcheck.rs   Checks de runtime
memvid-agent-core/tests/cli.rs       Flags del binario
memvid-agent-core/tests/network_ingest.rs  Fetch/feed/lote locales
memvid-agent-core/tests/api_server.rs      API TCP real
.github/workflows/{ci,verify,release}.yml
.gitea/workflows/{ci,verify,release}.yml
```
