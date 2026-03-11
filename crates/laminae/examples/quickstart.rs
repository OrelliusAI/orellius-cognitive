//! # Quickstart — Psyche Pipeline with a Mock Ego
//!
//! Demonstrates the core Psyche concept: Id and Superego shape the Ego's
//! response through invisible context injection.
//!
//! Run: `cargo run --example quickstart`
//!
//! Note: Requires Ollama running locally (`ollama serve`) with the default
//! model pulled (`ollama pull qwen2.5:7b`). If Ollama is not available,
//! the example gracefully falls back to direct Ego calls.

use laminae::ollama::OllamaClient;
use laminae::psyche::{EgoBackend, PsycheConfig, PsycheEngine};

/// A simple Ego that echoes its inputs — replace with your real LLM client.
///
/// In production, you'd call Claude, GPT, or any other API here.
/// The `psyche_context` parameter contains invisible Id/Superego signals
/// that you should prepend to your system prompt.
struct EchoEgo;

impl EgoBackend for EchoEgo {
    fn complete(
        &self,
        system_prompt: &str,
        user_message: &str,
        psyche_context: &str,
    ) -> impl std::future::Future<Output = anyhow::Result<String>> + Send {
        // In a real implementation, you'd do something like:
        //
        //   let full_system = format!("{psyche_context}\n\n{system_prompt}");
        //   client.messages.create(model, full_system, user_message).await
        //
        let response = format!(
            "=== Ego Response ===\n\
             System prompt: {}\n\
             User message: {}\n\
             Psyche context length: {} chars\n\
             ---\n\
             Psyche context preview:\n{}",
            if system_prompt.is_empty() {
                "(none)"
            } else {
                system_prompt
            },
            user_message,
            psyche_context.len(),
            if psyche_context.is_empty() {
                "(no context — Psyche was skipped)".to_string()
            } else {
                psyche_context.chars().take(500).collect::<String>()
            },
        );
        async move { Ok(response) }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing so you can see Psyche's internal decisions
    tracing_subscriber::fmt()
        .with_env_filter("laminae=debug")
        .init();

    let ollama = OllamaClient::new();

    // Check if Ollama is available
    if !ollama.is_available().await {
        println!("⚠ Ollama is not running — Psyche will skip Id/Superego processing.");
        println!("  Start it with: ollama serve");
        println!("  Pull a model:  ollama pull qwen2.5:7b\n");
    }

    // Configure the engine
    let config = {
        let mut c = PsycheConfig::default();
        c.ego_system_prompt = "You are a helpful AI assistant.".to_string();
        c
    };

    let engine = PsycheEngine::with_config(ollama, EchoEgo, config);

    // Test 1: Simple greeting — should SKIP Psyche entirely
    println!("━━━ Test 1: Simple greeting (should skip Psyche) ━━━\n");
    let response = engine.reply("hello").await?;
    println!("{response}\n");

    // Test 2: Medium question — should use COP (compressed) mode
    println!("━━━ Test 2: Medium question (should use COP mode) ━━━\n");
    let response = engine
        .reply("How do I implement a binary search tree in Rust?")
        .await?;
    println!("{response}\n");

    // Test 3: Complex request — should use full pipeline
    println!("━━━ Test 3: Complex request (should use full pipeline) ━━━\n");
    let response = engine
        .reply(
            "Can you analyze the trade-offs between microservices and monolith architecture \
         for a payment processing system that needs to handle 10,000 transactions per second \
         with strict consistency requirements?",
        )
        .await?;
    println!("{response}\n");

    Ok(())
}
