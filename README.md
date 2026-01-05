# Claude Profiler

A fast, local TUI for launching Claude Code with saved profiles and a built-in proxy for LM Studio.

## Features
- Profile-based launcher for Claude Code
- Built-in Anthropic-to-OpenAI proxy for LM Studio
- Optional auxiliary model for lightweight requests
- In-app profile editing and model selection

## Requirements
- macOS
- Rust toolchain (stable)
- Claude Code CLI (`claude`)
- LM Studio (optional, for local models)

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
- `l` to select LM Studio models
- `?` for help
- `q` to quit

Configuration is stored at:
- `~/Library/Application Support/claude-profiler/profiles.toml`

## LM Studio
- Press `l` to select a local model from LM Studio.
- If the LM Studio CLI isn't installed, the app will prompt you to run:
  `~/.lmstudio/bin/lms bootstrap`

## Security
Please see `SECURITY.md` for reporting guidelines.

## License
MIT. See `LICENSE`.
