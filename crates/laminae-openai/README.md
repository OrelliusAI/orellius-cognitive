# laminae-openai

OpenAI-compatible backend for [Laminae](https://github.com/Orellius/laminae) -- works with OpenAI, Groq, Together AI, DeepSeek, and any server that implements the `/chat/completions` endpoint.

Part of the [Laminae](https://github.com/Orellius/laminae) SDK.

## Features

- Blocking and streaming completions via the OpenAI chat completions API
- Builder pattern for ergonomic configuration
- Built-in constructors for popular providers (Groq, Together, DeepSeek)
- Local server support (Ollama, vLLM, llama.cpp)
- Configurable model, temperature, max tokens, and timeout
- API key read from environment or passed directly

## Quick Start

```rust
use laminae_openai::OpenAIBackend;
use laminae_psyche::EgoBackend;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let openai = OpenAIBackend::from_env()?;
    let response = openai.complete(
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
let openai = OpenAIBackend::from_env()?;
let mut rx = openai.complete_streaming(
    "You are helpful.", "Hello!", "",
).await?;

while let Some(chunk) = rx.recv().await {
    print!("{chunk}");
}
```

## Compatible Providers

```rust
use laminae_openai::OpenAIBackend;

// Groq
let groq = OpenAIBackend::groq("gsk_...");

// Together AI
let together = OpenAIBackend::together("tok_...");

// DeepSeek
let deepseek = OpenAIBackend::deepseek("sk-...");

// Local server (Ollama, vLLM, llama.cpp)
let local = OpenAIBackend::local("http://localhost:11434/v1");
```

## Custom Configuration

```rust
use laminae_openai::{OpenAIBackend, OpenAIConfig};

let mut config = OpenAIConfig::default();
config.api_key = "sk-...".to_string();
config.model = "gpt-4o-mini".to_string();
config.max_tokens = Some(4096);
config.temperature = Some(0.7);
let openai = OpenAIBackend::with_config(config)?;
```

Or use the builder pattern:

```rust
let openai = OpenAIBackend::new("sk-...")
    .with_model("gpt-4o-mini")
    .with_temperature(0.5)
    .with_max_tokens(4096);
```

## Installation

```sh
cargo add laminae-openai@0.3
```

## License

Apache-2.0 -- see [LICENSE](../../LICENSE).
