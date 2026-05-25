# memvid-agent-core

Interactive AI agent CLI with local LLM inference via [llama.cpp](https://github.com/ggml-org/llama.cpp) (TurboQuant fork) and persistent memory via [memvid-core](https://crates.io/crates/memvid-core).

Conversations are persisted atomically in `.mv2` files (memvid-core format), indexed by a lightweight `manifest.json`.

## Quick start

### Prerequisites

- **Rust** ≥ 1.95.0 (edition 2024)
- **System deps**: `build-essential cmake libssl-dev clang libgomp1`
- A **GGUF model** file (e.g., [Llama 3.2 1B Instruct](https://huggingface.co/bartowski/Llama-3.2-1B-Instruct-GGUF))

### Build & run

```bash
cd memvid-agent-core

# First build compiles llama.cpp via cmake + bindgen (may take a while)
cargo build

# Run with a model:
MODEL_PATH=./models/llama-3.2-1b-instruct-q4.gguf \
MODEL_NAME=llama-3.2-1b \
cargo run
```

### Test

```bash
cd memvid-agent-core
cargo test            # 28 tests (26 unit + 2 integration)
```

## Architecture

```
User Input  →  agent.rs  →  llama/context.rs  →  llama.cpp (C FFI + TurboQuant)
                  │                                      │
                  │                                      ▼
                  │                               Response tokens
                  │                                      │
                  ▼                                      │
            ConversationBatch ◄──────────────────────────┘
                  │
                  ▼
            memvid/writer.rs
              ├─ memvid-core::Memvid::create() → .mv2 segment
              ├─ put_bytes_with_options() (tags: type, model, tokens)
              ├─ commit()
              └─ Atomic rename: temp → conv_YYYYMMDD_NNN.mv2
                  │
                  ▼
            memvid/playlist.rs
              ├─ Updates manifest.json (temp + rename)
              ├─ Backups manifest.json.bak
              ├─ Rolls segments at 50 MB
              └─ Creates core.mv2 with identity on first init
```

## Memory layout

```
memvid_data/
├── core.mv2                  # Agent identity & rules (created on first run)
├── manifest.json             # Lightweight index of all segments
├── conversations/            # Conversation segments (.mv2)
├── knowledge/                # Knowledge segments
└── archive/                  # Archived segments
```

- Auto-flush every 10 interactions (configurable)
- Segment rollover at 50 MB
- All writes are atomic (temp → rename)

## Environment variables

| Variable | Default | Description |
|---|---|---|
| `MODEL_PATH` | `models/llama-model.gguf` | Path to the GGUF model file |
| `MODEL_NAME` | `llama-3.2-3b-tq` | Model display name for metadata |

## Project structure

```
memvid-agent-core/
├── src/
│   ├── main.rs              # CLI entrypoint (REPL loop)
│   ├── lib.rs               # Public re-exports
│   ├── agent.rs             # Agent loop (chat, flush, Drop)
│   ├── types.rs             # Data types & WriterConfig
│   ├── utils.rs             # Atomic write, SHA-256 helpers
│   ├── llama/
│   │   ├── mod.rs
│   │   ├── context.rs       # Safe Rust wrapper over llama.cpp FFI
│   │   └── ffi.rs           # Auto-generated bindgen bindings
│   └── memvid/
│       ├── mod.rs
│       ├── manifest.rs      # manifest.json load/save
│       ├── playlist.rs      # Segment management & rolling
│       └── writer.rs        # .mv2 atomic writes via memvid-core
├── build.rs                 # Compiles llama.cpp + generates FFI bindings
├── wrapper.h                # #include "llama.h" for bindgen
├── llama-cpp-turboquant/    # llama.cpp fork source (TurboQuant)
├── tests/
│   └── writer_integration.rs
├── Cargo.toml
└── AGENTS.md                # AI assistant instructions
```

## llmama-cpp-turboquant

This repo bundles [TheTom/llama-cpp-turboquant](https://github.com/TheTom/llama-cpp-turboquant), a fork of llama.cpp with TurboQuant optimizations. See `memvid-agent-core/llama-cpp-turboquant/AGENTS.md` for contribution policies.

## License

Apache 2.0
