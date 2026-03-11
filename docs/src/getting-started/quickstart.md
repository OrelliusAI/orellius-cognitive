# Quick Start

This guide walks through the Psyche cognitive pipeline with a mock LLM backend.

## 1. Set Up the Project

```bash
cargo new my-ai-app && cd my-ai-app
```

```toml
# Cargo.toml
[dependencies]
laminae = "0.4"
tokio = { version = "1", features = ["full"] }
anyhow = "1"
```

## 2. Implement EgoBackend

The `EgoBackend` trait is how Laminae talks to your LLM:

```rust
use laminae::psyche::{PsycheEngine, EgoBackend, PsycheConfig};
use laminae::ollama::OllamaClient;

struct MyEgo;

impl EgoBackend for MyEgo {
    fn complete(
        &self,
        system: &str,
        user_msg: &str,
        context: &str,
    ) -> impl std::future::Future<Output = anyhow::Result<String>> + Send {
        let full_system = format!("{context}\n\n{system}");
        async move {
            // Replace with your actual LLM call
            Ok(format!("Response to: {user_msg}"))
        }
    }
}
```

## 3. Run the Pipeline

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let engine = PsycheEngine::new(OllamaClient::new(), MyEgo);
    let response = engine.reply("What is creativity?").await?;
    println!("{response}");
    Ok(())
}
```

## 4. Run It

```bash
# Make sure Ollama is running first
ollama serve &
cargo run
```

The Psyche pipeline will:
1. Send your message to **Id** (creative agent) and **Superego** (safety agent) on Ollama
2. Compress their signals into invisible context
3. Forward everything to your **Ego** (your LLM) for the final response

## What's Next?

- Use a [real LLM backend](../backends/claude.md) instead of the mock
- Add [voice enforcement](../layers/persona.md) to match a specific writing style
- Enable [red-teaming](../layers/shadow.md) to audit AI output for vulnerabilities
- Set up [safe code execution](../recipes/safe-execution.md) with Ironclad + Glassbox
