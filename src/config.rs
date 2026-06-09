use serde::Deserialize;
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct RouterConfig {
    #[serde(default = "default_listen")]
    pub listen: String,
    #[serde(default = "default_fallback")]
    pub fallback_on_error: bool,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            listen: default_listen(),
            fallback_on_error: default_fallback(),
        }
    }
}

fn default_listen() -> String {
    "127.0.0.1:3456".to_string()
}

fn default_fallback() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct TierConfig {
    pub name: String,
    pub provider: String,
    pub endpoint: String,
    #[serde(rename = "api_key_env")]
    pub api_key_env: Option<String>,
    pub model: String,
    #[serde(rename = "cost_input_per_1m")]
    pub cost_input_per_1m: f64,
    #[serde(rename = "cost_output_per_1m")]
    pub cost_output_per_1m: f64,
    #[serde(rename = "max_prompt_tokens")]
    pub max_prompt_tokens: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub router: RouterConfig,
    #[serde(default)]
    #[serde(rename = "tier")]
    pub tiers: Vec<TierConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            router: RouterConfig::default(),
            tiers: vec![TierConfig {
                name: "local".to_string(),
                provider: "ollama".to_string(),
                endpoint: "http://localhost:11434/v1".to_string(),
                api_key_env: None,
                model: "llama3.2".to_string(),
                cost_input_per_1m: 0.0,
                cost_output_per_1m: 0.0,
                max_prompt_tokens: Some(4000),
            }],
        }
    }
}

pub fn config_path(app_name: &str) -> Option<PathBuf> {
    let base = env::var("XDG_CONFIG_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| env::var("HOME").ok().map(PathBuf::from))?;
    Some(base.join(".config").join(app_name).join("config.toml"))
}

pub fn load_config(app_name: &str) -> AppConfig {
    let mut config = AppConfig::default();

    if let Some(path) = config_path(app_name) {
        if let Ok(content) = fs::read_to_string(path) {
            if let Ok(parsed) = toml::from_str::<AppConfig>(&content) {
                config = parsed;
            }
        }
    }

    // Allow env override for listen address
    if let Ok(listen) = env::var("AGENT_ROUTER_LISTEN") {
        config.router.listen = listen;
    }
    if let Ok(fb) = env::var("AGENT_ROUTER_FALLBACK_ON_ERROR") {
        config.router.fallback_on_error = fb.parse().unwrap_or(true);
    }

    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = AppConfig::default();
        assert_eq!(cfg.router.listen, "127.0.0.1:3456");
        assert!(cfg.router.fallback_on_error);
        assert_eq!(cfg.tiers.len(), 1);
        assert_eq!(cfg.tiers[0].name, "local");
    }

    #[test]
    fn test_config_path_format() {
        let path = config_path("agent-router").unwrap();
        assert!(path.to_string_lossy().contains("agent-router/config.toml"));
    }
}
