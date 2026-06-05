use crate::config::Config;
use crate::router::Router;
use crate::tracker::CostTracker;
use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

pub struct ProxyServer {
    _config: Arc<Config>,
    router: Arc<Router>,
    tracker: Arc<Mutex<CostTracker>>,
    client: reqwest::Client,
}

impl ProxyServer {
    pub fn new(config: Config, tracker: CostTracker) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(config.fallback.timeout_ms))
            .build()
            .context("Failed to build HTTP client")?;

        let router = Arc::new(Router::new(config.clone()));

        Ok(Self {
            _config: Arc::new(config),
            router,
            tracker: Arc::new(Mutex::new(tracker)),
            client,
        })
    }

    pub async fn run(self, addr: SocketAddr) -> Result<()> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        tracing::info!("agent-router listening on http://{}", local_addr);

        let router = self.router.clone();
        let tracker = self.tracker.clone();
        let client = self.client.clone();

        loop {
            let (stream, _) = listener.accept().await?;
            let io = TokioIo::new(stream);

            let router = router.clone();
            let tracker = tracker.clone();
            let client = client.clone();

            tokio::task::spawn(async move {
                let service = service_fn(move |req| {
                    handle_request(req, router.clone(), tracker.clone(), client.clone())
                });

                if let Err(err) = http1::Builder::new().serve_connection(io, service).await {
                    tracing::debug!("Connection error: {}", err);
                }
            });
        }
    }
}

async fn handle_request(
    req: Request<Incoming>,
    router: Arc<Router>,
    tracker: Arc<Mutex<CostTracker>>,
    client: reqwest::Client,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let start = Instant::now();

    // Only handle POST /v1/chat/completions for now
    let path = req.uri().path();
    if path != "/v1/chat/completions" {
        return Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Full::new(Bytes::from(r#"{"error":"Not Found"}"#)))
            .unwrap());
    }

    // Collect body
    let body_bytes = match req.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from(r#"{"error":"Bad Request"}"#)))
                .unwrap());
        }
    };

    let body_json: Value = match serde_json::from_slice(&body_bytes) {
        Ok(v) => v,
        Err(_) => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from(r#"{"error":"Invalid JSON"}"#)))
                .unwrap());
        }
    };

    // Route the request
    let decision = match router.route(&body_json) {
        Some(d) => d,
        None => {
            return Ok(Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Full::new(Bytes::from(
                    r#"{"error":"No provider available for requested model"}"#,
                )))
                .unwrap());
        }
    };

    tracing::info!(
        "Routing to {} ({}) — complexity: {:?}, est_cost: ${:.6}",
        decision.provider,
        decision.model,
        decision.complexity,
        decision.estimated_cost
    );

    // For now, return a mock response since we don't have actual provider configs wired
    // In full implementation, this forwards to the actual provider
    let latency = start.elapsed().as_millis() as u64;

    // Log the request
    let _ = tracker.lock().await.log_request(
        &decision.provider,
        &decision.model,
        &format!("{:?}", decision.complexity).to_lowercase(),
        0,
        0,
        decision.estimated_cost,
        0.0,
        latency,
        "mock",
    );

    let mock_response = serde_json::json!({
        "id": "agent-router-mock",
        "object": "chat.completion",
        "created": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs(),
        "model": decision.model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": format!("[agent-router] Routed to {} ({:?})\n\nThis is a mock response. In production, the request would be forwarded to the provider.", decision.provider, decision.complexity)
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 0,
            "completion_tokens": 0,
            "total_tokens": 0
        },
        "_agent_router": {
            "provider": decision.provider,
            "complexity": format!("{:?}", decision.complexity).to_lowercase(),
            "estimated_cost": decision.estimated_cost
        }
    });

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(mock_response.to_string())))
        .unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::tracker::CostTracker;
    use tempfile::TempDir;

    #[test]
    fn test_proxy_server_creation() {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("tracker.db");
        let tracker = CostTracker::open(&db_path).unwrap();
        let config = Config::default();

        let proxy = ProxyServer::new(config, tracker);
        assert!(proxy.is_ok());
    }
}
