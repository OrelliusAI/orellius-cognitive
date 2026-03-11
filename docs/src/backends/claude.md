# Claude (Anthropic) Backend

The `laminae-anthropic` crate provides a first-party `EgoBackend` for Claude.

## Setup

```toml
[dependencies]
laminae = { version = "0.4", features = ["anthropic"] }
tokio = { version = "1", features = ["full"] }
```

## Usage

```rust
use laminae::psyche::PsycheEngine;
use laminae::anthropic::ClaudeBackend;
use laminae::ollama::OllamaClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let ego = ClaudeBackend::from_env()?; // reads ANTHROPIC_API_KEY from env
    let engine = PsycheEngine::new(OllamaClient::new(), ego);

    let response = engine.reply("Explain quantum entanglement simply.").await?;
    println!("{response}");
    Ok(())
}
```

```bash
ANTHROPIC_API_KEY=sk-ant-... cargo run
```

## Configuration

`ClaudeBackend` uses environment variables:

| Variable | Required | Default |
|----------|----------|---------|
| `ANTHROPIC_API_KEY` | Yes | - |
| `CLAUDE_MODEL` | No | `claude-sonnet-4-20250514` |
| `CLAUDE_MAX_TOKENS` | No | `4096` |
