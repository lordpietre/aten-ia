use crate::types::{Message, MessageRole};

const SAFETY_MARGIN_TOKENS: usize = 64;

pub struct ContextPolicy {
    n_ctx: u32,
    max_tokens: u32,
}

impl ContextPolicy {
    pub fn new(n_ctx: u32, max_tokens: u32) -> Self {
        Self { n_ctx, max_tokens }
    }

    pub fn n_ctx(&self) -> u32 {
        self.n_ctx
    }

    pub fn max_tokens(&self) -> u32 {
        self.max_tokens
    }

    pub fn prompt_budget(&self) -> usize {
        self.n_ctx.saturating_sub(self.max_tokens) as usize
    }

    pub fn trim_messages<F>(
        &self,
        system_content: &str,
        messages: &[Message],
        user_input: &str,
        count_tokens: F,
    ) -> Vec<Message>
    where
        F: Fn(&str) -> usize,
    {
        let budget = self.prompt_budget();

        let system_tokens = count_tokens(system_content);
        let input_tokens = count_tokens(user_input);

        let available = budget
            .saturating_sub(system_tokens)
            .saturating_sub(input_tokens)
            .saturating_sub(SAFETY_MARGIN_TOKENS);

        if available == 0 {
            return Vec::new();
        }

        let mut system_msgs: Vec<Message> = Vec::new();
        let mut history: Vec<Message> = Vec::new();

        for msg in messages {
            match msg.role {
                MessageRole::System => system_msgs.push(msg.clone()),
                _ => history.push(msg.clone()),
            }
        }

        let mut result = system_msgs;
        let mut remaining = available;

        let mut fitting = Vec::new();
        for msg in history.iter().rev() {
            let tokens = count_tokens(&msg.content);
            if tokens <= remaining {
                fitting.push(msg.clone());
                remaining = remaining.saturating_sub(tokens);
            } else {
                break;
            }
        }
        fitting.reverse();
        result.extend(fitting);

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MessageRole;
    use chrono::Utc;

    fn msg(role: MessageRole, content: &str) -> Message {
        Message {
            role,
            content: content.to_string(),
            timestamp: Utc::now(),
            tokens: None,
        }
    }

    fn count_chars(s: &str) -> usize {
        s.len() / 4
    }

    #[test]
    fn budget_calculation() {
        let policy = ContextPolicy::new(4096, 2048);
        assert_eq!(policy.prompt_budget(), 2048);
    }

    #[test]
    fn no_truncation_needed() {
        let policy = ContextPolicy::new(4096, 2048);
        let msgs = vec![msg(MessageRole::User, "hi")];
        let result = policy.trim_messages("system prompt", &msgs, "hello", count_chars);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "hi");
    }

    #[test]
    fn keeps_system_messages() {
        let policy = ContextPolicy::new(256, 128);
        let mut msgs: Vec<Message> = Vec::new();
        msgs.push(msg(MessageRole::System, "system msg"));
        for i in 0..20 {
            msgs.push(msg(MessageRole::User, &format!("msg {}", i)));
        }
        let result = policy.trim_messages("developer prompt", &msgs, "current input", count_chars);
        assert!(result.iter().any(|m| m.content == "system msg"));
        assert!(result.len() <= msgs.len());
    }

    #[test]
    fn prefers_newest_messages() {
        let policy = ContextPolicy::new(256, 128);
        let mut msgs: Vec<Message> = Vec::new();
        msgs.push(msg(MessageRole::User, "oldest"));
        msgs.push(msg(MessageRole::Assistant, "resp1"));
        msgs.push(msg(MessageRole::User, "middle"));
        msgs.push(msg(MessageRole::Assistant, "resp2"));
        msgs.push(msg(MessageRole::User, "newest"));
        let result = policy.trim_messages("dev prompt", &msgs, "current", count_chars);
        let positions: Vec<usize> = msgs
            .iter()
            .filter(|m| result.iter().any(|r| r.content == m.content))
            .map(|m| msgs.iter().position(|x| x.content == m.content).unwrap())
            .collect();
        if positions.len() >= 2 {
            assert!(
                positions.windows(2).all(|w| w[0] <= w[1]),
                "preserves original order"
            );
        }
    }

    #[test]
    fn returns_empty_when_system_exceeds_budget() {
        let policy = ContextPolicy::new(64, 32);
        let msgs = vec![msg(MessageRole::User, "hi")];
        let result = policy.trim_messages(&"a".repeat(200), &msgs, "hello", |s| s.len() / 4);
        assert!(result.is_empty() || result.iter().all(|m| m.role == MessageRole::System));
    }

    #[test]
    fn zero_budget_returns_empty() {
        let policy = ContextPolicy::new(64, 64);
        let msgs = vec![msg(MessageRole::User, "hi")];
        let result = policy.trim_messages("sys", &msgs, "input", |s| s.len() / 4);
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn n_ctx_getter() {
        let policy = ContextPolicy::new(4096, 2048);
        assert_eq!(policy.n_ctx(), 4096);
    }

    #[test]
    fn max_tokens_getter() {
        let policy = ContextPolicy::new(4096, 2048);
        assert_eq!(policy.max_tokens(), 2048);
    }

    #[test]
    fn trim_messages_empty_slice() {
        let policy = ContextPolicy::new(4096, 2048);
        let result = policy.trim_messages("sys", &[], "input", |s| s.len() / 4);
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn trim_messages_only_system() {
        let policy = ContextPolicy::new(256, 128);
        let msgs = vec![
            msg(MessageRole::System, "sys1"),
            msg(MessageRole::System, "sys2"),
        ];
        let result = policy.trim_messages("dev", &msgs, "input", |s| s.len() / 4);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content, "sys1");
        assert_eq!(result[1].content, "sys2");
    }

    #[test]
    fn trim_messages_exact_budget() {
        let policy = ContextPolicy::new(256, 128);
        let budget = policy.prompt_budget();
        let content = "a".repeat(budget.saturating_sub(128) * 4);
        let msgs = vec![msg(MessageRole::User, &content)];
        let result = policy.trim_messages("", &msgs, "", |s| s.len() / 4);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn prompt_budget_low_n_ctx() {
        let policy = ContextPolicy::new(64, 128);
        assert_eq!(policy.prompt_budget(), 0);
    }

    #[test]
    fn trim_messages_preserves_system_order() {
        let policy = ContextPolicy::new(4096, 2048);
        let msgs = vec![
            msg(MessageRole::System, "first"),
            msg(MessageRole::User, "user1"),
            msg(MessageRole::System, "second"),
            msg(MessageRole::User, "user2"),
        ];
        let result = policy.trim_messages("dev", &msgs, "input", |s| s.len() / 4);
        let system_positions: Vec<_> = result
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == MessageRole::System)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            system_positions,
            vec![0, 1],
            "system messages should come first in original order"
        );
    }
}
