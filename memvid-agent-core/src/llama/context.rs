use anyhow::{Context, Result};
use std::ffi::CString;

// Include generated FFI bindings
include!(concat!(env!("OUT_DIR"), "/llama_ffi.rs"));

pub struct LlamaContext {
    model: *mut llama_model,
    ctx: *mut llama_context,
    vocab: *const llama_vocab,
    n_ctx: u32,
}

unsafe impl Send for LlamaContext {}

impl LlamaContext {
    pub fn init(model_path: &str, n_ctx: u32) -> Result<Self> {
        unsafe {
            llama_backend_init();

            let model_path_c = CString::new(model_path)
                .context("Invalid model path")?;

            let mut model_params = llama_model_default_params();
            model_params.n_gpu_layers = 99;

            let model = llama_model_load_from_file(
                model_path_c.as_ptr(),
                model_params,
            );
            if model.is_null() {
                anyhow::bail!("Failed to load model from {}", model_path);
            }

            let vocab = llama_model_get_vocab(model);
            if vocab.is_null() {
                llama_model_free(model);
                anyhow::bail!("Failed to get vocab");
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
            })
        }
    }

    pub fn tokenize(&self, text: &str, add_special: bool) -> Result<Vec<i32>> {
        unsafe {
            let text_c = CString::new(text)?;
            let n_tokens = llama_tokenize(
                self.vocab,
                text_c.as_ptr(),
                text.len() as i32,
                std::ptr::null_mut(),
                0,
                add_special,
                false,
            );

            let mut tokens: Vec<i32> = vec![0; n_tokens as usize];
            llama_tokenize(
                self.vocab,
                text_c.as_ptr(),
                text.len() as i32,
                tokens.as_mut_ptr(),
                n_tokens,
                add_special,
                false,
            );

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
        let mut tokens = self.tokenize(prompt, true)?;
        self.decode(&mut tokens)?;

        let mut output = String::new();

        for _ in 0..max_tokens {
            let token = self.sample(40, 0.95, 0.8)?;
            if self.is_eog(token) {
                break;
            }
            let piece = self.detokenize(token)?;
            output.push_str(&piece);

            let mut single = [token];
            self.decode(&mut single)?;
        }

        Ok(output)
    }

    pub fn n_ctx(&self) -> u32 {
        self.n_ctx
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
        }
    }
}
