# AGENTS.md

Single Rust binary+lib crate in `memvid-agent-core/` — local LLM inference via bundled `llama-cpp-turboquant` CMake+bindgen, `.mv2` persistence (memvid-core v2), keyword RAG, OpenAI-compatible HTTP/1.1 API, multi-source ingestion.

## Commands (run from `memvid-agent-core/`)

| Action | Command |
|---|---|
| Build (first: ~30 min cmake+llama.cpp; subsequent: <1s) | `cargo build` |
| Build release | `cargo build --release` |
| Run (auto-downloads model if missing) | `cargo run` |
| All tests (no GGUF needed) | `cargo test` |
| Format check | `cargo fmt --all -- --check` |
| Lint (lib only — CI uses `--lib`) | `cargo clippy --lib` |
| System deps | `cmake libssl-dev clang libgomp1` |
| Prebuilt libs + binary release | `git tag v0.1.0 && git push --tags` (triggers `.github/workflows/release.yml` + `release-binary.yml`) |

CI order: `build → test → fmt → clippy --lib` (`.github/workflows/ci.yml`).
Build time: ~30 min first run, <5 min after cache warms (`Swatinem/rust-cache`).

**Prebuilt libs fallback chain** (in `build.rs`):
1. `LLAMA_LOCAL_LIBS=/path` — copy `.a` files from local dir
2. Download `llama-libs-{target}.tar.gz` from GitHub Releases
3. Cmake build with `CMAKE_BUILD_PARALLEL_LEVEL=1` to avoid OOM

Override download repo: `LLAMA_LIBS_REPO=user/repo`.

**CI checkout requires `submodules: recursive`** for `llama-cpp-turboquant/`.

**Release binaries** built for `x86_64` (ubuntu-latest) and `aarch64` (ubuntu-24.04-arm).
Prebuilt llama libs (`release.yml`) build the same matrix — `release-binary.yml` compiles the Rust crate.

## Structure

- **Entrypoints**: `src/main.rs` (binary REPL), `src/lib.rs` (library, used by tests + api.rs)
- **Module decls** in `lib.rs` — 19 modules covering all subsystems
- **Model catalog**: `src/models_catalog.json` — 20 models, loaded at runtime
- **Config**: `config.json` — version 1, fields with serde defaults; first-run wizard creates if absent
- **Persistence dir**: `memvid_data/` — `.mv2` segments + `knowledge_index.jsonl` + `manifest.json` + `.lock`
- **FFI**: `wrapper.h` → `llama-cpp-turboquant/include/llama.h` → bindgen → patched `unsafe extern "C"`

## Key details

- **Rust edition 2024** (min 1.95.0)
- **No GPU** — `n_gpu_layers = 0`, no CUDA/Metal/Vulkan cmake flags
- **RAG is keyword-only** — word substring match over `knowledge_index.jsonl`, no embeddings
- **API single-threaded** — raw TCP `TcpListener`, sequential connections, no streaming SSE
- **Session flushes every 5 interactions** — `Session::flush()` → `MemvidWriter`
- **KV cache type** is configurable: `model.kv_type_k` / `model.kv_type_v` (default `f16`; `turbo2/3/4` enable flash-attn automatically)
- **All persistence is atomic**: write → fsync → rename → fsync(parent)
- **`ingest <file>` auto-detects format** via `Format::from_extension()` (pdf/epub/md/html → text fallback)
- **Env overrides**: `MODEL_PATH`, `MODEL_NAME`, `MODEL_CTX`, `MODEL_URL` (applied on config load)
- **Default model**: `Qwen2.5-0.5B-Instruct` (`n_ctx: 8192`, `chat_template: chatml`)
- **`FileLock::acquire()`** creates `data_dir/.lock` with PID — concurrent instances rejected
- **First run** triggers interactive setup wizard (model select, API config, language docs)
- **Integration tests** (`tests/functional.rs`, `tests/writer_integration.rs`, `tests/kv_cache_and_history.rs`) — `tempfile::tempdir()` + `LlamaContext::null()`, no GGUF model required
- **`cargo clippy --lib`** only checks the library crate, not `main.rs`
- **Git-ignored**: `*.gguf`, `target/`, `memvid_data/`
- **`.env` loaded** at startup via `dotenvy::dotenv().ok()` (before config load); optional
