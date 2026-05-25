# AGENTS.md

Single package: `memvid-agent-core` (0.1.0), a Rust binary that runs an interactive AI agent CLI using local inference via llama.cpp.

The agent depends on the **published** `memvid-core` crate (v2.0.139, crates.io) for memory persistence in `.mv2` files, and bundles the `llama-cpp-turboquant` fork of llama.cpp for local LLM inference.

## Key commands

### memvid-agent-core (`memvid-agent-core/`)

```bash
cargo build                     # compiles llama.cpp via cmake+bindgen (build.rs)
MODEL_PATH=./model.gguf MODEL_NAME=my-model cargo run
```

First build compiles llama-cpp-turboquant (takes a while). The `build.rs`:
- Links static libs: `llama`, `llama-common`, `ggml-cpu`, `ggml`, `ggml-base`
- Links system deps: `stdc++`, `pthread`, `m`, `dl`, `gomp`
- Generates Rust FFI bindings via bindgen from `wrapper.h` → `llama_ffi.rs`
- Patches `extern "C"` → `unsafe extern "C"` for Rust 2024 edition

```bash
cargo test                     # 28 tests: 26 unit + 2 integration
cargo test -- --nocapture
```

System build deps: `build-essential`, `cmake`, `libssl-dev`, `clang`, `libgomp1`.

Runtime: `MODEL_PATH` env var (default: `models/llama-model.gguf`), `MODEL_NAME` (default: `llama-3.2-3b-tq`).

## Critical quirks

- **Rust edition 2024** — requires `unsafe extern "C"` in generated bindings (build.rs handles this patching). Minimum Rust: 1.95.0.
- **Atomic writes everywhere** — temp file + rename, never write in place.
- **`memvid-core` feature flags**: `tags` is `Vec<String>` with `"key=value"` format, not HashMap.
- llama-cpp-turboquant is built with tests/tools/examples/server all disabled in build.rs.

## Instruction files

- `memvid-agent-core/llama-cpp-turboquant/AGENTS.md` — strict policy: no AI-submitted PRs to upstream llama.cpp. Read before modifying that directory.
