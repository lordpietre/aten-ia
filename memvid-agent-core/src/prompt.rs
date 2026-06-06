use crate::types::{Message, MessageRole};

pub const DEFAULT_DEVELOPER_PROMPT: &str = "You are an expert software engineer. You help users write, debug, and understand code. You are fluent in all programming languages. If the user asks about a specific language, provide examples and explanations.";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChatTemplate {
    ChatML,
    Llama3,
    Mistral,
    Raw,
}

impl ChatTemplate {
    // Inherent `from_str` (not the `FromStr` trait) on purpose: it's infallible
    // (unknown → `Raw`) so a `Result`-returning trait impl would be misleading.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().trim() {
            "chatml" => ChatTemplate::ChatML,
            "llama3" => ChatTemplate::Llama3,
            "mistral" => ChatTemplate::Mistral,
            _ => ChatTemplate::Raw,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ChatTemplate::ChatML => "chatml",
            ChatTemplate::Llama3 => "llama3",
            ChatTemplate::Mistral => "mistral",
            ChatTemplate::Raw => "raw",
        }
    }
}

pub struct PromptBuilder {
    template: ChatTemplate,
    developer_prompt: String,
}

impl PromptBuilder {
    pub fn new(template: ChatTemplate) -> Self {
        Self {
            template,
            developer_prompt: DEFAULT_DEVELOPER_PROMPT.to_string(),
        }
    }

