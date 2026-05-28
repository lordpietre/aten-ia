# AGENTS.md

Single Rust binary+lib crate — local LLM inference via bundled `llama-cpp-turboquant` CMake+bindgen, `.mv2` persistence (memvid-core v2), keyword RAG, OpenAI-compatible HTTP/1.1 API, multi-source ingestion.

## Commands (run from `memvid-agent-core/`)

| Action | Command |
|---|---|
| Build (slow first — cmake+llama.cpp) | `cargo build` |
| Run (auto-downloads model if missing) | `cargo run` |
| All tests | `cargo test` |
| Format check | `cargo fmt --all -- --check` |
| Lint (lib only — CI uses `--lib`) | `cargo clippy --lib` |
| System deps | `cmake libssl-dev clang libgomp1` |

CI order: `build → test → fmt → clippy --lib` (`.github/workflows/ci.yml`).

## Structure

- **Entrypoints**: `src/main.rs` (binary REPL), `src/lib.rs` (library, used by tests + api.rs)
- **Model catalog**: `src/models_catalog.json` — 20 models, loaded at runtime by `ModelsCatalog::load()`
- **Config**: `config.json` — version 1, fields: `model`, `generation`, `api`, `languages`, `ingestion` (serde defaults for missing fields)
- **Persistence dir**: `memvid_data/` — `.mv2` segments + `knowledge_index.jsonl` + `manifest.json` + `.lock`
- **11 `.expect()` calls in main.rs** — all in `ProgressStyle::template()` calls (spinners + progress bar)

## Key details

- **Rust edition 2024** (min 1.95.0) — bindgen generates `extern "C"` blocks, patched to `unsafe extern "C"` in `build.rs`
- **build.rs** links 5 static libs: `llama`, `llama-common`, `ggml-cpu`, `ggml`, `ggml-base` + system deps `stdc++`, `pthread`, `m`, `dl`, `gomp`
- **No GPU** — `n_gpu_layers = 0` hardcoded, no CUDA/Metal/Vulkan cmake flags
- **RAG is keyword-only** — word substring match over `knowledge_index.jsonl`, no embeddings
- **API single-threaded** — raw TCP `TcpListener`, sequential connections, no streaming SSE
- **Session flushes every 5 interactions** — `Session::flush()` → `MemvidWriter`
- **All persistence is atomic**: write → fsync → rename → fsync(parent)
- **`ingest <file>` auto-detects format** via `Format::from_extension()` (pdf/epub/md/html → text fallback)
- **Env overrides**: `MODEL_PATH`, `MODEL_NAME`, `MODEL_CTX`, `MODEL_URL` (applied on config load)
- **Default model** in `config.json`: `Qwen2.5-0.5B-Instruct` (`n_ctx: 8192`, `chat_template: chatml`)
- **Commands accept uppercase**: `/MODELS`, `/LOAD`, `/INGEST`, etc.
- **`Raw` chat template** ignores messages, history, and RAG context
- **`switch_model`** does not preserve `developer_mode`
- **`add_entries()` batch** rewrites full JSONL (only single `add_entry()` is O(1) append)
- **`llama-cpp-turboquant/AGENTS.md`**: no AI-submitted PRs to upstream llama.cpp
- **`FileLock::acquire()`** creates `data_dir/.lock` with PID — concurrent instances rejected
- **`.env` loaded** via `dotenvy::dotenv().ok()` at startup (before config load)
- **First run** triggers interactive setup wizard (model select, API config)
- **Integration tests** (`tests/`) use tempdir, no GGUF model needed — run via `cargo test`
- **`cargo clippy --lib`** only checks the library crate, not the binary (`main.rs`)
