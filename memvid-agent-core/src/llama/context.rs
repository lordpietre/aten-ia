use anyhow::{Context, Result};
use std::ffi::CString;
use std::sync::atomic::{AtomicI32, Ordering};

use super::ffi::*;

// bindgen prefixes C enum constants with the enum type name. Alias them back to
// their plain spelling so they read naturally and work as `match` patterns.
use super::ffi::{
    ggml_type_GGML_TYPE_BF16 as GGML_TYPE_BF16, ggml_type_GGML_TYPE_F16 as GGML_TYPE_F16,
    ggml_type_GGML_TYPE_F32 as GGML_TYPE_F32, ggml_type_GGML_TYPE_Q4_0 as GGML_TYPE_Q4_0,
    ggml_type_GGML_TYPE_Q8_0 as GGML_TYPE_Q8_0, ggml_type_GGML_TYPE_TURBO2_0 as GGML_TYPE_TURBO2_0,
    ggml_type_GGML_TYPE_TURBO3_0 as GGML_TYPE_TURBO3_0,
    ggml_type_GGML_TYPE_TURBO4_0 as GGML_TYPE_TURBO4_0,
    llama_flash_attn_type_LLAMA_FLASH_ATTN_TYPE_ENABLED as LLAMA_FLASH_ATTN_TYPE_ENABLED,
};

static BACKEND_REFCOUNT: AtomicI32 = AtomicI32::new(0);

pub struct LlamaContext {
    model: *mut llama_model,
    ctx: *mut llama_context,
    vocab: *const llama_vocab,
    n_ctx: u32,
    // Retained for introspection; GPU offload is currently disabled at build
    // time (see K4 in IMPROVEMENTS_PLAN.md), so nothing reads it yet.
    #[allow(dead_code)]
    n_gpu_layers: u32,
    top_k: i32,
    top_p: f32,
    temp: f32,
}

unsafe impl Send for LlamaContext {}

/// Resolve a KV-cache type name (from config) to a `ggml_type`.
///
/// Supports full-precision floats (`f32`, `f16`, `bf16`), legacy quantized
/// caches (`q8_0`, `q4_0`) and the bundled TurboQuant KV codecs
/// (`turbo2`/`turbo3`/`turbo4` = WHT-rotated polar quantization). Unknown
/// names fall back to `f16`.
pub fn kv_cache_ggml_type(name: &str) -> ggml_type {
    match name.trim().to_ascii_lowercase().as_str() {
        "f32" => GGML_TYPE_F32,
        "f16" => GGML_TYPE_F16,
        "bf16" => GGML_TYPE_BF16,
        "q8_0" => GGML_TYPE_Q8_0,
        "q4_0" => GGML_TYPE_Q4_0,
        "turbo2" | "turbo2_0" => GGML_TYPE_TURBO2_0,
        "turbo3" | "turbo3_0" => GGML_TYPE_TURBO3_0,
        "turbo4" | "turbo4_0" => GGML_TYPE_TURBO4_0,
        other => {
            tracing::warn!("Unknown kv cache type '{}', falling back to f16", other);
            GGML_TYPE_F16
        }
    }
}

/// Whether `name` is a KV-cache type the resolver maps to a real codec (as
/// opposed to falling back to f16). Used to validate the `/kv` REPL command so
/// typos are rejected instead of silently downgraded.
pub fn is_valid_kv_cache_type(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "f32"
            | "f16"
            | "bf16"
            | "q8_0"
            | "q4_0"
            | "turbo2"
            | "turbo2_0"
            | "turbo3"
            | "turbo3_0"
            | "turbo4"
            | "turbo4_0"
    )
}

/// Interpret `llama_token_to_piece`'s return value `n`: a non-negative value is
/// the number of bytes written; a negative value means the buffer was too small
/// and `-n` bytes are required. Pure, so it's unit-testable without a model.
fn piece_len_or_needed(n: i32) -> std::result::Result<usize, usize> {
    if n < 0 {
        Err((-n) as usize)
    } else {
        Ok(n as usize)
    }
}

/// True for KV-cache types that are not full-precision floats. llama.cpp
/// requires flash attention to be enabled for a quantized V cache, and the
/// TurboQuant fork additionally auto-enables it for the `turbo*` codecs.
fn kv_is_quantized(t: ggml_type) -> bool {
    !matches!(t, GGML_TYPE_F32 | GGML_TYPE_F16 | GGML_TYPE_BF16)
}

