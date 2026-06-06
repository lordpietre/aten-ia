//! Real full fine-tuning of the active model on knowledge gathered via `/learn`.
//!
//! The bundled llama.cpp fork ships `examples/training/finetune.cpp`, which does
//! *full* fine-tuning (every weight, AdamW optimizer) and writes a new GGUF the
//! size of the whole model — not a small LoRA adapter. That is RAM- and
//! compute-heavy, so before launching anything we estimate the cost and refuse
//! (unless forced) when it cannot fit in available memory.
//!
//! aten-ia does **not** build llama.cpp itself at runtime. Point
//! `LLAMA_FINETUNE_BIN` (or `config.finetune.binary_path`) at a prebuilt
//! `llama-finetune`. When the binary is present we run it; when it is absent we
//! still write the training corpus and a ready-to-run command so the actual
//! training can happen on a more capable machine (the estimate is honest about
//! when "this machine" is not that machine).

use crate::types::KnowledgeEntry;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Bytes per parameter for a Q4_K_M GGUF (~4.5 bits/param).
const Q4KM_BYTES_PER_PARAM: f64 = 0.5625;
/// AdamW full fine-tune keeps, per parameter, an f32 master weight, an f32
/// gradient and two f32 optimizer moments → 16 bytes.
const ADAMW_BYTES_PER_PARAM: u64 = 16;
/// Fixed runtime overhead (activations, KV cache forced to f32, framework
/// buffers). A coarse constant — real usage varies with `n_ctx`.
const BASE_OVERHEAD_BYTES: u64 = 1_500_000_000;
/// Coarse CPU training throughput, in tokens/second, per 10^9 parameters per
/// thread. Calibrated loosely so a ~0.5B model on 4 weak cores lands around
/// ~1.5 tok/s. This sets expectations; it is NOT a benchmark.
const TOK_PER_S_PER_BILLION_PER_THREAD: f64 = 0.2;
/// ~4 characters per token — the usual rule of thumb for English/code.
const CHARS_PER_TOKEN: u64 = 4;

const BYTES_PER_GB: f64 = 1_073_741_824.0;

/// Cost estimate for a full fine-tune run. All fields are derived; the struct is
/// the public surface the REPL prints and pre-flight-checks against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinetuneEstimate {
    pub token_count: u64,
    pub param_count: u64,
    pub epochs: u32,
    pub n_threads: u32,
    /// Peak RAM the run is expected to need.
    pub ram_bytes: u64,
    /// Estimated wall-clock seconds.
    pub seconds: u64,
}

impl FinetuneEstimate {
    pub fn ram_gb(&self) -> f64 {
        self.ram_bytes as f64 / BYTES_PER_GB
    }

    /// Does the run fit in `available_bytes` of RAM (with no safety margin)?
    pub fn fits_in_ram(&self, available_bytes: u64) -> bool {
        self.ram_bytes <= available_bytes
    }

    /// Human-friendly wall-clock string, e.g. "~3 h 20 min" or "~2,5 days".
    pub fn human_time(&self) -> String {
        let s = self.seconds;
        if s < 90 {
            format!("~{} s", s.max(1))
        } else if s < 5400 {
            format!("~{} min", s.div_ceil(60))
        } else if s < 86_400 {
            let h = s / 3600;
            let m = (s % 3600) / 60;
            if m == 0 {
                format!("~{} h", h)
            } else {
                format!("~{} h {} min", h, m)
            }
        } else {
            format!("~{:.1} days", s as f64 / 86_400.0)
        }
    }
}

/// Estimate parameter count from a Q4_K_M GGUF size in MB.
pub fn estimate_params_from_size_mb(size_mb: u64) -> u64 {
    let bytes = size_mb as f64 * 1_048_576.0;
    (bytes / Q4KM_BYTES_PER_PARAM) as u64
}

/// Approximate token count for a text using the ~4 chars/token rule.
pub fn approx_token_count(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(CHARS_PER_TOKEN)
}

