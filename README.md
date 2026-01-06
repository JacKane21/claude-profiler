# Claude Profiler

A fast, local TUI for launching Claude Code with saved profiles and a built-in Anthropic <-> OpenAI proxy.

## Features
- Profile-based launcher for the Claude Code CLI
- Built-in proxy for OpenAI-compatible APIs (Responses / Chat Completions / Completions)
- Optional auxiliary model routing for lightweight requests
- OpenAI Codex OAuth flow with a local callback and token cache
- In-app profile editor, including a model picker for Codex profiles

## Requirements
- macOS, Windows, or Linux
- Rust toolchain (stable, edition 2024)
- Claude Code CLI (`claude`) available in PATH

## Install
Build from source:
```bash
cargo build --release
```

Or install locally:
```bash
cargo install --path .
```

## Run
```bash
claude-profiler
```

## Key Bindings
Normal mode:
- `Up`/`k`, `Down`/`j` to move
- `Enter` to launch
- `e` to edit the selected profile
- `n` to create a new profile
- `d` to delete the selected profile
- `r` to reset the selected profile (or clear OAuth tokens for Codex profiles)
- `R` to reset all profiles and OAuth tokens
- `?` to toggle help (any key closes it)
- `q` or `Esc` to quit

Edit mode:
- `Tab`/`Shift+Tab` or `Down`/`Up` to change fields
- `Ctrl+G` to toggle API key visibility
- `Enter` to save (or open the model picker on Codex model fields)
- `Esc` to cancel

Model picker:
- `Up`/`k`, `Down`/`j` to move
- `Enter` to select
- `Esc` to cancel

## Configuration
Profiles are stored in `profiles.toml`:
- macOS: `~/Library/Application Support/claude-profiler/profiles.toml`
- Linux: `~/.config/claude-profiler/profiles.toml`
- Windows: `%APPDATA%\claude-profiler\profiles.toml`

OpenAI OAuth tokens are stored alongside the profiles in `openai-oauth.json`.
Codex instruction caches are stored in the same directory under `cache/`.

You can edit profiles in the UI or by editing `profiles.toml` directly. Any additional
environment variables not exposed in the UI can be added manually to a profile.

### Default Profiles
On first run, a default config is created with these profiles:
- `default` (uses your existing environment)
- `zai` (Z.ai Anthropic-compatible proxy)
- `minimax` (MiniMax Anthropic-compatible proxy)
- `OpenRouter` (direct OpenRouter API)
- `OpenAI Codex OAuth` (ChatGPT OAuth + Codex backend)
- `custom example` (example OpenAI-compatible endpoint)

These are templates only. Replace placeholder API keys before use.

### Profile Environment Variables
The profile editor maps to these environment variables:

| Variable | Purpose |
| --- | --- |
| `ANTHROPIC_AUTH_TOKEN` | API key for Anthropic or the upstream provider. When `OPENAI_OAUTH` is enabled, this is populated automatically. |
| `ANTHROPIC_BASE_URL` | Base URL for a direct Anthropic-compatible API. When the proxy is enabled, this is set to `http://localhost:4000/anthropic`. |
| `PROXY_TARGET_URL` | OpenAI-compatible base URL or endpoint. Setting this enables the proxy. |
| `ANTHROPIC_DEFAULT_HAIKU_MODEL` | Default Haiku model name. |
| `ANTHROPIC_DEFAULT_SONNET_MODEL` | Default Sonnet model name. |
| `ANTHROPIC_DEFAULT_OPUS_MODEL` | Default Opus model name. |
| `ANTHROPIC_MODEL` | Fallback model name and proxy model override. |
| `ANTHROPIC_SMALL_FAST_MODEL` | Auxiliary model for lightweight requests (proxy only). |
| `OPENAI_OAUTH` | Set to `1`/`true` to enable ChatGPT OAuth. |
| `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` | Passed through to Claude Code. |
| `API_TIMEOUT_MS` | Passed through to Claude Code. |

### Example `profiles.toml`
```toml
# Minimal direct profile
[[profiles]]
name = "anthropic"
description = "Direct Anthropic endpoint"

[profiles.env]
ANTHROPIC_AUTH_TOKEN = "YOUR_API_KEY_HERE"
ANTHROPIC_BASE_URL = "https://api.anthropic.com"
ANTHROPIC_DEFAULT_HAIKU_MODEL = "claude-3-haiku"
ANTHROPIC_DEFAULT_SONNET_MODEL = "claude-3-5-sonnet"
ANTHROPIC_DEFAULT_OPUS_MODEL = "claude-3-opus"

# Proxy to an OpenAI-compatible endpoint
[[profiles]]
name = "local-openai"
description = "Local OpenAI-compatible server"

[profiles.env]
ANTHROPIC_AUTH_TOKEN = "local"
PROXY_TARGET_URL = "http://localhost:1234/v1"
ANTHROPIC_DEFAULT_HAIKU_MODEL = "local-model"
ANTHROPIC_DEFAULT_SONNET_MODEL = "local-model"
ANTHROPIC_DEFAULT_OPUS_MODEL = "local-model"
```

## Universal Proxy
Set `PROXY_TARGET_URL` to any OpenAI-compatible endpoint. You can provide a base URL
(such as one ending in `/v1`) or a full endpoint ending with `/responses`,
`/chat/completions`, or `/completions`.

When the proxy is enabled:
- A local server listens on `127.0.0.1:4000` and exposes `http://localhost:4000/anthropic`.
- Requests are translated from Anthropic to OpenAI formats and back.
- Auto mode tries `/v1/responses` first, then `/v1/chat/completions`, and finally
  `/v1/completions` if needed.

## OpenAI Codex OAuth
The `OpenAI Codex OAuth` profile uses ChatGPT OAuth and the Codex backend. On first launch:
- A browser opens for sign-in.
- A local callback server listens on `http://localhost:1455/auth/callback` for up to 5 minutes.
- If the browser cannot open, paste the redirect URL or code into the terminal.

Tokens are stored in `openai-oauth.json`. Use `r` on a Codex profile to clear tokens,
or `R` to reset everything.

For Codex requests, the proxy fetches official instructions from the OpenAI Codex
repository on GitHub and caches them for about 15 minutes under `cache/`.

## Troubleshooting
- `claude` not found: ensure the Claude Code CLI is installed and `claude` is in PATH.
- Proxy startup timeout: ensure nothing else is bound to `127.0.0.1:4000` and that
  the upstream URL in `PROXY_TARGET_URL` is reachable.
- OAuth sign-in never completes: make sure `http://localhost:1455/auth/callback` is
  not blocked by a firewall, then retry and paste the redirect URL manually.
- Model picker empty: it only appears for Codex profiles; ensure the profile points
  at the ChatGPT Codex backend.
- Accidentally cleared tokens: select the Codex profile and launch again to re-auth.

## Security
Please see `SECURITY.md` for reporting guidelines.

## License
MIT. See `LICENSE`.