/// Coarse fidelity rank (higher = more precise). Only used to warn when a
/// config compresses K more aggressively than V, which the TurboQuant
/// "asymmetric K/V" guidance advises against ("V is free, K is everything").
fn kv_fidelity_rank(t: ggml_type) -> u8 {
    match t {
        GGML_TYPE_F32 => 32,
        GGML_TYPE_F16 | GGML_TYPE_BF16 => 16,
        GGML_TYPE_Q8_0 => 8,
        GGML_TYPE_TURBO4_0 => 5,
        GGML_TYPE_Q4_0 => 4,
        GGML_TYPE_TURBO3_0 => 3,
        GGML_TYPE_TURBO2_0 => 2,
        _ => 16,
    }
}

impl LlamaContext {
    #[allow(clippy::too_many_arguments)]
    pub fn init(
        model_path: &str,
        n_ctx: u32,
        n_gpu_layers: u32,
        kv_type_k: &str,
        kv_type_v: &str,
        top_k: i32,
        top_p: f32,
        temp: f32,
    ) -> Result<Self> {
        unsafe {
            let prev = BACKEND_REFCOUNT.fetch_add(1, Ordering::SeqCst);
            if prev == 0 {
                llama_backend_init();
            }

            let model_path_c = CString::new(model_path).context("Invalid model path")?;

            let mut model_params = llama_model_default_params();
            model_params.n_gpu_layers = n_gpu_layers as i32;

            let model = llama_model_load_from_file(model_path_c.as_ptr(), model_params);
            if model.is_null() {
                anyhow::bail!("Failed to load model from {}", model_path);
            }

            let vocab = llama_model_get_vocab(model);
            if vocab.is_null() {
                llama_model_free(model);
                anyhow::bail!("Failed to get vocab");
            }

            let n_vocab = llama_vocab_n_tokens(vocab);
            let vocab_type = llama_vocab_type(vocab);
            if n_vocab <= 0 {
                llama_model_free(model);
                anyhow::bail!(
                    "Model has empty vocab (n_tokens={}, type={})",
                    n_vocab,
                    vocab_type
                );
            }

            let mut ctx_params = llama_context_default_params();
            ctx_params.n_ctx = n_ctx;

            let kv_k = kv_cache_ggml_type(kv_type_k);
            let kv_v = kv_cache_ggml_type(kv_type_v);
            ctx_params.type_k = kv_k;
            ctx_params.type_v = kv_v;

            if kv_fidelity_rank(kv_k) < kv_fidelity_rank(kv_v) {
                tracing::warn!(
                    "KV cache: K ('{}') is more compressed than V ('{}'); TurboQuant \
                     guidance recommends keeping K at >= V precision",
                    kv_type_k,
                    kv_type_v
                );
            }

            // A quantized / TurboQuant KV cache requires flash attention in
            // llama.cpp (a plain-float cache does not). Enable it explicitly so
            // non-default configs don't error out at context init.
            if kv_is_quantized(kv_k) || kv_is_quantized(kv_v) {
                ctx_params.flash_attn_type = LLAMA_FLASH_ATTN_TYPE_ENABLED;
                tracing::info!(
                    "KV cache types active: K={}, V={} (flash attention enabled)",
                    kv_type_k,
                    kv_type_v
                );
            }

            let ctx = llama_init_from_model(model, ctx_params);
            if ctx.is_null() {
                llama_model_free(model);
                anyhow::bail!("Failed to create context");
            }

            Ok(Self {
                model,
                ctx,
                vocab,
                n_ctx,
                n_gpu_layers,
                top_k,
                top_p,
                temp,
            })
        }
    }

    pub fn tokenize(&self, text: &str, add_special: bool) -> Result<Vec<i32>> {
        unsafe {
            let text_c = CString::new(text)?;
            let mut tokens: Vec<i32> = vec![0; 8192];
            let n_tokens = llama_tokenize(
                self.vocab,
                text_c.as_ptr(),
                text.len() as i32,
                tokens.as_mut_ptr(),
                tokens.len() as i32,
                add_special,
                true,
            );

            if n_tokens < 0 {
                // n_tokens_max was too small; retry with correct size
                let needed = (-n_tokens) as usize;
                let mut tokens: Vec<i32> = vec![0; needed];
                let n_tokens = llama_tokenize(
                    self.vocab,
                    text_c.as_ptr(),
                    text.len() as i32,
                    tokens.as_mut_ptr(),
                    tokens.len() as i32,
                    add_special,
                    true,
                );
                if n_tokens <= 0 {
                    anyhow::bail!(
                        "llama_tokenize retry failed: returned {} for '{}'",
                        n_tokens,
                        text
                    );
                }
                tokens.truncate(n_tokens as usize);
                return Ok(tokens);
            }

            tokens.truncate(n_tokens as usize);
            Ok(tokens)
        }
    }

