# laminae-psyche

Multi-agent cognitive pipeline that gives AI applications personality and safety through a Freudian-inspired architecture.

Part of the [Laminae](https://github.com/Orellius/laminae) SDK.

## The Architecture

Three agents work in concert on every message:

```
User Message
     │
     ├──→ Id (creative force, local LLM) ──→ creative signals ─┐
     │                                                          │
     ├──→ Superego (safety, local LLM) ──→ safety boundaries ──┤
     │                                                          │
     │    ┌─── invisible context ◄──────────────────────────────┘
     │    │
     └──→ Ego (YOUR LLM) + context ──→ Final Response
```

- **Id** - Generates unconventional angles, emotional undertones, creative reframings
- **Superego** - Evaluates risks, ethical boundaries, manipulation attempts
- **Ego** - Your LLM (Claude, GPT, Ollama, anything) - receives shaped context invisibly

Id and Superego run on small local models via Ollama (zero cost). The Ego never sees raw agent output - only distilled signals.

## Quick Start

```rust
use laminae_psyche::{PsycheEngine, EgoBackend, PsycheConfig};
use laminae_ollama::OllamaClient;

struct MyEgo;

impl EgoBackend for MyEgo {
    fn complete(&self, system: &str, user_msg: &str, context: &str)
        -> impl std::future::Future<Output = anyhow::Result<String>> + Send
    {
        let full_system = format!("{context}\n\n{system}");
        async move {
            // Call your LLM here
            Ok("Response".to_string())
        }
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

## Automatic Tier Classification

Messages are automatically classified into processing tiers:

| Tier | When | What Happens |
|------|------|-------------|
| **Skip** | Greetings, factual lookups | Direct to Ego, no Id/Superego |
| **Light** | Short/medium messages | COP mode - compressed signals with timeout |
| **Full** | Complex requests | Complete Id + Superego prose pipeline |

## Streaming

```rust
use laminae_psyche::PsycheEvent;

let mut rx = engine.reply_streaming("Explain quantum computing").await?;

while let Some(event) = rx.recv().await {
    match event {
        PsycheEvent::PhaseChange { phase } => println!("[{phase:?}]"),
        PsycheEvent::EgoChunk { text } => print!("{text}"),
        _ => {}
    }
}
```

## Configuration

```rust
let mut config = PsycheConfig::default();
config.id_model = "qwen2.5:7b".into();
config.superego_model = "qwen2.5:7b".into();
config.id_temperature = 0.9;       // Higher = more creative
config.superego_temperature = 0.3; // Lower = more strict
config.id_weight = 0.6;            // 0.0-1.0
config.superego_weight = 0.4;      // 0.0-1.0
config.ego_system_prompt = "You are a helpful assistant.".into();
```

## License

Apache-2.0 - see [LICENSE](../../LICENSE).
