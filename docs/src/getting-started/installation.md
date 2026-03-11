# Installation

## Full Stack

Add Laminae to your `Cargo.toml`:

```toml
[dependencies]
laminae = "0.4"
tokio = { version = "1", features = ["full"] }
```

## With LLM Backends

```toml
# Claude (Anthropic)
laminae = { version = "0.4", features = ["anthropic"] }

# OpenAI / Groq / Together / DeepSeek / local
laminae = { version = "0.4", features = ["openai"] }

# All backends
laminae = { version = "0.4", features = ["all-backends"] }
```

## Individual Layers

Pick only what you need:

```toml
[dependencies]
laminae-psyche = "0.4"       # Cognitive pipeline
laminae-persona = "0.4"      # Voice extraction & enforcement
laminae-cortex = "0.4"       # Learning loop
laminae-shadow = "0.4"       # Red-teaming
laminae-glassbox = "0.4"     # I/O containment
laminae-ironclad = "0.4"     # Process sandbox
laminae-ollama = "0.4"       # Ollama client
laminae-anthropic = "0.4"    # Claude EgoBackend
laminae-openai = "0.4"       # OpenAI-compatible EgoBackend
```

## Requirements

- **Rust 1.75+**
- **Ollama** (for Psyche, Persona, Cortex, and Shadow LLM features)
  ```bash
  # macOS
  brew install ollama && ollama serve

  # Linux
  curl -fsSL https://ollama.com/install.sh | sh && ollama serve
  ```
- **macOS, Linux, or Windows** (for Ironclad's process sandbox)
  - macOS: Full Seatbelt sandbox
  - Linux: Kernel namespaces + rlimits
  - Windows: Job Object resource limits + env scrubbing
