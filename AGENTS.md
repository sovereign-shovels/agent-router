# AGENTS.md — agent-router

> Constitution for agents working on this repo.

## What this repo is

An OpenAI-compatible proxy with heuristic model routing and cost tracking. Rust CLI.

## What this repo is NOT

- Not a load balancer (no horizontal scaling)
- Not a model provider (doesn't host models)
- Not a caching layer (v0.5)

## Sovereignty rules

1. No hardcoded API keys — only env var references in config
2. No telemetry, no phone-home
3. Works offline (SQLite for tracking, no external analytics)
4. All provider configs are user-editable TOML

## Scope boundaries

### In scope (v0.1)
- HTTP proxy with OpenAI-compatible API surface
- Heuristic prompt classification (simple/medium/complex)
- Priority-based provider routing
- SQLite cost tracking with CLI stats

### Out of scope (future versions)
- Actual HTTP forwarding to providers (v0.2)
- Streaming support (v0.3)
- Token counting via tiktoken (v0.5)
- Web dashboard (v1.0)

## Testing requirements

- Every module must have unit tests
- Router tests must cover all 3 complexity classes
- Config tests must cover save/load/expand
- Tracker tests must cover CRUD and aggregation
- CI must be green before merge

## No-nos

1. No blocking the client thread during routing
2. No leaking API keys in logs or error messages
3. No false cost guarantees (estimates are heuristic)
