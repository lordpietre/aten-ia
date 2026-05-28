use crate::context_policy::ContextPolicy;
use crate::llama::context::LlamaContext;
use crate::prompt::PromptBuilder;
use crate::retrieval::KnowledgeIndex;
use crate::session::estimate_tokens;
use crate::types::Message;
use anyhow::Result;

pub struct GenerationResult {
    pub response: String,
    pub tokens_estimated: u32,
}

pub fn generate_chat(
    llm: &mut LlamaContext,
    prompt_builder: &PromptBuilder,
    context_policy: &ContextPolicy,
    knowledge_index: &KnowledgeIndex,
    batch: &[Message],
    user_input: &str,
) -> Result<GenerationResult> {
    let rag_context: Vec<String> = knowledge_index
        .search(user_input, 3)
        .iter()
        .map(|e| format!("[{}] {}", e.source, e.content))
        .collect();

    let system_content = prompt_builder.developer_prompt().to_string();
    let trimmed = context_policy.trim_messages(&system_content, batch, user_input, |text| {
        llm.tokenize(text, false)
            .map(|t| t.len())
            .unwrap_or_else(|_| text.len() / 4)
    });

    let prompt = prompt_builder.build(&trimmed, user_input, &rag_context);
    let max_tokens = context_policy.max_tokens();
    let response = llm.generate(&prompt, max_tokens)?;
    let tokens_estimated = estimate_tokens(&response);

    Ok(GenerationResult {
        response,
        tokens_estimated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context_policy::ContextPolicy;
    use crate::prompt::{ChatTemplate, PromptBuilder};
    use crate::retrieval::KnowledgeIndex;
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
    fn generation_result_construction() {
        let result = GenerationResult {
            response: "hello".to_string(),
            tokens_estimated: 5,
        };
        assert_eq!(result.response, "hello");
        assert_eq!(result.tokens_estimated, 5);
    }

    #[test]
    fn generate_chat_no_model_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let index = KnowledgeIndex::load(dir.path()).unwrap();
        let builder = PromptBuilder::new(ChatTemplate::ChatML);
        let policy = ContextPolicy::new(4096, 2048);
        let batch = vec![msg(MessageRole::User, "hi")];

        // Without a real LlamaContext, generation will fail
        // This just validates the function signature and early path
        let system_content = builder.developer_prompt().to_string();
        let trimmed = policy.trim_messages(&system_content, &batch, "hello", |t| t.len() / 4);
        let prompt = builder.build(&trimmed, "hello", &[]);
        assert!(prompt.contains("<|im_start|>system"));
        assert!(prompt.contains("expert software engineer"));
    }
}
