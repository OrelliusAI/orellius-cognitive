//! # Claude EgoBackend — Using Anthropic's API as the Ego
//!
//! Shows how to implement `EgoBackend` for Claude via the Anthropic API.
//! This is a minimal, dependency-free implementation using raw HTTP —
//! no Anthropic SDK required.
//!
//! Run: `ANTHROPIC_API_KEY=sk-ant-... cargo run -p laminae --example ego_claude`
//!
//! Requires:
//! - `ANTHROPIC_API_KEY` environment variable
//! - Ollama running locally (for Id/Superego)

use laminae::ollama::OllamaClient;
use laminae::psyche::{EgoBackend, PsycheConfig, PsycheEngine};

/// Claude Ego backend — calls the Anthropic Messages API.
struct ClaudeEgo {
    api_key: String,
    model: String,
    http: reqwest::Client,
}

impl ClaudeEgo {
    fn new(api_key: String) -> Self {
        Self {
            api_key,
            model: "claude-sonnet-4-6".to_string(),
            http: reqwest::Client::new(),
        }
    }

    #[allow(dead_code)]
    fn with_model(api_key: String, model: &str) -> Self {
        Self {
            api_key,
            model: model.to_string(),
            http: reqwest::Client::new(),
        }
    }
}

impl EgoBackend for ClaudeEgo {
    fn complete(
        &self,
        system_prompt: &str,
        user_message: &str,
        psyche_context: &str,
    ) -> impl std::future::Future<Output = anyhow::Result<String>> + Send {
        // Prepend Psyche's invisible context to the system prompt
        let full_system = if psyche_context.is_empty() {
            system_prompt.to_string()
        } else {
            format!("{psyche_context}\n\n{system_prompt}")
        };

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 1024,
            "system": full_system,
            "messages": [{
                "role": "user",
                "content": user_message
            }]
        });

        let req = self
            .http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body);

        async move {
            let resp = req.send().await?;
            let status = resp.status();

            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("Claude API error ({status}): {body}");
            }

            let json: serde_json::Value = resp.json().await?;

            let text = json["content"]
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|block| block["text"].as_str())
                .unwrap_or("")
                .to_string();

            Ok(text)
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("laminae=info")
        .init();

    let api_key =
        std::env::var("ANTHROPIC_API_KEY").expect("Set ANTHROPIC_API_KEY environment variable");

    let ollama = OllamaClient::new();
    let ego = ClaudeEgo::new(api_key);

    let config = {
        let mut c = PsycheConfig::default();
        c.ego_system_prompt = "You are a helpful, concise AI assistant.".into();
        c
    };

    let engine = PsycheEngine::with_config(ollama, ego, config);

    // Check readiness
    let readiness = engine.check_readiness().await;
    println!("Psyche readiness: {:?}\n", readiness);

    // Simple message — skips Psyche
    println!("━━━ Simple (skips Psyche) ━━━");
    let response = engine.reply("hello").await?;
    println!("{response}\n");

    // Complex message — full pipeline
    println!("━━━ Complex (full pipeline) ━━━");
    let response = engine
        .reply(
            "Analyze the security implications of using JWT tokens stored in localStorage \
         versus httpOnly cookies for a banking application.",
        )
        .await?;
    println!("{response}\n");

    Ok(())
}
