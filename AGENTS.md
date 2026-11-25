# AGENTS.md

Single Rust binary+lib crate (`aten-ia`) in `memvid-agent-core/` — local LLM inference via bundled `llama-cpp-turboquant` CMake+bindgen, `.mv2` persistence (memvid-core v2), keyword RAG, OpenAI-compatible HTTP/1.1 API, multi-source ingestion.

## Commands (run from `memvid-agent-core/`)

| Action | Command |
|---|---|
| Build (first: ~30 min cmake+llama.cpp; subsequent: <1s) | `cargo build` |
| Build release | `cargo build --release` |
| Run (auto-downloads model if missing) | `cargo run` |
| All tests (no GGUF needed) | `cargo test --locked -- --test-threads=1` |
| Format check | `cargo fmt --all -- --check` |
| Lint (lib only — CI uses `--lib`) | `cargo clippy --locked --lib` |
| System deps (build) | `cmake libssl-dev clang libclang-dev libgomp-dev` |
| System deps (+ packaging) | …also `fakeroot` |

CI order: `build → test → fmt → clippy --lib` (`.github/workflows/ci.yml`).
CI runs `cargo test -- --test-threads=1` to avoid `MODEL_PATH` env-var race between tests.

**Prebuilt libs fallback chain** (in `build.rs`):
1. `LLAMA_LOCAL_LIBS=/path` — copy `.a` files from local dir
2. Download `llama-libs-{target}.tar.gz` from GitHub Releases
3. CMake build (parallel level via `CMAKE_BUILD_PARALLEL_LEVEL`, default 2)

Override download repo: `LLAMA_LIBS_REPO=user/repo`.

**Cross-compilation**: `build.rs` detects `TARGET` vs `HOST`. If different, generates a CMake cross-toolchain and passes `--target` to bindgen. ARM64 builds use `GGML_CPU_ARM_ARCH=armv8-a+dotprod` and `GGML_NATIVE=OFF`.

**Release** (`git tag v0.3.12 && git push --tags`) triggers workflows: cross-compiles `aarch64` with `gcc-aarch64-linux-gnu` + native build for `x86_64` inside a `node:20-bullseye` container (glibc 2.31) → llama static libs → Rust binary → `.tar.gz` + `.deb` (+ SHA-256).

## Structure

- **Entrypoints**: `src/main.rs` (binary REPL), `src/lib.rs` (library, used by tests + api.rs)
- **Crate names**: binary = `aten-ia`, lib = `memvid_agent_core` (for `use` statements)
- **Module decls** in `lib.rs` — 23 modules covering all subsystems
- **Model catalog**: `src/models_catalog.json` — 21 GGUF Q4_K_M models, loaded at runtime
- **Fine-tuning**: `src/finetune.rs` — `/finetune <lang>` builds a corpus from `/learn` knowledge, estimates RAM (~16 B/param AdamW full fine-tune + ~1.5 GB overhead) + time, then runs `llama-finetune` (`LLAMA_FINETUNE_BIN`/`config.finetune.binary_path`/`PATH`) or writes a portable run script. aten-ia never builds the finetune binary itself.
- **Config**: `config.json` — version 1, fields with serde defaults; first-run wizard creates if absent. `config.json` is git-ignored; `config.example.json` is the versioned template.
- **Persistence dir**: `~/.aten-ia/` by default (repo example uses `memvid_data/`) — `core.mv2`, `conversations/`, `knowledge/`, `knowledge_index.jsonl`, `manifest.json`, `feed_queue.jsonl`, `languages_catalog.json`, `.lock`
- **FFI**: `wrapper.h` → `llama-cpp-turboquant/include/llama.h` → bindgen → patched `unsafe extern "C"`

## Gotchas

- **Rust edition 2024** (min 1.89.0) — `rust-toolchain.toml` pins channel; install via `rustup`
- **`cargo clippy --lib`** only checks the library crate, not `main.rs`
- **`--test-threads=1`** required; parallel tests race on `MODEL_PATH` env var
- **First build** takes ~30 min (compiles llama.cpp from source); subsequent <1s if cached
- **First run** triggers interactive setup wizard (model select, API config, language docs)
- **`FileLock::acquire()`** creates `data_dir/.lock` with PID — concurrent instances rejected
- **`.env` loaded** before config via `dotenvy::dotenv().ok()`; optional
- **`rustfmt.toml`** exists — enforces consistent formatting across machines/versions

## Architecture

- **No GPU backends** — `n_gpu_layers` defaults to `0`; no CUDA/Metal/Vulkan cmake flags
- **RAG is keyword-only** — word substring match over `knowledge_index.jsonl`, no embeddings
- **RAG token budget** — dynamically limits RAG context based on available prompt budget (system + input tokens + 128-token margin subtracted first)
- **API threaded per connection** — raw TCP `TcpListener`, one thread per connection, inference serialized behind `Arc<Mutex<Agent>>`; SSE supported via `"stream": true` (single content chunk)
- **Session flushes every 5 interactions** — `Session::flush()` → `MemvidWriter` (also on `Drop`)
- **KV cache type** configurable: `model.kv_type_k` / `model.kv_type_v` (default `f16`; `turbo2/3/4` enable flash-attn automatically)
- **All persistence atomic**: write → fsync → rename → fsync(parent); sensitive files are `0600` and the data dir `0700` on Unix
- **License**: Apache 2.0, Copyright 2026 Dinámicas Electrónicas <dinamicas.electronicas@gmail.com>
- **Chunker UTF-8 safe** — `floor_char_boundary()` on overlap to avoid panics on multi-byte chars
- **`ingest <file>` auto-detects format** via `Format::from_extension()` (pdf/epub/md/html → text fallback)
- **Env overrides**: `MODEL_PATH`, `MODEL_NAME`, `MODEL_CTX`, `MODEL_URL` (applied on config load); `ATEN_THREADS` sets inference threads (default: all cores)
- **Default model**: `Qwen2.5-0.5B-Instruct` (`n_ctx: 8192`, `chat_template: chatml`, auto-downloads from HuggingFace)
- **llama.cpp verbose suppressed** at startup via `llama_log_set(noop_log)` in `context.rs`
- **Decode chunked** — `decode()` processes tokens in chunks of `n_batch` size to avoid GGML_ASSERT failures
- **`n_batch` = `min(2048, n_ctx)`** — kept below `n_ctx` to respect model internal limits
- **Only `--version`/`-V` is a CLI flag** — everything else is configured via `config.json` or `/commands`

## Debug Mode

Use `/debug` command in REPL to see RAG retrieval debug info after each query:
- Source file
- Relevance score
- Token count
- Content preview

## Testing

~541 unit + integration tests. Integration tests use `tempfile::tempdir()` + `LlamaContext::null()` — no GGUF model required:
- `tests/functional.rs`
- `tests/writer_integration.rs`
- `tests/kv_cache_and_history.rs`

## CI/CD

- **GitHub Actions**: `.github/workflows/ci.yml` + `.github/workflows/release.yml`
- **Gitea Actions**: `.gitea/workflows/ci.yml` + `.gitea/workflows/release.yml`
- Both CIs use same workflow content (Gitea Actions is compatible with GitHub Actions syntax)
- Gitea runner must run as root (no `sudo` needed in workflows)

## Git-ignored

`*.gguf`, `target/`, `memvid_data/`