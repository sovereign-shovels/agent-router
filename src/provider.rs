use reqwest;
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub tool_type: Option<String>,
    pub function: Option<ToolFunction>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolFunction {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<ToolDefinition>>,
    #[serde(default)]
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: ToolFunctionDef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolFunctionDef {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChatResponse {
    pub id: Option<String>,
    pub object: Option<String>,
    pub created: Option<u64>,
    pub model: Option<String>,
    pub choices: Option<Vec<Choice>>,
    pub usage: Option<Usage>,
    pub error: Option<ApiError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Choice {
    pub index: Option<u32>,
    pub message: Option<MessageContent>,
    pub delta: Option<MessageContent>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageContent {
    pub role: Option<String>,
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApiError {
    pub message: Option<String>,
    #[serde(rename = "type")]
    pub error_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub name: String,
    pub endpoint: String,
    pub api_key_env: Option<String>,
    pub model: String,
    pub cost_input_per_1m: f64,
    pub cost_output_per_1m: f64,
    pub max_prompt_tokens: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Provider {
    pub config: ProviderConfig,
    client: reqwest::Client,
}

impl Provider {
    pub fn new(config: ProviderConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    pub fn api_key(&self) -> Option<String> {
        self.config
            .api_key_env
            .as_ref()
            .and_then(|k| env::var(k).ok())
    }

    pub fn build_request_body(&self, req: &ChatRequest) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": req.model,
            "messages": req.messages,
            "stream": req.stream,
        });
        if let Some(temp) = req.temperature {
            body["temperature"] = serde_json::json!(temp);
        }
        if let Some(max) = req.max_tokens {
            body["max_tokens"] = serde_json::json!(max);
        }
        if let Some(tools) = &req.tools {
            body["tools"] = serde_json::json!(tools);
        }
        body
    }

    pub async fn chat(&self, req: &ChatRequest) -> Result<ChatResponse, Box<dyn std::error::Error + Send + Sync>> {
        let body = self.build_request_body(req);
        let url = format!("{}/chat/completions", self.config.endpoint);
        let mut builder = self
            .client
            .post(&url)
            .header("Content-Type", "application/json");
        if let Some(key) = self.api_key() {
            builder = builder.header("Authorization", format!("Bearer {}", key));
        }
        let res = builder.json(&body).send().await?;
        let status = res.status();
        if !status.is_success() {
            let text = res.text().await?;
            return Err(format!("Provider {} error ({}): {}", self.config.name, status, text).into());
        }
        let chat_res: ChatResponse = res.json().await?;
        Ok(chat_res)
    }

    pub async fn chat_stream(
        &self,
        req: &ChatRequest,
    ) -> Result<reqwest::Response, Box<dyn std::error::Error + Send + Sync>> {
        let mut body = self.build_request_body(req);
        body["stream"] = serde_json::json!(true);
        let url = format!("{}/chat/completions", self.config.endpoint);
        let mut builder = self
            .client
            .post(&url)
            .header("Content-Type", "application/json");
        if let Some(key) = self.api_key() {
            builder = builder.header("Authorization", format!("Bearer {}", key));
        }
        let res = builder.json(&body).send().await?;
        let status = res.status();
        if !status.is_success() {
            let text = res.text().await?;
            return Err(format!("Provider {} error ({}): {}", self.config.name, status, text).into());
        }
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_message_helpers() {
        let m = ChatMessage::user("hello");
        assert_eq!(m.role, "user");
        assert_eq!(m.content, "hello");
    }

    #[test]
    fn test_provider_api_key_from_env() {
        let p = Provider::new(ProviderConfig {
            name: "test".into(),
            endpoint: "http://localhost".into(),
            api_key_env: Some("NONEXISTENT_KEY_XYZ".into()),
            model: "m".into(),
            cost_input_per_1m: 0.0,
            cost_output_per_1m: 0.0,
            max_prompt_tokens: None,
        });
        assert!(p.api_key().is_none());
    }

    #[test]
    fn test_build_request_body() {
        let p = Provider::new(ProviderConfig {
            name: "test".into(),
            endpoint: "http://localhost".into(),
            api_key_env: None,
            model: "m".into(),
            cost_input_per_1m: 0.0,
            cost_output_per_1m: 0.0,
            max_prompt_tokens: None,
        });
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![ChatMessage::user("hi")],
            temperature: Some(0.5),
            max_tokens: Some(100),
            tools: None,
            stream: false,
        };
        let body = p.build_request_body(&req);
        assert_eq!(body["model"], "m");
        assert_eq!(body["temperature"], 0.5);
        assert_eq!(body["stream"], false);
    }
}
