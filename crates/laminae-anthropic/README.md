# laminae-anthropic

Anthropic Claude backend for [Laminae](https://github.com/Orellius/laminae) -- connect any Claude model to the Laminae SDK with blocking and streaming completions via the Messages API.

Part of the [Laminae](https://github.com/Orellius/laminae) SDK.

## Features

- Blocking and streaming completions via `/v1/messages`
- Builder pattern for model, temperature, and max tokens
- Configurable base URL for proxies or compatible endpoints
- API key from environment or explicit string
- Server-Sent Events (SSE) streaming support

## Quick Start

```toml
[dependencies]
laminae-anthropic = "0.3"
```

```rust
use laminae_anthropic::ClaudeBackend;
use laminae_psyche::EgoBackend;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let claude = ClaudeBackend::from_env()?;
    let response = claude.complete(
        "You are a helpful assistant.",
        "What is Rust?",
        "",
    ).await?;
    println!("{response}");
    Ok(())
}
```

## Streaming

```rust
let claude = ClaudeBackend::from_env()?;
let mut rx = claude.complete_streaming(
    "You are helpful.", "Hello!", "",
).await?;

while let Some(chunk) = rx.recv().await {
    print!("{chunk}");
}
```

## Custom Configuration

```rust
use laminae_anthropic::{ClaudeBackend, ClaudeConfig};

let claude = ClaudeBackend::new("sk-ant-...")
    .with_model("claude-opus-4-20250514")
    .with_temperature(0.7)
    .with_max_tokens(8192);

// Or with full config control:
let mut config = ClaudeConfig::default();
config.api_key = "sk-ant-...".to_string();
config.model = "claude-sonnet-4-20250514".to_string();
config.max_tokens = 4096;
let claude = ClaudeBackend::with_config(config)?;
```

## Environment

Set `ANTHROPIC_API_KEY` to your API key from [console.anthropic.com](https://console.anthropic.com/settings/keys).

```sh
export ANTHROPIC_API_KEY="sk-ant-..."
```

## License

Apache-2.0 -- see [LICENSE](../../LICENSE).
