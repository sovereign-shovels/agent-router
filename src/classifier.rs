use crate::provider::ChatRequest;
use regex::Regex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Complexity {
    Simple,
    Medium,
    Complex,
}

pub struct Classifier {
    complex_keywords: Regex,
}

impl Default for Classifier {
    fn default() -> Self {
        Self::new()
    }
}

impl Classifier {
    pub fn new() -> Self {
        let complex_keywords = Regex::new(
            r"(?i)\b(refactor|architecture|debug|complex|optimize|performance|memory leak|race condition|deadlock|profil|benchmark|explain in detail|step by step reasoning|chain of thought|system design|design pattern)\b"
        ).expect("valid regex");
        Self { complex_keywords }
    }

    /// Rough token estimation: ~4 characters per token for English text
    pub fn estimate_tokens(text: &str) -> usize {
        text.len() / 4
    }

    pub fn classify(&self, req: &ChatRequest) -> Complexity {
        let total_text: String = req
            .messages
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join(" ");

        let token_count = Self::estimate_tokens(&total_text);
        let has_tool_calls = req.tools.is_some() && !req.tools.as_ref().unwrap().is_empty();
        let has_complex_keywords = self.complex_keywords.is_match(&total_text);

        if has_tool_calls || token_count > 2000 || has_complex_keywords {
            Complexity::Complex
        } else if token_count > 100 || has_complex_keywords {
            // Note: has_complex_keywords already handled above, but keep for clarity
            Complexity::Medium
        } else {
            Complexity::Simple
        }
    }

    pub fn classify_by_tokens_and_flags(
        &self,
        token_count: usize,
        has_tool_calls: bool,
        has_complex_keywords: bool,
    ) -> Complexity {
        if has_tool_calls || token_count > 2000 || has_complex_keywords {
            Complexity::Complex
        } else if token_count > 100 {
            Complexity::Medium
        } else {
            Complexity::Simple
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ChatMessage;

    #[test]
    fn test_simple_prompt() {
        let c = Classifier::new();
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![ChatMessage::user("hi")],
            temperature: None,
            max_tokens: None,
            tools: None,
            stream: false,
        };
        assert_eq!(c.classify(&req), Complexity::Simple);
    }

    #[test]
    fn test_medium_length() {
        let c = Classifier::new();
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![ChatMessage::user(&"a ".repeat(500))],
            temperature: None,
            max_tokens: None,
            tools: None,
            stream: false,
        };
        assert_eq!(c.classify(&req), Complexity::Medium);
    }

    #[test]
    fn test_complex_keyword() {
        let c = Classifier::new();
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![ChatMessage::user("refactor this codebase")],
            temperature: None,
            max_tokens: None,
            tools: None,
            stream: false,
        };
        assert_eq!(c.classify(&req), Complexity::Complex);
    }

    #[test]
    fn test_tool_calls() {
        let c = Classifier::new();
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![ChatMessage::user("call a tool")],
            temperature: None,
            max_tokens: None,
            tools: Some(vec![crate::provider::ToolDefinition {
                tool_type: "function".into(),
                function: crate::provider::ToolFunctionDef {
                    name: "test".into(),
                    description: None,
                    parameters: None,
                },
            }]),
            stream: false,
        };
        assert_eq!(c.classify(&req), Complexity::Complex);
    }

    #[test]
    fn test_complex_length() {
        let c = Classifier::new();
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![ChatMessage::user(&"a ".repeat(5000))],
            temperature: None,
            max_tokens: None,
            tools: None,
            stream: false,
        };
        assert_eq!(c.classify(&req), Complexity::Complex);
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(Classifier::estimate_tokens("hello world"), 2);
        assert_eq!(Classifier::estimate_tokens(&"a".repeat(400)), 100);
    }
}
