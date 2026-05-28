use anyhow::{Context, Result};
use std::ffi::CString;
use std::sync::atomic::{AtomicI32, Ordering};

use super::ffi::*;

static BACKEND_REFCOUNT: AtomicI32 = AtomicI32::new(0);

pub struct LlamaContext {
    model: *mut llama_model,
    ctx: *mut llama_context,
    vocab: *const llama_vocab,
    n_ctx: u32,
    n_gpu_layers: u32,
    top_k: i32,
    top_p: f32,
    temp: f32,
}

unsafe impl Send for LlamaContext {}

impl LlamaContext {
    pub fn init(
        model_path: &str,
        n_ctx: u32,
        n_gpu_layers: u32,
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
            buffer.truncate(n.max(0) as usize);
            Ok(String::from_utf8_lossy(&buffer).to_string())
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
