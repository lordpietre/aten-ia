use anyhow::{Context, Result};
use colored::Colorize;
use std::io::{Read, Write};
use std::path::Path;

const DEFAULT_MODEL_URL: &str = "https://huggingface.co/bartowski/SmolLM2-360M-Instruct-GGUF/resolve/main/SmolLM2-360M-Instruct-Q4_K_M.gguf";

fn verify_checksum(path: &Path, expected: &str) -> Result<()> {
    let actual = crate::utils::compute_file_checksum(path)?;
    anyhow::ensure!(
        actual.eq_ignore_ascii_case(expected),
        "SHA-256 no coincide para {}: esperado {}, obtenido {}",
        path.display(),
        expected,
        actual
    );
    Ok(())
}

pub fn ensure_model(model_config: &crate::config::ModelConfig) -> Result<()> {
    let path = Path::new(&model_config.path);
    if path.exists() {
        if let Some(expected) = model_config.sha256.as_deref() {
            verify_checksum(path, expected)?;
        }
        return Ok(());
    }

    let url = model_config
        .download_url
        .as_deref()
        .unwrap_or(DEFAULT_MODEL_URL);
    let parent = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).context("No se pudo crear el directorio de modelos")?;

    let spinner = indicatif::ProgressBar::new_spinner();
    spinner.set_style(
        indicatif::ProgressStyle::default_spinner()
            .template("{spinner:.cyan} {msg}")
            .expect("Invalid spinner template"),
    );
    spinner.set_message("Conectando para descargar el modelo…");
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));

    let resp = match ureq::get(url).call() {
        Ok(r) => r,
        Err(e) => {
            spinner.finish_with_message(format!("{} Descarga fallida", "✗".red()));
            return Err(e).context("No se pudo iniciar la descarga del modelo");
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
        pb.set_message(format!(
            "Descargando {} ({:.1} MB)",
            model_config.name,
            total as f64 / 1_048_576.0
        ));

        let mut out =
            std::fs::File::create(path).context("No se pudo crear el archivo del modelo")?;
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
                    pb.finish_with_message(format!("{} Descarga fallida", "✗".red()));
                    return Err(e).context("No se pudo leer el flujo de descarga");
                }
            };
            if let Err(e) = out.write_all(&buf[..n]) {
                drop(out);
                let _ = std::fs::remove_file(path);
                pb.finish_with_message(format!("{} Descarga fallida", "✗".red()));
                return Err(e).context("No se pudo escribir el archivo del modelo");
            }
            downloaded += n as u64;
            pb.set_position(downloaded);
        }

        out.sync_all()
            .context("No se pudo sincronizar el archivo del modelo")?;
        drop(out);
        pb.finish_with_message(format!("{} Modelo descargado", "✓".green()));
        eprintln!();
    } else {
        spinner.set_message(format!("Descargando {}…", model_config.name));
        let mut out =
            std::fs::File::create(path).context("No se pudo crear el archivo del modelo")?;
        let mut buf = [0u8; 65536];
        let mut reader = resp.into_body().into_reader();

        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    drop(out);
                    let _ = std::fs::remove_file(path);
                    spinner.finish_with_message(format!("{} Descarga fallida", "✗".red()));
                    return Err(e).context("No se pudo leer el flujo de descarga");
                }
            };
            if let Err(e) = out.write_all(&buf[..n]) {
                drop(out);
                let _ = std::fs::remove_file(path);
                spinner.finish_with_message(format!("{} Descarga fallida", "✗".red()));
                return Err(e).context("No se pudo escribir el archivo del modelo");
            }
        }

        out.sync_all()
            .context("No se pudo sincronizar el archivo del modelo")?;
        drop(out);
        spinner.finish_with_message(format!("{} Modelo descargado", "✓".green()));
        eprintln!();
    }

    if let Some(expected) = model_config.sha256.as_deref() {
        if let Err(e) = verify_checksum(path, expected) {
            let _ = std::fs::remove_file(path);
            return Err(e);
        }
        eprintln!("  {} Checksum verificado", "✓".green());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ModelConfig;

    fn model_config(path: &Path, sha256: Option<&str>) -> ModelConfig {
        ModelConfig {
            path: path.to_string_lossy().to_string(),
            name: "test".to_string(),
            n_ctx: 2048,
            n_gpu_layers: 0,
            chat_template: "chatml".to_string(),
            kv_type_k: "f16".to_string(),
            kv_type_v: "f16".to_string(),
            download_url: None,
            sha256: sha256.map(|s| s.to_string()),
        }
    }

    #[test]
    fn ensure_model_accepts_existing_file_with_matching_checksum() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.gguf");
        std::fs::write(&path, b"fake gguf bytes").unwrap();
        let sha = crate::utils::sha256_digest(b"fake gguf bytes");

        let cfg = model_config(&path, Some(&sha));
        assert!(ensure_model(&cfg).is_ok());
    }

    #[test]
    fn ensure_model_rejects_existing_file_with_wrong_checksum() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("model.gguf");
        std::fs::write(&path, b"fake gguf bytes").unwrap();

        let cfg = model_config(&path, Some("deadbeef"));
        let err = ensure_model(&cfg).unwrap_err().to_string();
        assert!(err.contains("SHA-256 no coincide"), "{err}");
    }
}