/// Compute a [`FinetuneEstimate`] for a run. Pure and deterministic.
pub fn estimate(
    token_count: u64,
    param_count: u64,
    epochs: u32,
    n_threads: u32,
) -> FinetuneEstimate {
    let ram_bytes = param_count
        .saturating_mul(ADAMW_BYTES_PER_PARAM)
        .saturating_add(BASE_OVERHEAD_BYTES);

    let billions = (param_count as f64 / 1e9).max(0.01);
    let threads = n_threads.max(1) as f64;
    let tok_per_s = (TOK_PER_S_PER_BILLION_PER_THREAD * threads / billions).max(0.001);
    let total_tokens = token_count.saturating_mul(epochs.max(1) as u64);
    let seconds = (total_tokens as f64 / tok_per_s) as u64;

    FinetuneEstimate {
        token_count,
        param_count,
        epochs,
        n_threads,
        ram_bytes,
        seconds,
    }
}

/// Build the training corpus from knowledge entries that belong to `lang_key`
/// (i.e. whose `source` begins with `"{lang_key}/"`, the convention used by
/// `/learn`). Returns the concatenated text and the number of chunks included.
pub fn build_corpus(entries: &[KnowledgeEntry], lang_key: &str) -> (String, usize) {
    let prefix = format!("{}/", lang_key);
    let parts: Vec<&str> = entries
        .iter()
        .filter(|e| e.source.starts_with(&prefix))
        .map(|e| e.content.as_str())
        .collect();
    (parts.join("\n\n"), parts.len())
}

/// Available system RAM in bytes. Reads `/proc/meminfo` `MemAvailable` on Linux;
/// returns `None` where that is unavailable (callers then skip the hard check).
pub fn available_ram_bytes() -> Option<u64> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in meminfo.lines() {
        if let Some(rest) = line.strip_prefix("MemAvailable:") {
            let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb.saturating_mul(1024));
        }
    }
    None
}

/// Number of threads to suggest for training (all available cores).
pub fn suggested_threads() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(1)
}

/// Locate a `llama-finetune` binary: `LLAMA_FINETUNE_BIN` env, then the
/// configured path, then `llama-finetune`/`finetune` on `PATH`.
pub fn locate_binary(configured: Option<&str>) -> Option<PathBuf> {
    if let Ok(p) = std::env::var("LLAMA_FINETUNE_BIN") {
        let pb = PathBuf::from(p);
        if pb.is_file() {
            return Some(pb);
        }
    }
    if let Some(p) = configured {
        if !p.is_empty() {
            let pb = PathBuf::from(p);
            if pb.is_file() {
                return Some(pb);
            }
        }
    }
    for name in ["llama-finetune", "finetune"] {
        if let Some(pb) = which_in_path(name) {
            return Some(pb);
        }
    }
    None
}

fn which_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Assemble the `llama-finetune` argument vector (flags verified against the
/// bundled fork's `common/arg.cpp`).
pub fn build_command_args(
    model_path: &str,
    corpus_path: &Path,
    out_path: &Path,
    n_ctx: u32,
    n_threads: u32,
    epochs: u32,
) -> Vec<String> {
    vec![
        "-m".to_string(),
        model_path.to_string(),
        "-f".to_string(),
        corpus_path.to_string_lossy().to_string(),
        "-c".to_string(),
        n_ctx.to_string(),
        "-t".to_string(),
        n_threads.to_string(),
        "-epochs".to_string(),
        epochs.to_string(),
        "-opt".to_string(),
        "adamw".to_string(),
        "-o".to_string(),
        out_path.to_string_lossy().to_string(),
    ]
}

/// Render a portable shell command line from a binary and its args, quoting
/// arguments that contain whitespace so it is copy-paste safe.
pub fn render_command(binary: &str, args: &[String]) -> String {
    let mut out = String::from(binary);
    for a in args {
        out.push(' ');
        if a.is_empty() || a.contains(|c: char| c.is_whitespace()) {
            out.push('"');
            out.push_str(a);
            out.push('"');
        } else {
            out.push_str(a);
        }
    }
    out
}

/// Inputs for a fine-tune run, resolved by the caller from config + catalog.
pub struct FinetunePlan<'a> {
    pub lang_key: &'a str,
    pub lang_name: &'a str,
    pub model_path: &'a str,
    pub model_name: &'a str,
    pub param_count: u64,
    pub n_ctx: u32,
    pub epochs: u32,
    pub n_threads: u32,
    /// Directory where the corpus / script / fine-tuned model are written.
    pub output_dir: &'a Path,
    pub binary: Option<PathBuf>,
}

