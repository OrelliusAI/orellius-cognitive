# laminae

**The missing layer between raw LLMs and production AI.**

Meta-crate that re-exports all Laminae layers. Add this one dependency to get the full stack.

## Installation

```toml
[dependencies]
laminae = "0.1"
tokio = { version = "1", features = ["full"] }
```

Or pick individual layers:

```toml
laminae-psyche = "0.1"    # Multi-agent cognitive pipeline
laminae-persona = "0.1"   # Voice extraction & enforcement
laminae-cortex = "0.1"    # Self-improving learning loop
laminae-shadow = "0.1"    # Adversarial red-teaming
laminae-glassbox = "0.1"  # I/O containment
laminae-ironclad = "0.1"  # Process sandbox
laminae-ollama = "0.1"    # Ollama client
```

## The Layers

| Layer | Module | What It Does |
|-------|--------|-------------|
| **Psyche** | `laminae::psyche` | Id + Superego shape the Ego's response with invisible context |
| **Persona** | `laminae::persona` | Voice extraction from samples, style enforcement, AI phrase detection |
| **Cortex** | `laminae::cortex` | Tracks user edits, detects patterns, learns reusable instructions |
| **Shadow** | `laminae::shadow` | Automated security auditing of AI output |
| **Ironclad** | `laminae::ironclad` | Command whitelist, network sandbox, resource watchdog |
| **Glassbox** | `laminae::glassbox` | Input/output validation, rate limiting, path protection |

Plus `laminae::ollama` for local LLM inference.

## Quick Example

```rust
use laminae::psyche::{PsycheEngine, EgoBackend};
use laminae::ollama::OllamaClient;

struct MyEgo;

impl EgoBackend for MyEgo {
    fn complete(&self, _system: &str, user_msg: &str, _ctx: &str)
        -> impl std::future::Future<Output = anyhow::Result<String>> + Send
    {
        let msg = user_msg.to_string();
        async move { Ok(format!("Response to: {msg}")) }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let engine = PsycheEngine::new(OllamaClient::new(), MyEgo);
    let response = engine.reply("What is creativity?").await?;
    println!("{response}");
    Ok(())
}
```

See the [examples](https://github.com/Orellius/laminae/tree/main/crates/laminae/examples) for Claude API, OpenAI API, Shadow auditing, and full-stack integration.

## License

Apache-2.0 — Copyright 2026 Orel Ohayon.
