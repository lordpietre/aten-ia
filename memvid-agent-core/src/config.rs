use crate::types::IngestionConfig;
use crate::utils;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const CONFIG_FILENAME: &str = "config.json";
const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    pub data_dir: PathBuf,
    pub developer_mode: bool,
    pub developer_prompt: Option<String>,
    pub model: ModelConfig,
    pub generation: GenerationConfig,
    pub api: ApiConfig,
    pub languages: LanguagesConfig,
    #[serde(default)]
    pub ingestion: IngestionConfig,
    #[serde(default)]
    pub finetune: FinetuneConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub path: String,
    pub name: String,
    pub n_ctx: u32,
    pub n_gpu_layers: u32,
    pub chat_template: String,
    /// KV-cache data type for the K (keys) cache. Plain types (`f16`, `f32`,
    /// `bf16`, `q8_0`, `q4_0`) or TurboQuant codecs (`turbo2`/`turbo3`/`turbo4`).
    /// Defaults to `f16` (unchanged behavior). Keep K at >= V precision.
    #[serde(default = "default_kv_cache_type")]
    pub kv_type_k: String,
    /// KV-cache data type for the V (values) cache. See `kv_type_k`.
    /// V tolerates aggressive compression ("V is free"), so this is where
    /// TurboQuant codecs pay off. Defaults to `f16`.
    #[serde(default = "default_kv_cache_type")]
    pub kv_type_v: String,
    pub download_url: Option<String>,
    pub sha256: Option<String>,
}

fn default_kv_cache_type() -> String {
    "f16".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationConfig {
    pub top_k: i32,
    pub top_p: f32,
    pub temp: f32,
    pub max_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguagesConfig {
    pub installed: Vec<String>,
}

/// Full fine-tuning settings (see `finetune` module). All fields have serde
/// defaults so older `config.json` files (without a `finetune` section) load
/// unchanged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinetuneConfig {
    /// Path to a prebuilt `llama-finetune` binary. When unset, aten-ia falls
    /// back to `LLAMA_FINETUNE_BIN`, then `PATH`, then emits a run script.
    #[serde(default)]
    pub binary_path: Option<String>,
    /// Optimizer epochs (passed as `-epochs`).
    #[serde(default = "default_finetune_epochs")]
    pub epochs: u32,
    /// Directory where corpus, run script and fine-tuned model are written.
    #[serde(default = "default_finetune_output_dir")]
    pub output_dir: String,
}

fn default_finetune_epochs() -> u32 {
    3
}

fn default_finetune_output_dir() -> String {
    "models/finetuned".to_string()
}

impl Default for FinetuneConfig {
    fn default() -> Self {
        Self {
            binary_path: None,
            epochs: default_finetune_epochs(),
            output_dir: default_finetune_output_dir(),
        }
    }
}

impl LanguagesConfig {
    pub fn mark_installed(&mut self, key: &str) {
        if !self.installed.iter().any(|s| s == key) {
            self.installed.push(key.to_string());
        }
    }

    pub fn mark_uninstalled(&mut self, key: &str) {
        self.installed.retain(|s| s != key);
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            data_dir: PathBuf::from("memvid_data"),
            developer_mode: true,
            developer_prompt: None,
            model: ModelConfig {
                path: "models/qwen2.5-0.5b.gguf".to_string(),
                name: "Qwen2.5-0.5B-Instruct".to_string(),
                n_ctx: 8192,
                n_gpu_layers: 0,
                chat_template: "chatml".to_string(),
                kv_type_k: default_kv_cache_type(),
                kv_type_v: default_kv_cache_type(),
                download_url: Some("https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/qwen2.5-0.5b-instruct-q4_k_m.gguf".to_string()),
                sha256: None,
            },
            generation: GenerationConfig {
                top_k: 40,
                top_p: 0.95,
                temp: 0.8,
                max_tokens: 2048,
            },
            api: ApiConfig {
                enabled: false,
                host: "127.0.0.1".to_string(),
                port: 8787,
                token: None,
            },
            languages: LanguagesConfig {
                installed: Vec::new(),
            },
            ingestion: IngestionConfig::default(),
            finetune: FinetuneConfig::default(),
        }
    }
}

