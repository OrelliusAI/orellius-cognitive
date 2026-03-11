# OpenAI / Compatible Backend

The `laminae-openai` crate provides an `EgoBackend` for any OpenAI-compatible API: OpenAI, Groq, Together, DeepSeek, or local servers (Ollama, vLLM, etc.).

## Setup

```toml
[dependencies]
laminae = { version = "0.4", features = ["openai"] }
tokio = { version = "1", features = ["full"] }
```

## Usage

```rust
use laminae::psyche::PsycheEngine;
use laminae::openai::OpenAIBackend;
use laminae::ollama::OllamaClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let ego = OpenAIBackend::from_env()?; // reads OPENAI_API_KEY from env
    let engine = PsycheEngine::new(OllamaClient::new(), ego);

    let response = engine.reply("Write a haiku about Rust.").await?;
    println!("{response}");
    Ok(())
}
```

## Compatible Providers

| Provider | Base URL | Model Example |
|----------|----------|---------------|
| OpenAI | `https://api.openai.com/v1` (default) | `gpt-4o` |
| Groq | `https://api.groq.com/openai/v1` | `llama-3.1-70b-versatile` |
| Together | `https://api.together.xyz/v1` | `meta-llama/Llama-3-70b` |
| DeepSeek | `https://api.deepseek.com/v1` | `deepseek-chat` |
| Local (Ollama) | `http://localhost:11434/v1` | `qwen2.5:14b` |

```bash
# OpenAI
OPENAI_API_KEY=sk-... cargo run

# Groq
OPENAI_API_KEY=gsk_... OPENAI_BASE_URL=https://api.groq.com/openai/v1 OPENAI_MODEL=llama-3.1-70b-versatile cargo run

# Local Ollama
OPENAI_API_KEY=ollama OPENAI_BASE_URL=http://localhost:11434/v1 OPENAI_MODEL=qwen2.5:14b cargo run
```
