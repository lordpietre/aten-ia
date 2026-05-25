use crate::llama::context::LlamaContext;
use crate::memvid::writer::MemvidWriter;
use crate::types::{ConversationBatch, Message, MessageRole, WriterConfig};
use anyhow::Result;
use chrono::Utc;
use uuid::Uuid;

pub struct Agent {
    llm: LlamaContext,
    memory: MemvidWriter,
    model_name: String,
    interaction_count: u64,
    batch: Vec<Message>,
}

impl Agent {
    pub fn init(
        model_path: &str,
        model_name: &str,
        n_ctx: u32,
        writer_config: WriterConfig,
    ) -> Result<Self> {
        let llm = LlamaContext::init(model_path, n_ctx)?;
        let memory = MemvidWriter::init(writer_config)?;

        Ok(Self {
            llm,
            memory,
            model_name: model_name.to_string(),
            interaction_count: 0,
            batch: Vec::new(),
        })
    }

    pub fn chat(&mut self, user_input: &str) -> Result<String> {
        self.interaction_count += 1;

        let user_msg = Message {
            role: MessageRole::User,
            content: user_input.to_string(),
            timestamp: Utc::now(),
            tokens: None,
        };

        let response = self.llm.generate(user_input, 2048)?;
        let tokens = self.estimate_tokens(&response);

        let assistant_msg = Message {
            role: MessageRole::Assistant,
            content: response.clone(),
            timestamp: Utc::now(),
            tokens: Some(tokens),
        };

        self.batch.push(user_msg);
        self.batch.push(assistant_msg);

        // Flush every 5 interactions
        if self.interaction_count % 5 == 0 {
            self.flush_memory()?;
        }

        Ok(response)
    }

    fn flush_memory(&mut self) -> Result<()> {
        if self.batch.is_empty() {
            return Ok(());
        }

        let batch = ConversationBatch {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            messages: std::mem::take(&mut self.batch),
            model_used: self.model_name.clone(),
            tokens_used: 0,
        };

        self.memory.append_conversation(batch)?;
        Ok(())
    }

    fn estimate_tokens(&self, text: &str) -> u32 {
        (text.len() / 4) as u32
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        let _ = self.flush_memory();
    }
}
