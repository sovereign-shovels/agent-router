use crate::classifier::Complexity;
use crate::config::TierConfig;
use crate::provider::{Provider, ProviderConfig};

#[derive(Debug, Clone)]
pub struct Tier {
    pub name: String,
    pub provider: Provider,
    pub max_prompt_tokens: Option<usize>,
    pub complexity: Complexity,
}

pub struct Router {
    pub tiers: Vec<Tier>,
    pub fallback_on_error: bool,
}

impl Router {
    pub fn new(tier_configs: &[TierConfig], fallback_on_error: bool) -> Self {
        let mut tiers = Vec::new();
        for cfg in tier_configs {
            let provider_config = ProviderConfig {
                name: cfg.name.clone(),
                endpoint: cfg.endpoint.clone(),
                api_key_env: cfg.api_key_env.clone(),
                model: cfg.model.clone(),
                cost_input_per_1m: cfg.cost_input_per_1m,
                cost_output_per_1m: cfg.cost_output_per_1m,
                max_prompt_tokens: cfg.max_prompt_tokens,
            };
            let complexity = match cfg.name.as_str() {
                "local" | "free" => Complexity::Simple,
                "cheap" | "budget" => Complexity::Medium,
                _ => Complexity::Complex,
            };
            tiers.push(Tier {
                name: cfg.name.clone(),
                provider: Provider::new(provider_config),
                max_prompt_tokens: cfg.max_prompt_tokens,
                complexity,
            });
        }
        Self {
            tiers,
            fallback_on_error,
        }
    }

    pub fn select_tier(&self, complexity: Complexity, estimated_tokens: usize) -> Vec<&Tier> {
        let mut candidates: Vec<&Tier> = self
            .tiers
            .iter()
            .filter(|t| {
                let can_handle_complexity = match complexity {
                    Complexity::Simple => true,
                    Complexity::Medium => t.complexity != Complexity::Simple,
                    Complexity::Complex => t.complexity == Complexity::Complex,
                };
                let within_token_limit = t
                    .max_prompt_tokens
                    .map(|max| estimated_tokens <= max)
                    .unwrap_or(true);
                can_handle_complexity && within_token_limit
            })
            .collect();

        // Sort by cost (cheapest first)
        candidates.sort_by(|a, b| {
            let a_cost = a.provider.config.cost_input_per_1m + a.provider.config.cost_output_per_1m;
            let b_cost = b.provider.config.cost_input_per_1m + b.provider.config.cost_output_per_1m;
            a_cost.partial_cmp(&b_cost).unwrap_or(std::cmp::Ordering::Equal)
        });

        candidates
    }

    pub fn select_tier_single(&self, complexity: Complexity, estimated_tokens: usize) -> Option<&Tier> {
        self.select_tier(complexity, estimated_tokens).into_iter().next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tier_config(name: &str, cost_in: f64, cost_out: f64, max_tokens: Option<usize>) -> TierConfig {
        TierConfig {
            name: name.to_string(),
            provider: name.to_string(),
            endpoint: "http://localhost".to_string(),
            api_key_env: None,
            model: "m".to_string(),
            cost_input_per_1m: cost_in,
            cost_output_per_1m: cost_out,
            max_prompt_tokens: max_tokens,
        }
    }

    #[test]
    fn test_select_simple_routes_to_local() {
        let configs = vec![
            make_tier_config("local", 0.0, 0.0, Some(4000)),
            make_tier_config("cheap", 0.14, 0.28, Some(32000)),
            make_tier_config("premium", 3.0, 15.0, Some(200000)),
        ];
        let router = Router::new(&configs, true);
        let tier = router.select_tier_single(Complexity::Simple, 50).unwrap();
        assert_eq!(tier.name, "local");
    }

    #[test]
    fn test_select_medium_routes_to_cheap() {
        let configs = vec![
            make_tier_config("local", 0.0, 0.0, Some(4000)),
            make_tier_config("cheap", 0.14, 0.28, Some(32000)),
            make_tier_config("premium", 3.0, 15.0, Some(200000)),
        ];
        let router = Router::new(&configs, true);
        let tier = router.select_tier_single(Complexity::Medium, 500).unwrap();
        assert_eq!(tier.name, "cheap");
    }

    #[test]
    fn test_select_complex_routes_to_premium() {
        let configs = vec![
            make_tier_config("local", 0.0, 0.0, Some(4000)),
            make_tier_config("cheap", 0.14, 0.28, Some(32000)),
            make_tier_config("premium", 3.0, 15.0, Some(200000)),
        ];
        let router = Router::new(&configs, true);
        let tier = router.select_tier_single(Complexity::Complex, 500).unwrap();
        assert_eq!(tier.name, "premium");
    }

    #[test]
    fn test_respects_token_limit() {
        let configs = vec![
            make_tier_config("local", 0.0, 0.0, Some(100)),
            make_tier_config("cheap", 0.14, 0.28, Some(32000)),
        ];
        let router = Router::new(&configs, true);
        let tier = router.select_tier_single(Complexity::Simple, 5000).unwrap();
        assert_eq!(tier.name, "cheap");
    }

    #[test]
    fn test_fallback_order_by_cost() {
        let configs = vec![
            make_tier_config("local", 0.0, 0.0, Some(100)),
            make_tier_config("cheap", 0.14, 0.28, Some(32000)),
            make_tier_config("premium", 3.0, 15.0, Some(200000)),
        ];
        let router = Router::new(&configs, true);
        let candidates = router.select_tier(Complexity::Complex, 50);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].name, "premium");
    }

    #[test]
    fn test_no_tier_available() {
        let configs = vec![make_tier_config("local", 0.0, 0.0, Some(10))];
        let router = Router::new(&configs, true);
        assert!(router.select_tier_single(Complexity::Medium, 5000).is_none());
    }
}