    pub fn detokenize(&self, token: i32) -> Result<String> {
        unsafe {
            let mut buffer: Vec<u8> = vec![0; 256];
            let n = llama_token_to_piece(
                self.vocab,
                token,
                buffer.as_mut_ptr() as *mut std::ffi::c_char,
                buffer.len() as i32,
                0,
                false,
            );

            match piece_len_or_needed(n) {
                Ok(len) => {
                    buffer.truncate(len);
                    Ok(String::from_utf8_lossy(&buffer).to_string())
                }
                Err(needed) => {
                    // Token piece was larger than 256 bytes; retry with the
                    // exact size llama.cpp reported instead of dropping it.
                    let mut buffer: Vec<u8> = vec![0; needed];
                    let n = llama_token_to_piece(
                        self.vocab,
                        token,
                        buffer.as_mut_ptr() as *mut std::ffi::c_char,
                        buffer.len() as i32,
                        0,
                        false,
                    );
                    match piece_len_or_needed(n) {
                        Ok(len) => {
                            buffer.truncate(len);
                            Ok(String::from_utf8_lossy(&buffer).to_string())
                        }
                        Err(_) => {
                            anyhow::bail!("llama_token_to_piece retry failed for token {}", token)
                        }
                    }
                }
            }
        }
    }

    pub fn decode(&mut self, tokens: &mut [i32]) -> Result<()> {
        if tokens.is_empty() {
            return Ok(());
        }
        unsafe {
            let batch = llama_batch_get_one(tokens.as_mut_ptr(), tokens.len() as i32);
            let ret = llama_decode(self.ctx, batch);
            if ret != 0 {
                anyhow::bail!("Failed to decode batch (ret={})", ret);
            }
            Ok(())
        }
    }

    pub fn sample(&self, top_k: i32, top_p: f32, temp: f32) -> Result<i32> {
        unsafe {
            let sparams = llama_sampler_chain_default_params();
            let smpl = llama_sampler_chain_init(sparams);

            if temp > 0.0 {
                let temp_sampler = llama_sampler_init_temp(temp);
                llama_sampler_chain_add(smpl, temp_sampler);
            }

            let top_p_sampler = llama_sampler_init_top_p(top_p, 1);
            llama_sampler_chain_add(smpl, top_p_sampler);

            if top_k > 0 {
                let top_k_sampler = llama_sampler_init_top_k(top_k);
                llama_sampler_chain_add(smpl, top_k_sampler);
            }

            let dist_sampler = llama_sampler_init_dist(0);
            llama_sampler_chain_add(smpl, dist_sampler);

            let token = llama_sampler_sample(smpl, self.ctx, -1);
            llama_sampler_free(smpl);
            Ok(token)
        }
    }

    pub fn is_eog(&self, token: i32) -> bool {
        unsafe { llama_vocab_is_eog(self.vocab, token) }
    }

    pub fn generate(&mut self, prompt: &str, max_tokens: u32) -> Result<String> {
        self.clear_memory();
        let mut tokens = self
            .tokenize(prompt, false)
            .context("Failed to tokenize prompt")?;
        self.decode(&mut tokens)
            .context("Failed to decode prompt")?;

        let mut output = String::new();

        for i in 0..max_tokens {
            let token = self
                .sample(self.top_k, self.top_p, self.temp)
                .context(format!("Failed to sample token at position {}", i))?;
            if self.is_eog(token) {
                break;
            }
            let piece = self
                .detokenize(token)
                .context(format!("Failed to detokenize token at position {}", i))?;
            output.push_str(&piece);

            let mut single = [token];
            self.decode(&mut single)
                .context(format!("Failed to decode token at position {}", i))?;
        }

        Ok(output)
    }

    pub fn n_ctx(&self) -> u32 {
        self.n_ctx
    }

    pub fn is_valid(&self) -> bool {
        !self.vocab.is_null()
    }

