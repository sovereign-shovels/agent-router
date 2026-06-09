use crate::classifier::{Classifier, Complexity};
use crate::provider::{ChatRequest, ChatResponse, Choice, MessageContent, Usage};
use crate::router::Router;
use crate::tracker::CostTracker;
use axum::body::{Body, Bytes};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures::stream::StreamExt;
use serde_json::Value;
use std::sync::Arc;
use tracing::{error, info, warn};

#[derive(Clone)]
pub struct AppState {
    pub router: Arc<Router>,
    pub tracker: Arc<CostTracker>,
    pub classifier: Arc<Classifier>,
}

pub async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

pub async fn chat_completions(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, StatusCode> {
    let req: ChatRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            warn!("Failed to parse request body: {}", e);
            return Ok(bad_request(format!("Invalid JSON: {}", e)));
        }
    };

    let session_id = headers
        .get("x-session-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let estimated_tokens = Classifier::estimate_tokens(
        &req.messages
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join(" "),
    );
    let complexity = state.classifier.classify(&req);

    info!(
        "Request complexity={:?} tokens={} stream={}",
        complexity, estimated_tokens, req.stream
    );

    if state.router.select_tier(complexity, estimated_tokens).is_empty() {
        return Ok(bad_request("No tier available for this request".to_string()));
    }

    if req.stream {
        handle_stream(state, req, complexity, estimated_tokens, session_id).await
    } else {
        handle_non_stream(state, req, complexity, estimated_tokens, session_id).await
    }
}

async fn handle_non_stream(
    state: AppState,
    mut req: ChatRequest,
    complexity: Complexity,
    estimated_tokens: usize,
    session_id: Option<String>,
) -> Result<Response, StatusCode> {
    let mut last_error = None;
    let candidates = state.router.select_tier(complexity, estimated_tokens);

    for tier in &candidates {
        req.model = tier.provider.config.model.clone();
        match tier.provider.chat(&req).await {
            Ok(mut res) => {
                // Fill in model field if missing
                if res.model.is_none() {
                    res.model = Some(tier.provider.config.model.clone());
                }

                let input_tokens = res
                    .usage
                    .as_ref()
                    .and_then(|u| u.prompt_tokens)
                    .unwrap_or(0);
                let output_tokens = res
                    .usage
                    .as_ref()
                    .and_then(|u| u.completion_tokens)
                    .unwrap_or(0);
                let cost = CostTracker::calculate_cost(
                    input_tokens,
                    output_tokens,
                    tier.provider.config.cost_input_per_1m,
                    tier.provider.config.cost_output_per_1m,
                );

                if let Err(e) = state.tracker.record_request(
                    session_id.as_deref(),
                    &tier.name,
                    &tier.provider.config.model,
                    &tier.provider.config.name,
                    input_tokens,
                    output_tokens,
                    cost,
                    true,
                    None,
                ) {
                    warn!("Failed to record request: {}", e);
                }

                return Ok((StatusCode::OK, Json(res)).into_response());
            }
            Err(e) => {
                warn!(
                    "Tier {} failed: {}. Fallback={}",
                    tier.name, e, state.router.fallback_on_error
                );
                last_error = Some(format!("{}: {}", tier.name, e));
                if !state.router.fallback_on_error {
                    break;
                }
            }
        }
    }

    let error_msg = last_error.unwrap_or_else(|| "All tiers failed".to_string());
    error!("All tiers failed: {}", error_msg);

    // Record fallback failure
    if let Some(first_tier) = candidates.first() {
        let _ = state.tracker.record_request(
            session_id.as_deref(),
            &first_tier.name,
            &first_tier.provider.config.model,
            &first_tier.provider.config.name,
            0,
            0,
            0.0,
            false,
            Some(&error_msg),
        );
    }

    Ok(bad_request(error_msg))
}

