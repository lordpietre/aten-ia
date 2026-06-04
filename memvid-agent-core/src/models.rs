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

    let spinner = indicatif::ProgressBar::new_spinner();
    spinner.set_style(
        indicatif::ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .expect("Invalid spinner template"),
    );
    spinner.set_message("Connecting to download model…");
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

    let resp = match ureq::get(url).call() {
        Ok(r) => r,
        Err(e) => {
            spinner.finish_with_message(format!("{} Download failed", "✗".red()));
            return Err(e).context("Failed to start model download");
        }
    };

    let total: u64 = resp
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    if total > 0 {
        spinner.finish_and_clear();
        let pb = indicatif::ProgressBar::new(total);
        pb.set_style(
            indicatif::ProgressStyle::default_bar()
                .template("{msg}\n[{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                .expect("Invalid progress bar template")
                .progress_chars("##-"),
        );
        pb.set_message(format!("Downloading {} ({:.1} MB)", model_config.name, total as f64 / 1_048_576.0));

        let mut out = std::fs::File::create(path).context("Failed to create model file")?;
        let mut downloaded: u64 = 0;
        let mut buf = [0u8; 65536];
        let mut reader = resp.into_body().into_reader();

        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    drop(out);
                    let _ = std::fs::remove_file(path);
                    pb.finish_with_message(format!("{} Download failed", "✗".red()));
                    return Err(e).context("Failed to read download stream");
                }
            };
            if let Err(e) = out.write_all(&buf[..n]) {
                drop(out);
                let _ = std::fs::remove_file(path);
                pb.finish_with_message(format!("{} Download failed", "✗".red()));
                return Err(e).context("Failed to write model file");
            }
            downloaded += n as u64;
            pb.set_position(downloaded);
        }

        out.sync_all().context("Failed to sync model file")?;
        drop(out);
        pb.finish_with_message(format!("{} Model downloaded", "✓".green()));
        eprintln!();
    } else {
        spinner.set_message(format!("Downloading {}…", model_config.name));
        let mut out = std::fs::File::create(path).context("Failed to create model file")?;
        let mut buf = [0u8; 65536];
        let mut reader = resp.into_body().into_reader();

        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    drop(out);
                    let _ = std::fs::remove_file(path);
                    spinner.finish_with_message(format!("{} Download failed", "✗".red()));
                    return Err(e).context("Failed to read download stream");
                }
            };
            if let Err(e) = out.write_all(&buf[..n]) {
                drop(out);
                let _ = std::fs::remove_file(path);
                spinner.finish_with_message(format!("{} Download failed", "✗".red()));
                return Err(e).context("Failed to write model file");
            }
        }

        out.sync_all().context("Failed to sync model file")?;
        drop(out);
        spinner.finish_with_message(format!("{} Model downloaded", "✓".green()));
        eprintln!();
    }

    Ok(())
}
