use agent_router::{config::Config, proxy::ProxyServer, tracker::CostTracker};
use anyhow::Result;
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "agent-router")]
#[command(about = "OpenAI-compatible proxy with heuristic model routing and cost tracking")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the proxy server
    Serve {
        /// Path to config file
        #[arg(short, long, default_value = "~/.config/agent-router/config.toml")]
        config: PathBuf,
    },
    /// Show cost statistics
    Stats {
        /// Path to database file
        #[arg(short, long, default_value = "~/.local/share/agent-router/tracker.db")]
        db: PathBuf,
        /// Show stats for last N hours
        #[arg(short, long)]
        hours: Option<u64>,
    },
    /// Initialize default configuration
    Init {
        /// Path to config file
        #[arg(short, long, default_value = "~/.config/agent-router/config.toml")]
        config: PathBuf,
    },
}

fn expand_path(path: &PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(&s[2..]);
        }
    }
    path.clone()
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("agent_router=info".parse()?))
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { config } => {
            let config_path = expand_path(&config);
            let config = Config::load(&config_path)?;

            let db_dir = dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("agent-router");
            std::fs::create_dir_all(&db_dir)?;
            let tracker = CostTracker::open(&db_dir.join("tracker.db"))?;

            let addr: SocketAddr = config.listen.parse()?;
            let server = ProxyServer::new(config, tracker)?;
            server.run(addr).await?;
        }
        Commands::Stats { db, hours } => {
            let db_path = expand_path(&db);
            if !db_path.exists() {
                println!("No database found at {}", db_path.display());
                return Ok(());
            }
            let tracker = CostTracker::open(&db_path)?;

            let total_cost = tracker.total_cost(hours)?;
            let (input_tokens, output_tokens) = tracker.total_tokens(hours)?;
            let breakdown = tracker.provider_breakdown()?;
            let recent = tracker.recent_requests(10)?;

            println!("\n📊 agent-router Stats");
            println!("=====================");
            if let Some(h) = hours {
                println!("Period: last {} hours", h);
            } else {
                println!("Period: all time");
            }
            println!("Total cost:     ${:.6}", total_cost);
            println!("Input tokens:   {}", input_tokens);
            println!("Output tokens:  {}", output_tokens);

            if !breakdown.is_empty() {
                println!("\nProvider breakdown:");
                for (provider, cost, count) in breakdown {
                    println!("  {:15} ${:>10.6}  {} requests", provider, cost, count);
                }
            }

            if !recent.is_empty() {
                println!("\nRecent requests:");
                for req in recent {
                    println!(
                        "  [{}] {:10} {:20} {:>8}ms  ${:.6}",
                        req.provider,
                        req.complexity,
                        req.model,
                        req.latency_ms,
                        req.actual_cost
                    );
                }
            }
            println!();
        }
        Commands::Init { config } => {
            let config_path = expand_path(&config);
            Config::default().save(&config_path)?;
            println!("✓ Created default config at {}", config_path.display());
            println!("  Edit the file and set your API keys before running `agent-router serve`");
        }
    }

    Ok(())
}