impl Config {
    pub fn save(&self) -> Result<()> {
        self.save_to_path(CONFIG_FILENAME)
    }

    pub fn save_to_path<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let content = serde_json::to_string_pretty(self)?;
        utils::atomic_write(path, content)?;
        Ok(())
    }

    pub fn load_or_create() -> Result<Self> {
        Self::load_or_create_with_path(CONFIG_FILENAME)
    }

    pub fn load_or_create_with_path<P: AsRef<Path>>(config_path: P) -> Result<Self> {
        let config_path = config_path.as_ref();
        let mut config = if config_path.exists() {
            let content = std::fs::read_to_string(config_path)
                .with_context(|| format!("Failed to read {}", config_path.display()))?;
            let cfg: Config =
                serde_json::from_str(&content).context("Failed to parse config.json")?;
            cfg
        } else {
            let cfg = Self::default();
            cfg.save_to_path(config_path)?;
            eprintln!("Created default {}", config_path.display());
            cfg
        };

        config.apply_env_overrides();
        config.ensure_data_dir()?;

        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(self.model.n_ctx > 0, "model.n_ctx must be > 0");
        anyhow::ensure!(
            self.generation.max_tokens > 0,
            "generation.max_tokens must be > 0"
        );
        anyhow::ensure!(
            self.generation.temp >= 0.0,
            "generation.temp must be >= 0.0"
        );
        anyhow::ensure!(
            self.generation.top_p >= 0.0 && self.generation.top_p <= 1.0,
            "generation.top_p must be between 0.0 and 1.0"
        );
        anyhow::ensure!(self.generation.top_k >= 0, "generation.top_k must be >= 0");
        anyhow::ensure!(self.api.port > 0, "api.port must be > 0");
        anyhow::ensure!(
            crate::llama::context::is_valid_kv_cache_type(&self.model.kv_type_k),
            "model.kv_type_k '{}' is not a valid KV cache type",
            self.model.kv_type_k
        );
        anyhow::ensure!(
            crate::llama::context::is_valid_kv_cache_type(&self.model.kv_type_v),
            "model.kv_type_v '{}' is not a valid KV cache type",
            self.model.kv_type_v
        );
        Ok(())
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(val) = std::env::var("MODEL_PATH") {
            self.model.path = val;
        }
        if let Ok(val) = std::env::var("MODEL_NAME") {
            self.model.name = val;
        }
        if let Ok(val) = std::env::var("MODEL_CTX")
            && let Ok(n) = val.parse()
        {
            self.model.n_ctx = n;
        }
        if let Ok(val) = std::env::var("MODEL_URL") {
            self.model.download_url = Some(val);
        }
    }

    fn ensure_data_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.data_dir).with_context(|| {
            format!(
                "Failed to create data directory: {}",
                self.data_dir.display()
            )
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.version, 1);
        assert_eq!(cfg.model.path, "models/qwen2.5-0.5b.gguf");
        assert_eq!(cfg.model.n_ctx, 8192);
        assert_eq!(cfg.model.n_gpu_layers, 0);
        assert_eq!(cfg.model.chat_template, "chatml");
        assert_eq!(cfg.model.kv_type_k, "f16");
        assert_eq!(cfg.model.kv_type_v, "f16");
        assert_eq!(cfg.generation.top_k, 40);
        assert_eq!(cfg.generation.top_p, 0.95);
        assert!(cfg.generation.temp - 0.8 < f32::EPSILON);
        assert_eq!(cfg.generation.max_tokens, 2048);
        assert!(!cfg.api.enabled);
        assert_eq!(cfg.api.port, 8787);
        assert!(cfg.languages.installed.is_empty());
        assert!(cfg.developer_prompt.is_none());
    }

    #[test]
    fn config_roundtrip() {
        let cfg = Config::default();
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let deserialized: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.model.path, cfg.model.path);
        assert_eq!(deserialized.model.n_ctx, cfg.model.n_ctx);
        assert_eq!(
            deserialized.generation.max_tokens,
            cfg.generation.max_tokens
        );
    }

    #[test]
    fn config_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");

        let cfg = Config::default();
        cfg.save_to_path(&config_path).unwrap();
        assert!(config_path.exists());

        let loaded = Config::load_or_create_with_path(&config_path).unwrap();
        assert_eq!(loaded.model.path, cfg.model.path);
        assert_eq!(loaded.model.n_ctx, cfg.model.n_ctx);
    }

    #[test]
    fn kv_cache_types_default_to_f16_when_absent() {
        // An older config.json without the kv_type_* fields must still load,
        // defaulting both caches to f16 (i.e. unchanged behavior).
        let json = r#"{
            "version": 1,
            "data_dir": "memvid_data",
            "developer_mode": true,
            "developer_prompt": null,
            "model": {
                "path": "m.gguf",
                "name": "m",
                "n_ctx": 4096,
                "n_gpu_layers": 0,
                "chat_template": "chatml",
                "download_url": null,
                "sha256": null
            },
            "generation": { "top_k": 40, "top_p": 0.95, "temp": 0.8, "max_tokens": 2048 },
            "api": { "enabled": false, "host": "127.0.0.1", "port": 8787, "token": null },
            "languages": { "installed": [] }
        }"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.model.kv_type_k, "f16");
        assert_eq!(cfg.model.kv_type_v, "f16");
    }

    #[test]
    fn kv_cache_types_roundtrip() {
        let mut cfg = Config::default();
        cfg.model.kv_type_k = "f16".to_string();
        cfg.model.kv_type_v = "turbo3".to_string();
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.model.kv_type_k, "f16");
        assert_eq!(back.model.kv_type_v, "turbo3");
    }

    #[test]
    fn validate_rejects_zero_n_ctx() {
        let mut cfg = Config::default();
        cfg.model.n_ctx = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_negative_temp() {
        let mut cfg = Config::default();
        cfg.generation.temp = -1.0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn load_or_create_creates_default_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");

        assert!(!config_path.exists());
        let cfg = Config::load_or_create_with_path(&config_path).unwrap();
        assert!(config_path.exists());
        assert_eq!(cfg.version, 1);
    }

    fn with_env_var(key: &str, val: &str, f: impl FnOnce()) {
        let old = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, val);
        }
        f();
        match old {
            Some(v) => unsafe {
                std::env::set_var(key, v);
            },
            None => unsafe {
                std::env::remove_var(key);
            },
        }
    }

    #[test]
    fn env_var_override_model_path() {
        with_env_var("MODEL_PATH", "/custom/path.gguf", || {
            let dir = tempfile::tempdir().unwrap();
            let config_path = dir.path().join("config.json");
            let cfg = Config::load_or_create_with_path(&config_path).unwrap();
            assert_eq!(cfg.model.path, "/custom/path.gguf");
        });
    }

    #[test]
    fn env_var_override_model_name() {
        with_env_var("MODEL_NAME", "test-model", || {
            let mut cfg = Config::default();
            cfg.apply_env_overrides();
            assert_eq!(cfg.model.name, "test-model");
        });
    }

    #[test]
    fn env_var_override_model_ctx() {
        with_env_var("MODEL_CTX", "8192", || {
            let mut cfg = Config::default();
            cfg.apply_env_overrides();
            assert_eq!(cfg.model.n_ctx, 8192);
        });
    }

    #[test]
    fn env_var_override_model_url() {
        with_env_var("MODEL_URL", "https://example.com/model.gguf", || {
            let mut cfg = Config::default();
            cfg.apply_env_overrides();
            assert_eq!(
                cfg.model.download_url,
                Some("https://example.com/model.gguf".to_string())
            );
        });
    }

    #[test]
    fn env_var_unset_does_not_override() {
        let mut cfg = Config::default();
        cfg.apply_env_overrides();
        assert_eq!(cfg.model.path, "models/qwen2.5-0.5b.gguf");
        assert_eq!(cfg.model.name, "Qwen2.5-0.5B-Instruct");
    }

    #[test]
    fn validate_rejects_zero_max_tokens() {
        let mut cfg = Config::default();
        cfg.generation.max_tokens = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_zero_api_port() {
        let mut cfg = Config::default();
        cfg.api.port = 0;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_accepts_zero_temp() {
        let mut cfg = Config::default();
        cfg.generation.temp = 0.0;
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn validate_rejects_top_p_out_of_range() {
        let mut cfg = Config::default();
        cfg.generation.top_p = 1.5;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_negative_top_k() {
        let mut cfg = Config::default();
        cfg.generation.top_k = -1;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_invalid_kv_type_k() {
        let mut cfg = Config::default();
        cfg.model.kv_type_k = "invalid".to_string();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_rejects_invalid_kv_type_v() {
        let mut cfg = Config::default();
        cfg.model.kv_type_v = "invalid".to_string();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_accepts_valid_kv_types() {
        let mut cfg = Config::default();
        cfg.model.kv_type_k = "f16".to_string();
        cfg.model.kv_type_v = "turbo3".to_string();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn mark_installed_adds_key() {
        let mut lang = LanguagesConfig {
            installed: Vec::new(),
        };
        lang.mark_installed("rust");
        assert_eq!(lang.installed.len(), 1);
        assert!(lang.installed.contains(&"rust".to_string()));
    }

    #[test]
    fn mark_installed_idempotent() {
        let mut lang = LanguagesConfig {
            installed: vec!["rust".to_string()],
        };
        lang.mark_installed("rust");
        assert_eq!(lang.installed.len(), 1);
    }

    #[test]
    fn mark_installed_empty_key() {
        let mut lang = LanguagesConfig {
            installed: Vec::new(),
        };
        lang.mark_installed("");
        assert_eq!(lang.installed.len(), 1);
        assert!(lang.installed.contains(&"".to_string()));
    }

    #[test]
    fn env_var_model_ctx_non_numeric_ignored() {
        with_env_var("MODEL_CTX", "not-a-number", || {
            let mut cfg = Config::default();
            cfg.apply_env_overrides();
            assert_eq!(cfg.model.n_ctx, 8192);
        });
    }

    #[test]
    fn save_method_works() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let cfg = Config::default();
        cfg.save_to_path(&config_path).unwrap();
        assert!(config_path.exists());
        let loaded = Config::load_or_create_with_path(&config_path).unwrap();
        assert_eq!(loaded.model.path, cfg.model.path);
    }

    #[test]
    fn load_or_create_with_corrupted_json() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        std::fs::write(&config_path, "not valid json").unwrap();
        let result = Config::load_or_create_with_path(&config_path);
        assert!(result.is_err());
    }

    #[test]
    fn config_without_finetune_section_loads_with_defaults() {
        // A v1 config.json that predates the `finetune` section must still load.
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.json");
        let cfg = Config::default();
        let mut json = serde_json::to_value(&cfg).unwrap();
        json.as_object_mut().unwrap().remove("finetune");
        std::fs::write(&config_path, serde_json::to_string(&json).unwrap()).unwrap();

        let loaded = Config::load_or_create_with_path(&config_path).unwrap();
        assert_eq!(loaded.finetune.epochs, 3);
        assert_eq!(loaded.finetune.output_dir, "models/finetuned");
        assert!(loaded.finetune.binary_path.is_none());
    }
}
