use crate::config::{Config, Provider};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Complexity {
    Simple,
    Medium,
    Complex,
}

#[derive(Debug, Clone)]
pub struct RouteDecision {
    pub provider: String,
    pub model: String,
    pub complexity: Complexity,
    pub estimated_cost: f64,
}

pub struct Router {
    config: Config,
}

impl Router {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn classify(&self, body: &Value) -> Complexity {
        let messages = body.get("messages").and_then(|m| m.as_array());
        let max_tokens = body
            .get("max_tokens")
            .or_else(|| body.get("max_completion_tokens"))
            .and_then(|t| t.as_u64())
            .unwrap_or(256) as usize;

        let total_chars = messages
            .map(|msgs| {
                msgs.iter()
                    .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
                    .map(|s| s.chars().count())
                    .sum::<usize>()
            })
            .unwrap_or(0);

        // Count complexity hints
        let hint_count = messages
            .map(|msgs| {
                msgs.iter()
                    .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
                    .map(|content| {
                        let lower = content.to_lowercase();
                        self.config
                            .routing
                            .complexity_hints
                            .iter()
                            .filter(|hint| lower.contains(&hint.to_lowercase()))
                            .count()
                    })
                    .sum::<usize>()
            })
            .unwrap_or(0);

        // Message count heuristic
        let msg_count = messages.map(|m| m.len()).unwrap_or(1);

        if max_tokens <= self.config.routing.simple_max_tokens
            && total_chars < 500
            && hint_count == 0
            && msg_count <= 2
        {
            return Complexity::Simple;
        }

        if max_tokens <= self.config.routing.medium_max_tokens
            && total_chars < 3000
            && hint_count <= 1
            && msg_count <= 5
        {
            return Complexity::Medium;
        }

        Complexity::Complex
    }

    pub fn route(&self, body: &Value) -> Option<RouteDecision> {
        let complexity = self.classify(body);
        let requested_model = body
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("default");

        // Sort providers by priority
        let mut providers: Vec<_> = self.config.providers.iter().collect();
        providers.sort_by_key(|(_, p)| p.priority);

        match complexity {
            Complexity::Simple => {
                // Try cheapest first (local Ollama)
                for (name, provider) in &providers {
                    if provider.models.iter().any(|m| m == requested_model)
                        || requested_model == "default"
                    {
                        let model = if requested_model == "default" {
                            provider.models.first()?.clone()
                        } else {
                            requested_model.to_string()
                        };
                        return Some(RouteDecision {
                            provider: name.to_string(),
                            model,
                            complexity,
                            estimated_cost: 0.0,
                        });
                    }
                }
            }
            Complexity::Medium => {
                // Try mid-tier (DeepSeek)
                for (name, provider) in &providers {
                    if provider.priority >= 2
                        && (provider.models.iter().any(|m| m == requested_model)
                            || requested_model == "default")
                    {
                        let model = if requested_model == "default" {
                            provider.models.first()?.clone()
                        } else {
                            requested_model.to_string()
                        };
                        let estimated_cost = self.estimate_cost(body, provider);
                        return Some(RouteDecision {
                            provider: name.to_string(),
                            model,
                            complexity,
                            estimated_cost,
                        });
                    }
                }
            }
            Complexity::Complex => {
                // Go for best quality (OpenRouter/Claude)
                for (name, provider) in providers.iter().rev() {
                    if provider.models.iter().any(|m| m == requested_model)
                        || requested_model == "default"
                    {
                        let model = if requested_model == "default" {
                            provider.models.first()?.clone()
                        } else {
                            requested_model.to_string()
                        };
                        let estimated_cost = self.estimate_cost(body, provider);
                        return Some(RouteDecision {
                            provider: name.to_string(),
                            model,
                            complexity,
                            estimated_cost,
                        });
                    }
                }
            }
        }

        // Fallback: first provider that has the model
        for (name, provider) in &providers {
            if provider.models.iter().any(|m| m == requested_model) || requested_model == "default" {
                let model = if requested_model == "default" {
                    provider.models.first()?.clone()
                } else {
                    requested_model.to_string()
                };
                let estimated_cost = self.estimate_cost(body, provider);
                return Some(RouteDecision {
                    provider: name.to_string(),
                    model,
                    complexity,
                    estimated_cost,
                });
            }
        }

        None
    }

    fn estimate_cost(&self, body: &Value, provider: &Provider) -> f64 {
        let input_chars = body
            .get("messages")
            .and_then(|m| m.as_array())
            .map(|msgs| {
                msgs.iter()
                    .filter_map(|m| m.get("content").and_then(|c| c.as_str()))
                    .map(|s| s.chars().count())
                    .sum::<usize>()
            })
            .unwrap_or(0);

        let max_tokens = body
            .get("max_tokens")
            .or_else(|| body.get("max_completion_tokens"))
            .and_then(|t| t.as_u64())
            .unwrap_or(256) as f64;

        // Rough estimate: ~4 chars per token
        let input_tokens = input_chars as f64 / 4.0;
        let output_tokens = max_tokens;

        (input_tokens / 1000.0) * provider.cost_per_1k_input
            + (output_tokens / 1000.0) * provider.cost_per_1k_output
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_config() -> Config {
        Config::default()
    }

    #[test]
    fn test_classify_simple() {
        let router = Router::new(test_config());
        let body = json!({
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 100
        });
        assert_eq!(router.classify(&body), Complexity::Simple);
    }

    #[test]
    fn test_classify_medium() {
        let router = Router::new(test_config());
        let body = json!({
            "messages": [{"role": "user", "content": "Explain Rust ownership"}],
            "max_tokens": 512
        });
        assert_eq!(router.classify(&body), Complexity::Medium);
    }

    #[test]
    fn test_classify_complex() {
        let router = Router::new(test_config());
        let body = json!({
            "messages": [
                {"role": "system", "content": "You are an expert."},
                {"role": "user", "content": "Refactor this large codebase to use async/await. Analyze the architecture and design a new system."}
            ],
            "max_tokens": 2048
        });
        assert_eq!(router.classify(&body), Complexity::Complex);
    }

    #[test]
    fn test_route_simple_to_ollama() {
        let router = Router::new(test_config());
        let body = json!({
            "messages": [{"role": "user", "content": "hi"}],
            "max_tokens": 100
        });
        let decision = router.route(&body).unwrap();
        assert_eq!(decision.provider, "ollama");
        assert_eq!(decision.complexity, Complexity::Simple);
        assert_eq!(decision.estimated_cost, 0.0);
    }

    #[test]
    fn test_route_complex_to_best() {
        let router = Router::new(test_config());
        let body = json!({
            "messages": [{"role": "user", "content": "Refactor this architecture"}],
            "max_tokens": 2048
        });
        let decision = router.route(&body).unwrap();
        assert_eq!(decision.complexity, Complexity::Complex);
        // Should route to highest priority (best quality)
        assert_eq!(decision.provider, "openrouter");
    }

    #[test]
    fn test_requested_model_respected() {
        let router = Router::new(test_config());
        let body = json!({
            "model": "deepseek-chat",
            "messages": [{"role": "user", "content": "hello"}],
            "max_tokens": 100
        });
        let decision = router.route(&body).unwrap();
        assert_eq!(decision.model, "deepseek-chat");
    }
}