    pub fn with_developer_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.developer_prompt = prompt.into();
        self
    }

    /// Return a builder for a different chat template while preserving the
    /// current developer prompt. Used by `switch_model` so that switching the
    /// model does not silently reset the developer prompt to the default.
    pub fn with_template(&self, template: ChatTemplate) -> Self {
        Self {
            template,
            developer_prompt: self.developer_prompt.clone(),
        }
    }

    pub fn developer_prompt(&self) -> &str {
        &self.developer_prompt
    }

    pub fn template(&self) -> ChatTemplate {
        self.template
    }

    pub fn build(&self, messages: &[Message], user_input: &str, rag_context: &[String]) -> String {
        match self.template {
            ChatTemplate::ChatML => self.build_chatml(messages, user_input, rag_context),
            ChatTemplate::Llama3 => self.build_llama3(messages, user_input, rag_context),
            ChatTemplate::Mistral => self.build_mistral(messages, user_input, rag_context),
            ChatTemplate::Raw => self.build_raw(messages, user_input, rag_context),
        }
    }

    fn system_content(&self, rag_context: &[String]) -> String {
        let mut content = self.developer_prompt.clone();
        if !rag_context.is_empty() {
            content.push_str("\n\n## Relevant context:\n");
            for ctx in rag_context {
                content.push_str(ctx);
                content.push('\n');
            }
        }
        content
    }

    fn build_chatml(
        &self,
        messages: &[Message],
        user_input: &str,
        rag_context: &[String],
    ) -> String {
        let mut prompt = String::new();

        let system = self.system_content(rag_context);
        prompt.push_str(&format!("<|im_start|>system\n{}\n<|im_end|>\n", system));

        for msg in messages {
            let role = match msg.role {
                crate::types::MessageRole::System => "system",
                crate::types::MessageRole::User => "user",
                crate::types::MessageRole::Assistant => "assistant",
                crate::types::MessageRole::Tool => "tool",
            };
            prompt.push_str(&format!(
                "<|im_start|>{}\n{}\n<|im_end|>\n",
                role, msg.content
            ));
        }

        prompt.push_str(&format!("<|im_start|>user\n{}\n<|im_end|>\n", user_input));
        prompt.push_str("<|im_start|>assistant\n");
        prompt
    }

    fn build_llama3(
        &self,
        messages: &[Message],
        user_input: &str,
        rag_context: &[String],
    ) -> String {
        let mut prompt = String::from("<|begin_of_text|>");

        let system = self.system_content(rag_context);
        prompt.push_str(&format!(
            "<|start_header_id|>system<|end_header_id|>\n\n{}\n<|eot_id|>",
            system
        ));

        for msg in messages {
            let role = match msg.role {
                crate::types::MessageRole::System => "system",
                crate::types::MessageRole::User => "user",
                crate::types::MessageRole::Assistant => "assistant",
                crate::types::MessageRole::Tool => "ipython",
            };
            prompt.push_str(&format!(
                "<|start_header_id|>{}<|end_header_id|>\n\n{}\n<|eot_id|>",
                role, msg.content
            ));
        }

        prompt.push_str(&format!(
            "<|start_header_id|>user<|end_header_id|>\n\n{}\n<|eot_id|>",
            user_input
        ));
        prompt.push_str("<|start_header_id|>assistant<|end_header_id|>\n\n");
        prompt
    }

    fn build_mistral(
        &self,
        messages: &[Message],
        user_input: &str,
        rag_context: &[String],
    ) -> String {
        let mut prompt = String::new();

        let system = self.system_content(rag_context);
        let system_prefix = if system.is_empty() {
            String::new()
        } else {
            format!("{}\n\n", system)
        };

        let mut first_user = true;
        for msg in messages {
            match msg.role {
                crate::types::MessageRole::User => {
                    if first_user {
                        prompt.push_str(&format!(
                            "[INST] {}{} [/INST]\n",
                            system_prefix, msg.content
                        ));
                        first_user = false;
                    } else {
                        prompt.push_str(&format!("[INST] {} [/INST]\n", msg.content));
                    }
                }
                crate::types::MessageRole::Assistant => {
                    prompt.push_str(&format!("{} </s>", msg.content));
                }
                crate::types::MessageRole::System => {
                    if first_user {
                        prompt.push_str(&format!("[INST] {} [/INST]\n", msg.content));
                    }
                }
                crate::types::MessageRole::Tool => {}
            }
        }

        if !user_input.is_empty()
            && !messages
                .iter()
                .any(|m| m.role == MessageRole::User && m.content == user_input)
        {
            prompt.push_str(&format!("[INST] {}{} [/INST]\n", system_prefix, user_input));
        }

        prompt
    }

    fn build_raw(
        &self,
        _messages: &[Message],
        user_input: &str,
        _rag_context: &[String],
    ) -> String {
        user_input.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Message, MessageRole};
    use chrono::Utc;

    fn msg(role: MessageRole, content: &str) -> Message {
        Message {
            role,
            content: content.to_string(),
            timestamp: Utc::now(),
            tokens: None,
        }
    }

    #[test]
    fn chatml_basic() {
        let builder = PromptBuilder::new(ChatTemplate::ChatML);
        let result = builder.build(&[], "hello", &[]);
        assert!(result.contains("<|im_start|>system"));
        assert!(result.contains("expert software engineer"));
        assert!(result.contains("<|im_start|>user\nhello\n<|im_end|>"));
        assert!(result.contains("<|im_start|>assistant\n"));
        assert!(result.ends_with("<|im_start|>assistant\n"));
    }

    #[test]
    fn chatml_with_history() {
        let builder = PromptBuilder::new(ChatTemplate::ChatML);
        let messages = vec![
            msg(MessageRole::User, "hi"),
            msg(MessageRole::Assistant, "hello there"),
        ];
        let result = builder.build(&messages, "how are you?", &[]);
        assert!(result.contains("<|im_start|>user\nhi\n<|im_end|>"));
        assert!(result.contains("<|im_start|>assistant\nhello there\n<|im_end|>"));
        assert!(result.contains("<|im_start|>user\nhow are you?\n<|im_end|>"));
    }

    #[test]
    fn chatml_with_rag_context() {
        let builder = PromptBuilder::new(ChatTemplate::ChatML);
        let result = builder.build(&[], "question", &["some docs here".to_string()]);
        assert!(result.contains("## Relevant context"));
        assert!(result.contains("some docs here"));
    }

    #[test]
    fn chatml_with_system_in_history() {
        let builder = PromptBuilder::new(ChatTemplate::ChatML);
        let messages = vec![msg(MessageRole::System, "user loaded file x.py")];
        let result = builder.build(&messages, "explain it", &[]);
        assert!(result.contains("<|im_start|>system\nuser loaded file x.py"));
    }

    #[test]
    fn llama3_template() {
        let builder = PromptBuilder::new(ChatTemplate::Llama3);
        let result = builder.build(&[], "hello", &[]);
        assert!(result.starts_with("<|begin_of_text|>"));
        assert!(result.contains("<|start_header_id|>system<|end_header_id|>"));
        assert!(result.contains("<|start_header_id|>user<|end_header_id|>\n\nhello"));
        assert!(result.contains("<|start_header_id|>assistant<|end_header_id|>"));
    }

    #[test]
    fn mistral_template() {
        let builder = PromptBuilder::new(ChatTemplate::Mistral);
        let result = builder.build(&[], "hello", &[]);
        assert!(result.starts_with("[INST]"));
        assert!(result.contains("expert software engineer"));
        assert!(result.contains("hello"));
        assert!(result.contains("[/INST]"));
    }

    #[test]
    fn mistral_template_with_messages() {
        let builder = PromptBuilder::new(ChatTemplate::Mistral);
        let messages = vec![
            msg(MessageRole::User, "hi"),
            msg(MessageRole::Assistant, "hello there"),
        ];
        let result = builder.build(&messages, "how are you?", &[]);
        assert!(result.contains("[INST]"));
        assert!(result.contains("hi"));
        assert!(result.contains("hello there"));
        assert!(result.contains("how are you?"));
    }

    #[test]
    fn mistral_template_with_system_message() {
        let builder = PromptBuilder::new(ChatTemplate::Mistral);
        let messages = vec![msg(MessageRole::System, "important context")];
        let result = builder.build(&messages, "question", &[]);
        assert!(result.contains("[INST] important context [/INST]"));
    }

    #[test]
    fn raw_template() {
        let builder = PromptBuilder::new(ChatTemplate::Raw);
        let result = builder.build(&[], "hello world", &[]);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn template_from_str() {
        assert_eq!(ChatTemplate::from_str("chatml"), ChatTemplate::ChatML);
        assert_eq!(ChatTemplate::from_str("CHATML"), ChatTemplate::ChatML);
        assert_eq!(ChatTemplate::from_str("llama3"), ChatTemplate::Llama3);
        assert_eq!(ChatTemplate::from_str("mistral"), ChatTemplate::Mistral);
        assert_eq!(ChatTemplate::from_str("unknown"), ChatTemplate::Raw);
    }

    #[test]
    fn custom_developer_prompt() {
        let builder =
            PromptBuilder::new(ChatTemplate::ChatML).with_developer_prompt("You are a poet.");
        let result = builder.build(&[], "write a poem", &[]);
        assert!(result.contains("You are a poet."));
        assert!(!result.contains("expert software engineer"));
    }

    #[test]
    fn with_template_preserves_developer_prompt() {
        // Regression: switching templates (as `switch_model` does) must keep
        // the developer prompt, not reset it to the default.
        let original =
            PromptBuilder::new(ChatTemplate::ChatML).with_developer_prompt("You are a poet.");
        let switched = original.with_template(ChatTemplate::Llama3);
        assert_eq!(switched.developer_prompt(), "You are a poet.");
        assert_eq!(switched.template(), ChatTemplate::Llama3);
    }

    #[test]
    fn with_template_preserves_empty_developer_prompt() {
        // developer_mode = false stores an empty developer prompt; that empty
        // state must also survive a template switch.
        let original = PromptBuilder::new(ChatTemplate::ChatML).with_developer_prompt("");
        let switched = original.with_template(ChatTemplate::Mistral);
        assert_eq!(switched.developer_prompt(), "");
    }
}
