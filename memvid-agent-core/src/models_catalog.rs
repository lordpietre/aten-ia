use crate::config;
use anyhow::{Context, Result};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    pub url: String,
    pub description: String,
    pub size_mb: u64,
    pub n_ctx_recommended: u32,
    pub sha256: Option<String>,
    pub chat_template: String,
}

static BUNDLED_CATALOG: &str = include_str!("models_catalog.json");

pub struct ModelsCatalog {
    entries: Vec<ModelEntry>,
}

impl ModelsCatalog {
    pub fn load() -> Self {
        let entries: Vec<ModelEntry> =
            serde_json::from_str(BUNDLED_CATALOG).expect("Invalid bundled models_catalog.json");
        Self { entries }
    }

    pub fn list(&self) -> &[ModelEntry] {
        &self.entries
    }

    pub fn find(&self, id: &str) -> Option<&ModelEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn download(entry: &ModelEntry, target_dir: &Path) -> Result<PathBuf> {
        let target_path = target_dir.join(format!("{}.gguf", entry.id));
        if target_path.exists() {
            eprintln!(
                "  {} Model already exists at {}",
                "✓".green(),
                target_path.display()
            );
            return Ok(target_path);
        }

        std::fs::create_dir_all(target_dir).context("Failed to create models directory")?;

        let spinner = indicatif::ProgressBar::new_spinner();
        spinner.set_style(
            indicatif::ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .expect("Invalid spinner template"),
        );
        spinner.set_message(format!("Connecting to download {}…", entry.name));
        spinner.enable_steady_tick(std::time::Duration::from_millis(80));

        let resp = match ureq::get(&entry.url).call() {
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

        spinner.finish_and_clear();

        if total > 0 {
            let pb = indicatif::ProgressBar::new(total);
            pb.set_style(
                indicatif::ProgressStyle::default_bar()
                    .template("{msg}\n[{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                    .expect("Invalid progress bar template")
                    .progress_chars("##-"),
            );
            pb.set_message(format!(
                "Downloading {} ({:.1} MB)",
                entry.name,
                total as f64 / 1_048_576.0
            ));

            let mut out =
                std::fs::File::create(&target_path).context("Failed to create model file")?;
            let mut downloaded: u64 = 0;
            let mut buf = [0u8; 65536];
            let mut reader = resp.into_body().into_reader();

            loop {
                let n = match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) => {
                        drop(out);
                        let _ = std::fs::remove_file(&target_path);
                        pb.finish_with_message(format!("{} Download failed", "✗".red()));
                        return Err(e).context("Failed to read download stream");
                    }
                };
                if let Err(e) = out.write_all(&buf[..n]) {
                    drop(out);
                    let _ = std::fs::remove_file(&target_path);
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
            let pb = indicatif::ProgressBar::new_spinner();
            pb.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .expect("Invalid spinner template"),
            );
            pb.set_message(format!("Downloading {}…", entry.name));
            pb.enable_steady_tick(std::time::Duration::from_millis(80));

            let mut out =
                std::fs::File::create(&target_path).context("Failed to create model file")?;
            let mut buf = [0u8; 65536];
            let mut reader = resp.into_body().into_reader();

            loop {
                let n = match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) => {
                        drop(out);
                        let _ = std::fs::remove_file(&target_path);
                        pb.finish_with_message(format!("{} Download failed", "✗".red()));
                        return Err(e).context("Failed to read download stream");
                    }
                };
                if let Err(e) = out.write_all(&buf[..n]) {
                    drop(out);
                    let _ = std::fs::remove_file(&target_path);
                    pb.finish_with_message(format!("{} Download failed", "✗".red()));
                    return Err(e).context("Failed to write model file");
                }
            }

            out.sync_all().context("Failed to sync model file")?;
            drop(out);
            pb.finish_with_message(format!("{} Model downloaded", "✓".green()));
            eprintln!();
        }

        if let Some(ref expected_sha) = entry.sha256 {
            let spinner = indicatif::ProgressBar::new_spinner();
            spinner.set_style(
                indicatif::ProgressStyle::default_spinner()
                    .template("{spinner:.cyan} {msg}")
                    .expect("Invalid spinner template"),
            );
            spinner.set_message("Verifying checksum…");
            spinner.enable_steady_tick(std::time::Duration::from_millis(80));

            let actual_sha = crate::utils::compute_file_checksum(&target_path)?;
            spinner.finish_and_clear();

            if &actual_sha != expected_sha {
                let _ = std::fs::remove_file(&target_path);
                anyhow::bail!(
                    "SHA-256 mismatch: expected {}, got {}",
                    expected_sha,
                    actual_sha
                );
            }
            eprintln!("  {} Checksum verified", "✓".green());
        }

        Ok(target_path)
    }
}

pub fn download_model(entry: &ModelEntry, target_dir: &Path) -> Result<PathBuf> {
    ModelsCatalog::download(entry, target_dir)
}

pub fn apply_model_to_config(
    model_path: &Path,
    entry: &ModelEntry,
    config: &mut config::Config,
) -> Result<()> {
    apply_model_to_config_with_path(
        model_path,
        entry,
        config,
        std::path::Path::new("config.json"),
    )
}

pub fn apply_model_to_config_with_path(
    model_path: &Path,
    entry: &ModelEntry,
    config: &mut config::Config,
    config_path: &Path,
) -> Result<()> {
    config.model.path = model_path.to_string_lossy().to_string();
    config.model.name = entry.name.clone();
    config.model.n_ctx = entry.n_ctx_recommended;
    config.model.chat_template = entry.chat_template.clone();
    config.model.download_url = Some(entry.url.clone());
    config.model.sha256 = entry.sha256.clone();
    config.save_to_path(config_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_loads() {
        let catalog = ModelsCatalog::load();
        assert!(!catalog.list().is_empty());
    }

    #[test]
    fn catalog_contains_smollm2() {
        let catalog = ModelsCatalog::load();
        let entry = catalog.find("smollm2-360m");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().chat_template, "chatml");
    }

    #[test]
    fn catalog_find_unknown() {
        let catalog = ModelsCatalog::load();
        assert!(catalog.find("nonexistent").is_none());
    }

    #[test]
    fn all_entries_have_required_fields() {
        let catalog = ModelsCatalog::load();
        for entry in catalog.list() {
            assert!(!entry.id.is_empty());
            assert!(!entry.name.is_empty());
            assert!(!entry.url.is_empty());
            assert!(!entry.description.is_empty());
            assert!(entry.size_mb > 0);
            assert!(entry.n_ctx_recommended > 0);
        }
    }

    #[test]
    fn apply_model_to_config_updates_fields() {
        let dir = tempfile::tempdir().unwrap();
        let mut cfg = config::Config::default();
        let catalog = ModelsCatalog::load();
        let entry = catalog.find("qwen2.5-0.5b").unwrap();
        let path = Path::new("models/qwen2.5-0.5b.gguf");
        let config_path = dir.path().join("config.json");

        apply_model_to_config_with_path(path, entry, &mut cfg, &config_path).unwrap();

        assert_eq!(cfg.model.path, "models/qwen2.5-0.5b.gguf");
        assert_eq!(cfg.model.name, entry.name);
        assert_eq!(cfg.model.n_ctx, entry.n_ctx_recommended);
        assert_eq!(cfg.model.chat_template, "chatml");
        assert!(config_path.exists());
    }
}