    pub fn clear_memory(&self) {
        unsafe {
            let mem = llama_get_memory(self.ctx);
            if !mem.is_null() {
                llama_memory_clear(mem, true);
            }
        }
    }
}

impl LlamaContext {
    /// Create a null-pointer LlamaContext for testing methods that don't
    /// call tokenize/generate. Drop is safe (checks null before free).
    pub fn null() -> Self {
        Self {
            model: std::ptr::null_mut(),
            ctx: std::ptr::null_mut(),
            vocab: std::ptr::null_mut(),
            n_ctx: 4096,
            n_gpu_layers: 0,
            top_k: 40,
            top_p: 0.95,
            temp: 0.8,
        }
    }
}

impl Drop for LlamaContext {
    fn drop(&mut self) {
        unsafe {
            if !self.ctx.is_null() {
                llama_free(self.ctx);
            }
            if !self.model.is_null() {
                llama_model_free(self.model);
            }
            let prev = BACKEND_REFCOUNT.fetch_sub(1, Ordering::SeqCst);
            if prev == 1 {
                llama_backend_free();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kv_cache_type_known_names() {
        assert_eq!(kv_cache_ggml_type("f16"), GGML_TYPE_F16);
        assert_eq!(kv_cache_ggml_type("F16"), GGML_TYPE_F16);
        assert_eq!(kv_cache_ggml_type(" f32 "), GGML_TYPE_F32);
        assert_eq!(kv_cache_ggml_type("q8_0"), GGML_TYPE_Q8_0);
        assert_eq!(kv_cache_ggml_type("turbo2"), GGML_TYPE_TURBO2_0);
        assert_eq!(kv_cache_ggml_type("turbo3"), GGML_TYPE_TURBO3_0);
        assert_eq!(kv_cache_ggml_type("turbo4"), GGML_TYPE_TURBO4_0);
    }

    #[test]
    fn kv_cache_type_unknown_falls_back_to_f16() {
        assert_eq!(kv_cache_ggml_type("nonsense"), GGML_TYPE_F16);
        assert_eq!(kv_cache_ggml_type(""), GGML_TYPE_F16);
    }

    #[test]
    fn kv_is_quantized_classifies_floats_and_codecs() {
        assert!(!kv_is_quantized(GGML_TYPE_F16));
        assert!(!kv_is_quantized(GGML_TYPE_F32));
        assert!(!kv_is_quantized(GGML_TYPE_BF16));
        assert!(kv_is_quantized(GGML_TYPE_Q4_0));
        assert!(kv_is_quantized(GGML_TYPE_TURBO2_0));
        assert!(kv_is_quantized(GGML_TYPE_TURBO4_0));
    }

    #[test]
    fn is_valid_kv_cache_type_accepts_known_rejects_unknown() {
        for name in ["f16", "F16", " f32 ", "q8_0", "turbo2", "turbo3", "turbo4"] {
            assert!(is_valid_kv_cache_type(name), "{name} should be valid");
        }
        for name in ["", "turbo5", "fp16", "nonsense", "q3_0"] {
            assert!(!is_valid_kv_cache_type(name), "{name} should be invalid");
        }
    }

    #[test]
    fn piece_len_or_needed_classifies_return_values() {
        assert_eq!(piece_len_or_needed(5), Ok(5));
        assert_eq!(piece_len_or_needed(0), Ok(0));
        assert_eq!(piece_len_or_needed(256), Ok(256));
        // Negative → buffer too small; need the absolute value of bytes.
        assert_eq!(piece_len_or_needed(-300), Err(300));
        assert_eq!(piece_len_or_needed(-1), Err(1));
    }

    #[test]
    fn kv_fidelity_rank_orders_k_above_v_for_recommended_config() {
        // Recommended asymmetric config: K=f16, V=turbo3 → K must rank higher.
        assert!(kv_fidelity_rank(GGML_TYPE_F16) > kv_fidelity_rank(GGML_TYPE_TURBO3_0));
        // Symmetric float config: equal rank (no warning).
        assert_eq!(
            kv_fidelity_rank(GGML_TYPE_F16),
            kv_fidelity_rank(GGML_TYPE_F16)
        );
        // turbo4 was "rehabilitated" to beat q4_0 on fidelity.
        assert!(kv_fidelity_rank(GGML_TYPE_TURBO4_0) > kv_fidelity_rank(GGML_TYPE_Q4_0));
    }
}
