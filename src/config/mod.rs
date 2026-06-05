use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub listen: String,
    pub providers: HashMap<String, Provider>,
    pub routing: RoutingRules,
    pub fallback: FallbackConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provider {
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<String>,
    pub cost_per_1k_input: f64,
    pub cost_per_1k_output: f64,
    pub priority: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingRules {
    pub simple_max_tokens: usize,
    pub medium_max_tokens: usize,
    pub complexity_hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackConfig {
    pub enabled: bool,
    pub max_retries: u32,
    pub timeout_ms: u64,
}

impl Default for Config {
    fn default() -> Self {
        let mut providers = HashMap::new();
        providers.insert(
            "ollama".to_string(),
            Provider {
                base_url: "http://localhost:11434".to_string(),
                api_key: "".to_string(),
                models: vec!["llama3.1".to_string(), "qwen2.5".to_string()],
                cost_per_1k_input: 0.0,
                cost_per_1k_output: 0.0,
                priority: 1,
            },
        );
        providers.insert(
            "deepseek".to_string(),
            Provider {
                base_url: "https://api.deepseek.com".to_string(),
                api_key: "${DEEPSEEK_API_KEY}".to_string(),
                models: vec!["deepseek-chat".to_string()],
                cost_per_1k_input: 0.00014,
                cost_per_1k_output: 0.00028,
                priority: 2,
            },
        );
        providers.insert(
            "openrouter".to_string(),
            Provider {
                base_url: "https://openrouter.ai/api/v1".to_string(),
                api_key: "${OPENROUTER_API_KEY}".to_string(),
                models: vec!["anthropic/claude-3.5-sonnet".to_string()],
                cost_per_1k_input: 0.003,
                cost_per_1k_output: 0.015,
                priority: 3,
            },
        );

        Self {
            listen: "127.0.0.1:3000".to_string(),
            providers,
            routing: RoutingRules {
                simple_max_tokens: 256,
                medium_max_tokens: 1024,
                complexity_hints: vec![
                    "refactor".to_string(),
                    "architecture".to_string(),
                    "design".to_string(),
                    "analyze".to_string(),
                    "debug".to_string(),
                    "complex".to_string(),
                    "multi-step".to_string(),
                ],
            },
            fallback: FallbackConfig {
                enabled: true,
                max_retries: 2,
                timeout_ms: 30000,
            },
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            let default = Self::default();
            default.save(path)?;
            return Ok(default);
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;
        let mut config: Config = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config from {}", path.display()))?;

        // Expand env vars in api_key
        for provider in config.providers.values_mut() {
            if provider.api_key.starts_with("${") && provider.api_key.ends_with("}") {
                let var_name = &provider.api_key[2..provider.api_key.len() - 1];
                provider.api_key = std::env::var(var_name).unwrap_or_default();
            }
        }

        Ok(config)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let content = toml::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)
            .with_context(|| format!("Failed to write config to {}", path.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.listen, "127.0.0.1:3000");
        assert_eq!(config.providers.len(), 3);
    }

    #[test]
    fn test_save_and_load() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let config = Config::default();
        config.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.listen, config.listen);
        assert_eq!(loaded.providers.len(), config.providers.len());
    }

    #[test]
    fn test_env_expansion() {
        std::env::set_var("TEST_API_KEY", "secret123");
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = Config::default();
        config.providers.insert(
            "test".to_string(),
            Provider {
                base_url: "http://test".to_string(),
                api_key: "${TEST_API_KEY}".to_string(),
                models: vec!["test-model".to_string()],
                cost_per_1k_input: 0.0,
                cost_per_1k_output: 0.0,
                priority: 1,
            },
        );
        config.save(&path).unwrap();
        let loaded = Config::load(&path).unwrap();
        assert_eq!(loaded.providers["test"].api_key, "secret123");
    }
}
