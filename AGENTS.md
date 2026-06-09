# AGENTS.md — agent-router

## Project Context

`agent-router` is a Rust CLI smart proxy that routes LLM requests to the cheapest capable model. It is part of the sovereign-shovels open-source portfolio.

## Coding Conventions

- Rust 2021 edition, `cargo fmt`, `cargo clippy -- -D warnings` enforced in CI.
- All public modules must have unit tests (`#[cfg(test)]` at bottom of file).
- Integration tests live in `tests/integration.rs`.
- Use `tracing` for logs, never `println!` in library code.
- Errors propagate with `Box<dyn std::error::Error + Send + Sync>` or `thiserror` if added later.

## Build & Test

```bash
cargo test
cargo build --release
```

## 18 Universal No-Nos

1. Never commit secrets, API keys, or credentials to the repository.
2. Never introduce telemetry, analytics, or phone-home behavior.
3. Never require user sign-up or authentication for core functionality.
4. Never hardcode endpoints, models, or tiers that cannot be overridden by config.
5. Never break the OpenAI-compatible API contract on `/v1/chat/completions`.
6. Never drop or mutate streaming SSE events unexpectedly.
7. Never block the async runtime with synchronous I/O.
8. Never panic in request-handling code paths.
9. Never ignore errors from SQLite writes silently.
10. Never log request content to disk. Only metadata (token count, cost, model) is tracked.
11. Never route to a tier that exceeds its configured `max_prompt_tokens`.
12. Never skip fallback when `fallback_on_error = true` and a tier fails.
13. Never change the default listen address without updating docs.
14. Never add a new dependency without justifying it in a comment or PR description.
15. Never disable Rust warnings or clippy lints globally.
16. Never merge without CI passing on all three platforms (Linux, macOS, Windows).
17. Never ship a release without updating the changelog/version.
18. Never violate the Apache-2.0 license obligations (preserve NOTICE, attribution).
