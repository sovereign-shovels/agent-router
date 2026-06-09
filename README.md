# agent-router

Drop-in proxy that cuts AI agent costs by 5-10x. Routes simple prompts to local models, complex ones to Claude.

## Install

```bash
cargo install --git https://github.com/sovereign-shovels/agent-router
```

Or download a prebuilt binary from [Releases](https://github.com/sovereign-shovels/agent-router/releases).

## Usage

```bash
# Start the proxy on localhost:3456
agent-router serve

# Point your agent at the proxy instead of OpenAI
export OPENAI_BASE_URL=http://localhost:3456/v1
```

## Configuration

Create `~/.config/agent-router/config.toml`:

```toml
[router]
listen = "127.0.0.1:3456"
fallback_on_error = true

[[tier]]
name = "local"
provider = "ollama"
endpoint = "http://localhost:11434/v1"
model = "llama3.2"
cost_input_per_1m = 0.0
cost_output_per_1m = 0.0
max_prompt_tokens = 4000

[[tier]]
name = "cheap"
provider = "deepseek"
endpoint = "https://api.deepseek.com/v1"
api_key_env = "DEEPSEEK_API_KEY"
model = "deepseek-chat"
cost_input_per_1m = 0.14
cost_output_per_1m = 0.28
max_prompt_tokens = 32000

[[tier]]
name = "premium"
provider = "anthropic"
endpoint = "https://api.anthropic.com/v1"
api_key_env = "ANTHROPIC_API_KEY"
model = "claude-sonnet-4-20250514"
cost_input_per_1m = 3.0
cost_output_per_1m = 15.0
```

Set your API keys as environment variables:

```bash
export DEEPSEEK_API_KEY=sk-...
export ANTHROPIC_API_KEY=sk-ant-...
```

## Cost Comparison

| Scenario | Before (Premium Only) | With agent-router |
|---|---|---|
| "Hi, summarize this" (10 tokens) | $0.0003 (Claude) | $0.0000 (local Ollama) |
| "Refactor this module" (500 tokens) | $0.009 (Claude) | $0.0002 (DeepSeek) |
| "Design system architecture" (3000 tokens + tools) | $0.054 (Claude) | $0.054 (Claude) |

**Result:** 5-10x savings on typical agent workloads without touching complex queries.

## How It Works

1. **Classify** — Heuristic classifier scores prompt complexity (token count, tool calls, keywords like "refactor" or "architecture")
2. **Route** — Picks the cheapest configured tier that can handle the complexity
3. **Fallback** — If a tier errors or times out, automatically tries the next tier
4. **Track** — Logs cost per request and per session to a local SQLite database
5. **Stream** — Passes through SSE streaming responses unchanged

## Philosophy

- **Sovereign-by-construction:** BYO API key, local-first, no telemetry, no required sign-up
- **No hardcoded secrets:** No phone-home. Default endpoint is `localhost:11434` (Ollama)
- **Apache 2.0 licensed**

## Disclaimer

This is a community project and is **not affiliated with Anthropic**, OpenAI, DeepSeek, or any other model provider.

## License

Apache-2.0. See [LICENSE](./LICENSE).
