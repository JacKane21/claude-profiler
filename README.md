# Claude Profiler

A fast, local TUI for launching Claude Code with saved profiles and a built-in universal proxy.

## Features
- Profile-based launcher for Claude Code
- Built-in Anthropic-to-OpenAI proxy (Responses or Completions)
- Optional auxiliary model for lightweight requests
- In-app profile editing

## Requirements
- macOS
- Rust toolchain (stable)
- Claude Code CLI (`claude`)

## Install
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

## Usage
- `↑/k`, `↓/j` to move
- `Enter` to launch
- `e` to edit the selected profile
- `n` to create a new profile
- `d` to delete the selected profile
- `r` to reset the selected profile
- `R` to reset ALL profiles
- `?` for help
- `q` to quit

Configuration is stored at:
- `~/Library/Application Support/claude-profiler/profiles.toml`

## Universal Proxy
- Use the `proxy` profile and set `PROXY_TARGET_URL` to any OpenAI-compatible base URL
  (e.g. `http://localhost:1234/v1`).
- The proxy auto-detects whether the upstream supports `/v1/responses` or `/v1/chat/completions`
  (and falls back to `/v1/completions` if needed).
- Adjust model IDs via `ANTHROPIC_DEFAULT_*_MODEL` and `ANTHROPIC_MODEL`.

## Security
Please see `SECURITY.md` for reporting guidelines.

## License
MIT. See `LICENSE`.
