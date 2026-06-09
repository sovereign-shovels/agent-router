use agent_router::classifier::Classifier;
use agent_router::config::TierConfig;
use agent_router::provider::{ChatMessage, ChatRequest};
use agent_router::router::Router as TierRouter;
use agent_router::tracker::CostTracker;
use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode};
use serde_json::json;
use std::sync::Arc;

async fn spawn_mock_provider(response_body: serde_json::Value, status: StatusCode) -> String {
    let app = axum::Router::new().route(
        "/chat/completions",
        axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| async move {
            let _ = body;
            (status, axum::Json(response_body))
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    // Give server a moment to start
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    format!("http://127.0.0.1:{}", port)
}

async fn spawn_mock_provider_stream(chunks: Vec<String>) -> String {
    let app = axum::Router::new().route(
        "/chat/completions",
        axum::routing::post(|| async move {
            let stream = tokio_stream::iter(chunks.clone().into_iter().map(|s| Ok::<_, std::convert::Infallible>(Bytes::from(s))));
            let body = axum::body::Body::from_stream(stream);
            axum::response::Response::builder()
                .header("content-type", "text/event-stream")
                .body(body)
                .unwrap()
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    format!("http://127.0.0.1:{}", port)
}

fn make_tier_config(name: &str, endpoint: &str, model: &str, cost_in: f64, cost_out: f64, max_tokens: Option<usize>) -> TierConfig {
    TierConfig {
        name: name.to_string(),
        provider: name.to_string(),
        endpoint: endpoint.to_string(),
        api_key_env: None,
        model: model.to_string(),
        cost_input_per_1m: cost_in,
        cost_output_per_1m: cost_out,
        max_prompt_tokens: max_tokens,
    }
}

#[tokio::test]
async fn test_proxy_routes_to_local_tier() {
    let mock_body = json!({
        "id": "test",
        "object": "chat.completion",
        "created": 1,
        "model": "llama3.2",
        "choices": [{"index":0,"message":{"role":"assistant","content":"hi"},"finish_reason":"stop"}],
        "usage": {"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}
    });
    let endpoint = spawn_mock_provider(mock_body, StatusCode::OK).await;

    let configs = vec![
        make_tier_config("local", &endpoint, "llama3.2", 0.0, 0.0, Some(4000)),
        make_tier_config("cheap", &endpoint, "cheap-model", 0.14, 0.28, Some(32000)),
    ];
    let tier_router = TierRouter::new(&configs, true);
    let tracker = CostTracker::new(Some(std::env::temp_dir().join("test_routes_local.db"))).unwrap();
    let classifier = Classifier::new();

    let state = agent_router::proxy::AppState {
        router: Arc::new(tier_router),
        tracker: Arc::new(tracker),
        classifier: Arc::new(classifier),
    };

    let req = ChatRequest {
        model: "ignored".to_string(),
        messages: vec![ChatMessage::user("hi")],
        temperature: None,
        max_tokens: None,
        tools: None,
        stream: false,
    };
    let body = Bytes::from(serde_json::to_vec(&req).unwrap());
    let headers = HeaderMap::new();

    let res = agent_router::proxy::chat_completions(
        axum::extract::State(state),
        headers,
        body,
    )
    .await
    .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_fallback_when_first_tier_fails() {
    let fail_endpoint = spawn_mock_provider(
        json!({"error":{"message":"down","type":"provider_error"}}),
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;

    let ok_body = json!({
        "id": "test2",
        "object": "chat.completion",
        "created": 1,
        "model": "cheap-model",
        "choices": [{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],
        "usage": {"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}
    });
    let ok_endpoint = spawn_mock_provider(ok_body, StatusCode::OK).await;

    let configs = vec![
        make_tier_config("local", &fail_endpoint, "llama3.2", 0.0, 0.0, Some(4000)),
        make_tier_config("cheap", &ok_endpoint, "cheap-model", 0.14, 0.28, Some(32000)),
    ];
    let tier_router = TierRouter::new(&configs, true);
    let tracker = CostTracker::new(Some(std::env::temp_dir().join("test_fallback.db"))).unwrap();
    let classifier = Classifier::new();

    let state = agent_router::proxy::AppState {
        router: Arc::new(tier_router),
        tracker: Arc::new(tracker),
        classifier: Arc::new(classifier),
    };

    let req = ChatRequest {
        model: "ignored".to_string(),
        messages: vec![ChatMessage::user("hi")],
        temperature: None,
        max_tokens: None,
        tools: None,
        stream: false,
    };
    let body = Bytes::from(serde_json::to_vec(&req).unwrap());
    let headers = HeaderMap::new();

    let res = agent_router::proxy::chat_completions(
        axum::extract::State(state),
        headers,
        body,
    )
    .await
    .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_streaming_pass_through() {
    let chunks = vec![
        "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\"},\"finish_reason\":null}]}\n\n".to_string(),
        "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n".to_string(),
        "data: {\"id\":\"1\",\"object\":\"chat.completion.chunk\",\"created\":1,\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n".to_string(),
        "data: [DONE]\n\n".to_string(),
    ];
    let endpoint = spawn_mock_provider_stream(chunks).await;

    let configs = vec![make_tier_config("local", &endpoint, "llama3.2", 0.0, 0.0, Some(4000))];
    let tier_router = TierRouter::new(&configs, true);
    let tracker = CostTracker::new(Some(std::env::temp_dir().join("test_stream.db"))).unwrap();
    let classifier = Classifier::new();

    let state = agent_router::proxy::AppState {
        router: Arc::new(tier_router),
        tracker: Arc::new(tracker),
        classifier: Arc::new(classifier),
    };

    let req = ChatRequest {
        model: "ignored".to_string(),
        messages: vec![ChatMessage::user("stream me")],
        temperature: None,
        max_tokens: None,
        tools: None,
        stream: true,
    };
    let body = Bytes::from(serde_json::to_vec(&req).unwrap());
    let headers = HeaderMap::new();

    let res = agent_router::proxy::chat_completions(
        axum::extract::State(state),
        headers,
        body,
    )
    .await
    .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let ct = res.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(ct.contains("event-stream"));
}

#[tokio::test]
async fn test_complex_request_routes_to_premium() {
    let cheap_endpoint = spawn_mock_provider(
        json!({"error":{"message":"nope"}}),
        StatusCode::BAD_REQUEST,
    )
    .await;

    let premium_body = json!({
        "id": "prem",
        "object": "chat.completion",
        "created": 1,
        "model": "claude",
        "choices": [{"index":0,"message":{"role":"assistant","content":"architected"},"finish_reason":"stop"}],
        "usage": {"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}
    });
    let premium_endpoint = spawn_mock_provider(premium_body, StatusCode::OK).await;

    let configs = vec![
        make_tier_config("local", &cheap_endpoint, "llama", 0.0, 0.0, Some(4000)),
        make_tier_config("cheap", &cheap_endpoint, "deepseek", 0.14, 0.28, Some(32000)),
        make_tier_config("premium", &premium_endpoint, "claude", 3.0, 15.0, Some(200000)),
    ];
    let tier_router = TierRouter::new(&configs, true);
    let tracker = CostTracker::new(Some(std::env::temp_dir().join("test_premium.db"))).unwrap();
    let classifier = Classifier::new();

    let state = agent_router::proxy::AppState {
        router: Arc::new(tier_router),
        tracker: Arc::new(tracker),
        classifier: Arc::new(classifier),
    };

    let req = ChatRequest {
        model: "ignored".to_string(),
        messages: vec![ChatMessage::user("design the system architecture for a distributed database")],
        temperature: None,
        max_tokens: None,
        tools: None,
        stream: false,
    };
    let body = Bytes::from(serde_json::to_vec(&req).unwrap());
    let headers = HeaderMap::new();

    let res = agent_router::proxy::chat_completions(
        axum::extract::State(state),
        headers,
        body,
    )
    .await
    .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_cost_tracking_accuracy() {
    let endpoint = spawn_mock_provider(
        json!({
            "id": "cost",
            "object": "chat.completion",
            "created": 1,
            "model": "ds",
            "choices": [{"index":0,"message":{"role":"assistant","content":"x"},"finish_reason":"stop"}],
            "usage": {"prompt_tokens":1000000,"completion_tokens":500000,"total_tokens":1500000}
        }),
        StatusCode::OK,
    )
    .await;

    let configs = vec![make_tier_config("cheap", &endpoint, "ds", 0.14, 0.28, Some(32000))];
    let tier_router = TierRouter::new(&configs, true);
    let db_path = std::env::temp_dir().join("test_cost.db");
    let _ = std::fs::remove_file(&db_path);
    let tracker = CostTracker::new(Some(db_path.clone())).unwrap();
    let classifier = Classifier::new();

    let state = agent_router::proxy::AppState {
        router: Arc::new(tier_router),
        tracker: Arc::new(tracker.clone()),
        classifier: Arc::new(classifier),
    };

    let req = ChatRequest {
        model: "ignored".to_string(),
        messages: vec![ChatMessage::user(&"x ".repeat(2000))],
        temperature: None,
        max_tokens: None,
        tools: None,
        stream: false,
    };
    let body = Bytes::from(serde_json::to_vec(&req).unwrap());
    let headers = HeaderMap::new();

    let res = agent_router::proxy::chat_completions(
        axum::extract::State(state),
        headers,
        body,
    )
    .await
    .unwrap();

    assert_eq!(res.status(), StatusCode::OK);

    let total = tracker.total_cost().unwrap();
    let expected = CostTracker::calculate_cost(1_000_000, 500_000, 0.14, 0.28);
    assert!((total - expected).abs() < 0.0001, "cost {} != expected {}", total, expected);

    let _ = std::fs::remove_file(&db_path);
}
