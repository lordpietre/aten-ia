# AGENTS.md

Single Rust binary+lib crate (`aten-ia`) in `memvid-agent-core/` — local LLM inference via bundled `llama-cpp-turboquant` CMake+bindgen, `.mv2` persistence (memvid-core v2), keyword RAG, OpenAI-compatible HTTP/1.1 API, multi-source ingestion.

## Commands (run from `memvid-agent-core/`)

| Action | Command |
|---|---|
| Build (first: ~30 min cmake+llama.cpp; subsequent: <1s) | `cargo build` |
| Build release | `cargo build --release` |
| Run (auto-downloads model if missing) | `cargo run` |
| All tests (no GGUF needed) | `cargo test -- --test-threads=1` |
| Format check | `cargo fmt --all -- --check` |
| Lint (lib only — CI uses `--lib`) | `cargo clippy --lib` |
| System deps (build) | `cmake libssl-dev clang libgomp-dev` |
| System deps (+ packaging) | …also `fakeroot` |

CI order: `build → test → fmt → clippy --lib` (`.github/workflows/ci.yml`).
CI runs `cargo test -- --test-threads=1` to avoid `MODEL_PATH` env-var race between tests.

**Prebuilt libs fallback chain** (in `build.rs`):
1. `LLAMA_LOCAL_LIBS=/path` — copy `.a` files from local dir
2. Download `llama-libs-{target}.tar.gz` from GitHub Releases
3. CMake build (parallel level via `CMAKE_BUILD_PARALLEL_LEVEL`, default 2)

Override download repo: `LLAMA_LIBS_REPO=user/repo`.

**Cross-compilation**: `build.rs` detects `TARGET` vs `HOST`. If different, generates a CMake cross-toolchain and passes `--target` to bindgen. ARM64 builds use `GGML_CPU_ARM_ARCH=armv8-a+dotprod` and `GGML_NATIVE=OFF`.

**Release** (`git tag v0.1.0 && git push --tags`) triggers `.github/workflows/release.yml`: cross-compiles `aarch64` on `ubuntu-latest` with `gcc-aarch64-linux-gnu` + native build for `x86_64` → llama static libs → Rust binary → `.tar.gz` → `.deb` → `.snap`.

## Structure

- **Entrypoints**: `src/main.rs` (binary REPL), `src/lib.rs` (library, used by tests + api.rs)
- **Crate names**: binary = `aten-ia`, lib = `memvid_agent_core` (for `use` statements)
- **Module decls** in `lib.rs` — 21 modules covering all subsystems
- **Model catalog**: `src/models_catalog.json` — 20 models, loaded at runtime
- **Config**: `config.json` — version 1, fields with serde defaults; first-run wizard creates if absent
- **Persistence dir**: `memvid_data/` — `.mv2` segments + `knowledge_index.jsonl` + `manifest.json` + `.lock`
- **FFI**: `wrapper.h` → `llama-cpp-turboquant/include/llama.h` → bindgen → patched `unsafe extern "C"`

## Gotchas

- **Rust edition 2024** (min 1.85.0) — `rust-toolchain.toml` pins channel; install via `rustup`
- **`cargo clippy --lib`** only checks the library crate, not `main.rs`
- **`--test-threads=1`** required; parallel tests race on `MODEL_PATH` env var
- **First build** takes ~30 min (compiles llama.cpp from source); subsequent <1s if cached
- **First run** triggers interactive setup wizard (model select, API config, language docs)
- **`FileLock::acquire()`** creates `data_dir/.lock` with PID — concurrent instances rejected
- **`.env` loaded** before config via `dotenvy::dotenv().ok()`; optional

## Architecture

- **No GPU** — `n_gpu_layers = 0`, no CUDA/Metal/Vulkan cmake flags
- **RAG is keyword-only** — word substring match over `knowledge_index.jsonl`, no embeddings
- **API single-threaded** — raw TCP `TcpListener`, sequential connections, no streaming SSE
- **Session flushes every 5 interactions** — `Session::flush()` → `MemvidWriter`
- **KV cache type** configurable: `model.kv_type_k` / `model.kv_type_v` (default `f16`; `turbo2/3/4` enable flash-attn automatically)
- **All persistence atomic**: write → fsync → rename → fsync(parent)
- **Chunker UTF-8 safe** — `floor_char_boundary()` on overlap to avoid panics on multi-byte chars
- **`ingest <file>` auto-detects format** via `Format::from_extension()` (pdf/epub/md/html → text fallback)
- **Env overrides**: `MODEL_PATH`, `MODEL_NAME`, `MODEL_CTX`, `MODEL_URL` (applied on config load)
- **Default model**: `Qwen2.5-0.5B-Instruct` (`n_ctx: 8192`, `chat_template: chatml`, auto-downloads from HuggingFace)
- **llama.cpp verbose suppressed** at startup via `llama_log_set(noop_log)` in `context.rs`

## Testing

Integration tests use `tempfile::tempdir()` + `LlamaContext::null()` — no GGUF model required:
- `tests/functional.rs`
- `tests/writer_integration.rs`
- `tests/kv_cache_and_history.rs`

## Git-ignored

`*.gguf`, `target/`, `memvid_data/`