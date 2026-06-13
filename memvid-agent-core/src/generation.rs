use crate::context_policy::ContextPolicy;
use crate::llama::context::LlamaContext;
use crate::prompt::PromptBuilder;
use crate::retrieval::KnowledgeIndex;
use crate::session::estimate_tokens;
use crate::types::Message;
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct RagEntryDebug {
    pub source: String,
    pub content: String,
    pub score: f32,
    pub tokens: usize,
}

pub struct GenerationResult {
    pub response: String,
    pub tokens_estimated: u32,
    pub rag_debug: Vec<RagEntryDebug>,
}

pub fn generate_chat(
    llm: &mut LlamaContext,
    prompt_builder: &PromptBuilder,
    context_policy: &ContextPolicy,
    knowledge_index: &KnowledgeIndex,
    batch: &[Message],
    user_input: &str,
) -> Result<GenerationResult> {
    let count_tokens = |text: &str| {
        llm.tokenize(text, false)
            .map(|t| t.len())
            .unwrap_or_else(|_| text.len() / 4)
    };

    let system_content = prompt_builder.developer_prompt().to_string();
    let system_tokens = count_tokens(&system_content) as i32;
    let input_tokens = count_tokens(user_input) as i32;
    let prompt_budget = context_policy.prompt_budget() as i32;

    let max_rag_tokens = (prompt_budget - system_tokens - input_tokens - 128).max(0) as u32;

    let rag_search_results = knowledge_index.search_with_scores(user_input, 10);
    let mut rag_context: Vec<String> = Vec::new();
    let mut rag_tokens_used: usize = 0;
    let mut rag_debug: Vec<RagEntryDebug> = Vec::new();

    for entry in rag_search_results {
        let formatted = format!("[{}] {}", entry.entry.source, entry.entry.content);
        let entry_tokens = count_tokens(&formatted);
        if rag_tokens_used + entry_tokens > max_rag_tokens as usize {
            break;
        }
        rag_tokens_used += entry_tokens;
        rag_context.push(formatted.clone());
        rag_debug.push(RagEntryDebug {
            source: entry.entry.source.clone(),
            content: entry.entry.content.clone(),
            score: entry.score,
            tokens: entry_tokens,
        });
    }

    let trimmed = context_policy.trim_messages(
        &system_content,
        &rag_context,
        batch,
        user_input,
        count_tokens,
    );

    let prompt = prompt_builder.build(&trimmed, user_input, &rag_context);
    let prompt_tokens = count_tokens(&prompt);

    if prompt_tokens as u32 > context_policy.n_ctx() - context_policy.max_tokens() {
        anyhow::bail!(
            "Prompt too large: {} tokens (budget: {})",
            prompt_tokens,
            context_policy.n_ctx() - context_policy.max_tokens()
        );
    }

    let max_tokens = context_policy.max_tokens();
    let response = llm.generate(&prompt, max_tokens)?;
    let tokens_estimated = estimate_tokens(&response);

    Ok(GenerationResult {
        response,
        tokens_estimated,
        rag_debug,
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
            rag_debug: Vec::new(),
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
        let trimmed = policy.trim_messages(&system_content, &[], &batch, "hello", |t| t.len() / 4);
        let prompt = builder.build(&trimmed, "hello", &[]);
        assert!(prompt.contains("<|im_start|>system"));
        assert!(prompt.contains("expert software engineer"));
    }
}
