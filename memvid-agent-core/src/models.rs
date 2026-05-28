use anyhow::{Context, Result};
use colored::Colorize;
use std::io::{Read, Write};
use std::path::Path;

const DEFAULT_MODEL_URL: &str = "https://huggingface.co/bartowski/SmolLM2-360M-Instruct-GGUF/resolve/main/SmolLM2-360M-Instruct-Q4_K_M.gguf";

pub fn ensure_model(model_config: &crate::config::ModelConfig) -> Result<()> {
    let path = Path::new(&model_config.path);
    if path.exists() {
        return Ok(());
    }

    let url = model_config
        .download_url
        .as_deref()
        .unwrap_or(DEFAULT_MODEL_URL);
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).context("Failed to create models directory")?;

    eprintln!(
        "{} Model not found at {}",
        "[↓]".yellow(),
        model_config.path
    );
    eprintln!("{} Downloading from:", "  src".dimmed());
    eprintln!("  {}", url);
    eprintln!();

    let resp = ureq::get(url)
        .call()
        .context("Failed to start model download")?;

    let total: u64 = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let pb = indicatif::ProgressBar::new(total);
    pb.set_style(
        indicatif::ProgressStyle::default_bar()
            .template("{msg}\n[{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .expect("Invalid progress bar template")
            .progress_chars("##-"),
    );
    pb.set_message("Downloading model…");

    let mut out = std::fs::File::create(path).context("Failed to create model file")?;
    let mut downloaded: u64 = 0;
    let mut buf = [0u8; 65536];
    let mut reader = resp.into_body().into_reader();

    loop {
        let n = reader
            .read(&mut buf)
            .context("Failed to read download stream")?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])
            .context("Failed to write model file")?;
        downloaded += n as u64;
        pb.set_position(downloaded);
    }

    pb.finish_with_message("✓ Model downloaded");
    eprintln!();
    Ok(())
}