/// Outcome of [`prepare_and_run`].
pub enum FinetuneOutcome {
    /// The binary ran and produced a fine-tuned GGUF at this path.
    Trained(PathBuf),
    /// No binary available: corpus + run script were written for use elsewhere.
    Deferred { corpus: PathBuf, script: PathBuf },
}

/// Write the corpus, then either run `llama-finetune` (binary present) or emit a
/// ready-to-run script (binary absent). RAM/time estimation and confirmation are
/// the caller's responsibility — this performs the side effects.
pub fn prepare_and_run(corpus: &str, plan: &FinetunePlan) -> Result<FinetuneOutcome> {
    std::fs::create_dir_all(plan.output_dir)
        .with_context(|| format!("Failed to create {}", plan.output_dir.display()))?;

    let corpus_path = plan
        .output_dir
        .join(format!("{}_corpus.txt", plan.lang_key));
    crate::utils::atomic_write(&corpus_path, corpus.to_string())
        .context("Failed to write training corpus")?;

    let out_path = plan.output_dir.join(format!(
        "{}-{}.gguf",
        model_stem(plan.model_path),
        plan.lang_key
    ));

    let args = build_command_args(
        plan.model_path,
        &corpus_path,
        &out_path,
        plan.n_ctx,
        plan.n_threads,
        plan.epochs,
    );

    match &plan.binary {
        Some(bin) => {
            let status = std::process::Command::new(bin)
                .args(&args)
                .status()
                .with_context(|| format!("Failed to launch {}", bin.display()))?;
            if !status.success() {
                anyhow::bail!("llama-finetune exited with status {}", status);
            }
            if !out_path.is_file() {
                anyhow::bail!(
                    "llama-finetune reported success but {} was not created",
                    out_path.display()
                );
            }
            Ok(FinetuneOutcome::Trained(out_path))
        }
        None => {
            let bin_hint = "llama-finetune";
            let cmd = render_command(bin_hint, &args);
            let script = format!(
                "#!/usr/bin/env bash\n\
                 # Fine-tune {model} on {lang} docs gathered via aten-ia /learn.\n\
                 # Build llama-finetune from the bundled fork, then run this on a\n\
                 # machine with enough RAM (see the estimate aten-ia printed).\n\
                 #   cmake -B build -DLLAMA_BUILD_EXAMPLES=ON\n\
                 #   cmake --build build --target llama-finetune -j\n\
                 set -euo pipefail\n\
                 {cmd}\n",
                model = plan.model_name,
                lang = plan.lang_name,
                cmd = cmd,
            );
            let script_path = plan
                .output_dir
                .join(format!("finetune_{}.sh", plan.lang_key));
            crate::utils::atomic_write(&script_path, script)
                .context("Failed to write run script")?;
            Ok(FinetuneOutcome::Deferred {
                corpus: corpus_path,
                script: script_path,
            })
        }
    }
}