async fn handle_stream(
    state: AppState,
    mut req: ChatRequest,
    complexity: Complexity,
    estimated_tokens: usize,
    session_id: Option<String>,
) -> Result<Response, StatusCode> {
    let mut last_error = None;
    let candidates = state.router.select_tier(complexity, estimated_tokens);

    for tier in candidates {
        req.model = tier.provider.config.model.clone();
        match tier.provider.chat_stream(&req).await {
            Ok(response) => {
                let tier_name = tier.name.clone();
                let model = tier.provider.config.model.clone();
                let provider_name = tier.provider.config.name.clone();
                let cost_input = tier.provider.config.cost_input_per_1m;
                let cost_output = tier.provider.config.cost_output_per_1m;
                let tracker = state.tracker.clone();
                let session_id = session_id.clone();
                let messages_for_estimate = req.messages.clone();

                let stream = response.bytes_stream();
                let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Result<Bytes, std::convert::Infallible>>();

                tokio::spawn(async move {
                    let mut buffer = Vec::new();
                    let mut stream = stream;

                    while let Some(chunk_result) = stream.next().await {
                        match chunk_result {
                            Ok(chunk) => {
                                buffer.extend_from_slice(&chunk);
                                let _ = tx.send(Ok(chunk));
                            }
                            Err(e) => {
                                warn!("Stream chunk error: {}", e);
                                let err_bytes = Bytes::from(format!("data: {{\"error\":\"{}\"}}\n\n", e));
                                let _ = tx.send(Ok(err_bytes));
                            }
                        }
                    }
                    drop(tx);

                    // Parse buffer for usage and token estimation
                    let text = String::from_utf8_lossy(&buffer);
                    let mut input_tokens = 0u32;
                    let mut output_tokens = 0u32;
                    let mut total_content_chars = 0usize;

                    for line in text.lines() {
                        if let Some(data) = line.strip_prefix("data: ") {
                            if data == "[DONE]" {
                                continue;
                            }
                            if let Ok(json) = serde_json::from_str::<Value>(data) {
                                if let Some(usage) = json.get("usage") {
                                    if let Some(p) = usage.get("prompt_tokens").and_then(|v| v.as_u64()) {
                                        input_tokens = p as u32;
                                    }
                                    if let Some(c) = usage.get("completion_tokens").and_then(|v| v.as_u64()) {
                                        output_tokens = c as u32;
                                    }
                                }
                                if let Some(choices) = json.get("choices") {
                                    if let Some(first) = choices.as_array().and_then(|a| a.first()) {
                                        if let Some(delta) = first.get("delta") {
                                            if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                                                total_content_chars += content.len();
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Estimate output tokens from chars if not reported
                    if output_tokens == 0 && total_content_chars > 0 {
                        output_tokens = (total_content_chars / 4) as u32;
                    }
                    // Estimate input tokens if not reported
                    if input_tokens == 0 {
                        input_tokens = (messages_for_estimate.iter().map(|m| m.content.len()).sum::<usize>() / 4) as u32;
                    }

                    let cost = CostTracker::calculate_cost(input_tokens, output_tokens, cost_input, cost_output);
                    if let Err(e) = tracker.record_request(
                        session_id.as_deref(),
                        &tier_name,
                        &model,
                        &provider_name,
                        input_tokens,
                        output_tokens,
                        cost,
                        true,
                        None,
                    ) {
                        warn!("Failed to record stream request: {}", e);
                    }
                });

                let body_stream = tokio_stream::wrappers::UnboundedReceiverStream::new(rx);
                return Ok(Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .header("cache-control", "no-cache")
                    .body(Body::from_stream(body_stream))
                    .unwrap());
            }
            Err(e) => {
                warn!("Tier {} stream failed: {}", tier.name, e);
                last_error = Some(format!("{}: {}", tier.name, e));
                if !state.router.fallback_on_error {
                    break;
                }
            }
        }
    }

    let error_msg = last_error.unwrap_or_else(|| "All tiers failed".to_string());
    error!("All stream tiers failed: {}", error_msg);
    Ok(bad_request(error_msg))
}

fn bad_request(msg: String) -> Response {
    let res = ChatResponse {
        id: None,
        object: Some("chat.completion".into()),
        created: Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        ),
        model: None,
        choices: Some(vec![Choice {
            index: Some(0),
            message: Some(MessageContent {
                role: Some("assistant".into()),
                content: Some(format!("Error: {}", msg)),
                tool_calls: None,
            }),
            delta: None,
            finish_reason: Some("error".into()),
        }]),
        usage: Some(Usage {
            prompt_tokens: Some(0),
            completion_tokens: Some(0),
            total_tokens: Some(0),
        }),
        error: Some(crate::provider::ApiError {
            message: Some(msg),
            error_type: Some("router_error".into()),
        }),
    };
    (StatusCode::BAD_REQUEST, Json(res)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[test]
    fn test_bad_request_response() {
        let res = bad_request("test error".to_string());
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
}
