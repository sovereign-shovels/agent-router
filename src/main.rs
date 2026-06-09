use axum::routing::post;
use axum::{routing::get, Router};
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{info, warn};

mod classifier;
mod config;
mod provider;
mod proxy;
mod router;
mod tracker;

use classifier::Classifier;
use config::load_config;
use proxy::{chat_completions, health_check, AppState};
use router::Router as TierRouter;
use tracker::CostTracker;

#[derive(Parser)]
#[command(name = "agent-router")]
#[command(about = "Smart model router for AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the proxy server
    Serve,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve => serve().await,
    }
}

async fn serve() {
    let app_config = load_config("agent-router");
    let listen_addr: SocketAddr = app_config
        .router
        .listen
        .parse()
        .expect("Invalid listen address");

    info!("Starting agent-router on {}", listen_addr);
    info!("Loaded {} tiers", app_config.tiers.len());
    for tier in &app_config.tiers {
        info!("  Tier: {} -> {} ({})", tier.name, tier.model, tier.endpoint);
    }

    let tier_router = TierRouter::new(&app_config.tiers, app_config.router.fallback_on_error);
    let tracker = match CostTracker::new(None) {
        Ok(t) => {
            info!("Cost tracker initialized");
            t
        }
        Err(e) => {
            warn!("Failed to initialize cost tracker: {}. Using in-memory fallback.", e);
            // This will panic on actual use, but we keep compiling
            CostTracker::new(Some(std::env::temp_dir().join("agent-router-fallback.db")))
                .expect("fallback tracker")
        }
    };

    let state = AppState {
        router: Arc::new(tier_router),
        tracker: Arc::new(tracker),
        classifier: Arc::new(Classifier::new()),
    };

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/v1/chat/completions", post(chat_completions))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(listen_addr)
        .await
        .expect("Failed to bind");

    info!("agent-router ready — proxying OpenAI-compatible requests");
    axum::serve(listener, app)
        .await
        .expect("Server error");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parse() {
        let cli = Cli::try_parse_from(["agent-router", "serve"]);
        assert!(cli.is_ok());
    }
}
