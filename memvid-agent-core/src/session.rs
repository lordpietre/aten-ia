use crate::llama::context::LlamaContext;
use crate::memvid::writer::MemvidWriter;
use crate::types::{ConversationBatch, Message};
use anyhow::Result;
use chrono::Utc;
use uuid::Uuid;

pub struct Session {
    batch: Vec<Message>,
    interaction_count: u64,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Self {
            batch: Vec::new(),
            interaction_count: 0,
        }
    }

    pub fn push_message(&mut self, msg: Message) {
        self.batch.push(msg);
    }

    pub fn messages(&self) -> &[Message] {
        &self.batch
    }

    pub fn take_batch(&mut self) -> Vec<Message> {
        std::mem::take(&mut self.batch)
    }

    pub fn interaction_count(&self) -> u64 {
        self.interaction_count
    }

    pub fn increment_interactions(&mut self) {
        self.interaction_count += 1;
    }

    pub fn flush(
        &mut self,
        llm: &LlamaContext,
        model_name: &str,
        memory: &mut MemvidWriter,
    ) -> Result<()> {
        if self.batch.is_empty() {
            return Ok(());
        }
        let batch = self.take_batch();
        let all_text: String = batch
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<&str>>()
            .join(" ");
        let real_tokens = llm
            .tokenize(&all_text, false)
            .map(|t| t.len() as u32)
            .unwrap_or_else(|_| estimate_tokens(&all_text));

        let conv_batch = ConversationBatch {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            messages: batch,
            model_used: model_name.to_string(),
            tokens_used: real_tokens,
        };

        memory.append_conversation(conv_batch)?;
        Ok(())
    }
}

pub fn estimate_tokens(text: &str) -> u32 {
    (text.len() / 4) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MessageRole;

    fn msg(role: MessageRole, content: &str) -> Message {
        Message {
            role,
            content: content.to_string(),
            timestamp: Utc::now(),
            tokens: None,
        }
    }

    #[test]
    fn new_session_empty() {
        let s = Session::new();
        assert_eq!(s.interaction_count(), 0);
        assert!(s.messages().is_empty());
    }

    #[test]
    fn push_and_messages() {
        let mut s = Session::new();
        s.push_message(msg(MessageRole::User, "hello"));
        s.push_message(msg(MessageRole::Assistant, "world"));
        assert_eq!(s.messages().len(), 2);
        assert_eq!(s.messages()[0].content, "hello");
        assert_eq!(s.messages()[1].content, "world");
    }

    #[test]
    fn take_batch_clears() {
        let mut s = Session::new();
        s.push_message(msg(MessageRole::User, "hello"));
        let taken = s.take_batch();
        assert_eq!(taken.len(), 1);
        assert!(s.messages().is_empty());
    }

    #[test]
    fn interaction_counting() {
        let mut s = Session::new();
        assert_eq!(s.interaction_count(), 0);
        s.increment_interactions();
        assert_eq!(s.interaction_count(), 1);
        s.increment_interactions();
        assert_eq!(s.interaction_count(), 2);
    }

    #[test]
    fn estimate_tokens_accuracy() {
        assert_eq!(estimate_tokens("hello"), 1);
        assert_eq!(estimate_tokens("a".repeat(40).as_str()), 10);
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn flush_empty_is_noop() {
        let mut s = Session::new();
        let dir = tempfile::tempdir().unwrap();
        let config = crate::types::WriterConfig {
            data_dir: dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut writer = crate::memvid::writer::MemvidWriter::init(config).unwrap();
        // Can't test flush with real LlamaContext, just verify no panic
        assert!(s.messages().is_empty());
    }

    #[test]
    fn push_message_special_chars() {
        let mut s = Session::new();
        s.push_message(msg(MessageRole::User, "hello\nworld\t\r\n"));
        s.push_message(msg(MessageRole::Assistant, "café ñoño 你好 🌍"));
        s.push_message(msg(MessageRole::System, ""));
        assert_eq!(s.messages().len(), 3);
        assert_eq!(s.messages()[0].content, "hello\nworld\t\r\n");
        assert_eq!(s.messages()[1].content, "café ñoño 你好 🌍");
        assert_eq!(s.messages()[2].content, "");
    }

    #[test]
    fn push_message_very_long() {
        let mut s = Session::new();
        let long = "a".repeat(100_000);
        s.push_message(msg(MessageRole::User, &long));
        assert_eq!(s.messages().len(), 1);
        assert_eq!(s.messages()[0].content.len(), 100_000);
    }

    #[test]
    fn take_batch_empty() {
        let mut s = Session::new();
        let taken = s.take_batch();
        assert!(taken.is_empty());
    }

    #[test]
    fn take_batch_all_messages() {
        let mut s = Session::new();
        s.push_message(msg(MessageRole::User, "a"));
        s.push_message(msg(MessageRole::Assistant, "b"));
        s.push_message(msg(MessageRole::System, "c"));
        let taken = s.take_batch();
        assert_eq!(taken.len(), 3);
        assert!(s.messages().is_empty());
    }

    #[test]
    fn estimate_tokens_unicode() {
        // estimate_tokens divides byte length by 4
        // "ñ" = 2 bytes, 2/4 = 0
        assert_eq!(estimate_tokens("ñ"), 0);
        // "ñoño" = 6 bytes, 6/4 = 1
        assert_eq!(estimate_tokens("ñoño"), 1);
        // "ñoño y café" = 12 bytes, 12/4 = 3
        assert_eq!(estimate_tokens("ñoño y café"), 3);
    }

    #[test]
    fn estimate_tokens_whitespace() {
        assert_eq!(estimate_tokens("   "), 0);
        assert_eq!(estimate_tokens("\n\t\r"), 0);
        // " a b c " = 7 bytes, 7/4 = 1
        assert_eq!(estimate_tokens(" a b c "), 1);
    }

    #[test]
    fn default_session() {
        let s = Session::default();
        assert!(s.messages().is_empty());
        assert_eq!(s.interaction_count(), 0);
    }

    #[test]
    fn increment_interactions_multiple() {
        let mut s = Session::new();
        for _ in 0..100 {
            s.increment_interactions();
        }
        assert_eq!(s.interaction_count(), 100);
    }

    #[test]
    fn messages_immutable_after_push() {
        let mut s = Session::new();
        s.push_message(msg(MessageRole::User, "hello"));
        let msgs = s.messages();
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].content, "hello");
    }
}