/// File stem of a model path, used to name the fine-tuned output.
fn model_stem(model_path: &str) -> String {
    Path::new(model_path)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "model".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn entry(source: &str, content: &str) -> KnowledgeEntry {
        KnowledgeEntry {
            id: "x".to_string(),
            source: source.to_string(),
            content: content.to_string(),
            timestamp: Utc::now(),
            checksum: "c".to_string(),
        }
    }

    #[test]
    fn corpus_filters_by_language_prefix() {
        let entries = vec![
            entry("rust/The Book", "fn main() {}"),
            entry("python/Tutorial", "print('hi')"),
            entry("rust/Async", "async fn f() {}"),
        ];
        let (corpus, n) = build_corpus(&entries, "rust");
        assert_eq!(n, 2);
        assert!(corpus.contains("fn main"));
        assert!(corpus.contains("async fn"));
        assert!(!corpus.contains("print"));
    }

    #[test]
    fn corpus_empty_for_unknown_language() {
        let entries = vec![entry("rust/x", "a")];
        let (corpus, n) = build_corpus(&entries, "haskell");
        assert_eq!(n, 0);
        assert!(corpus.is_empty());
    }

    #[test]
    fn token_count_rounds_up() {
        assert_eq!(approx_token_count(""), 0);
        assert_eq!(approx_token_count("abcd"), 1);
        assert_eq!(approx_token_count("abcde"), 2);
    }

    #[test]
    fn params_from_size_is_in_the_right_ballpark() {
        // SmolLM2-360M Q4_K_M is ~246 MB → a few hundred million params.
        let p = estimate_params_from_size_mb(246);
        assert!((300_000_000..=600_000_000).contains(&p), "got {p}");
    }

    #[test]
    fn ram_grows_with_params() {
        let small = estimate(100_000, 500_000_000, 3, 4);
        let big = estimate(100_000, 7_000_000_000, 3, 4);
        assert!(big.ram_bytes > small.ram_bytes);
        // 0.5B full fine-tune needs more than its 8 GB of optimizer state.
        assert!(small.ram_gb() > 8.0);
    }

    #[test]
    fn ram_preflight_rejects_oversized_run() {
        let est = estimate(200_000, 7_000_000_000, 3, 4);
        // 7B full fine-tune cannot fit in 8 GB.
        assert!(!est.fits_in_ram(8 * 1024 * 1024 * 1024));
    }

    #[test]
    fn time_scales_with_tokens_and_epochs() {
        let one = estimate(100_000, 500_000_000, 1, 4);
        let three = estimate(100_000, 500_000_000, 3, 4);
        assert!(three.seconds > one.seconds);
        assert!(three.seconds >= one.seconds * 2);
    }

    #[test]
    fn human_time_formats_ranges() {
        // A huge run on a single slow thread lands in the "days" bucket.
        let days = estimate(50_000_000, 7_000_000_000, 3, 1);
        assert!(days.human_time().contains("day"));
        // A tiny run reads as minutes/seconds, never days.
        let quick = estimate(200, 500_000_000, 1, 8);
        assert!(!quick.human_time().contains("day"));
    }

    #[test]
    fn command_args_have_expected_flags() {
        let args = build_command_args(
            "models/m.gguf",
            Path::new("out/c.txt"),
            Path::new("out/m-rust.gguf"),
            4096,
            4,
            3,
        );
        assert!(
            args.windows(2)
                .any(|w| w[0] == "-m" && w[1] == "models/m.gguf")
        );
        assert!(args.windows(2).any(|w| w[0] == "-epochs" && w[1] == "3"));
        assert!(args.windows(2).any(|w| w[0] == "-c" && w[1] == "4096"));
        assert!(args.iter().any(|a| a == "-o"));
    }

    #[test]
    fn render_command_quotes_whitespace() {
        let rendered = render_command("llama-finetune", &["-f".into(), "my corpus.txt".into()]);
        assert!(rendered.contains("\"my corpus.txt\""));
    }

    #[test]
    fn locate_binary_finds_configured_path() {
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("llama-finetune");
        std::fs::write(&fake, "#!/bin/sh\n").unwrap();
        let found = locate_binary(Some(fake.to_str().unwrap()));
        assert_eq!(found, Some(fake));
    }

    #[test]
    fn locate_binary_none_when_missing() {
        // A configured path that does not exist and (almost certainly) no
        // llama-finetune on PATH in CI.
        let found = locate_binary(Some("/nonexistent/llama-finetune-xyz"));
        // Can't assert None universally (a dev box might have it on PATH), but
        // the bogus configured path must not be returned.
        assert_ne!(
            found.as_deref(),
            Some(Path::new("/nonexistent/llama-finetune-xyz"))
        );
    }

    #[test]
    fn deferred_run_writes_corpus_and_script() {
        let dir = tempfile::tempdir().unwrap();
        let plan = FinetunePlan {
            lang_key: "rust",
            lang_name: "Rust",
            model_path: "models/qwen.gguf",
            model_name: "Qwen2.5-0.5B",
            param_count: 500_000_000,
            n_ctx: 4096,
            epochs: 3,
            n_threads: 4,
            output_dir: dir.path(),
            binary: None,
        };
        let outcome = prepare_and_run("some rust docs", &plan).unwrap();
        match outcome {
            FinetuneOutcome::Deferred { corpus, script } => {
                assert!(corpus.is_file());
                assert!(script.is_file());
                let s = std::fs::read_to_string(&script).unwrap();
                assert!(s.contains("llama-finetune"));
                assert!(s.contains("-epochs"));
            }
            _ => panic!("expected Deferred outcome without a binary"),
        }
    }
}
