//! # OpenAI EgoBackend — Using GPT as the Ego
//!
//! Shows how to implement `EgoBackend` for OpenAI's Chat Completions API.
//! Minimal raw HTTP implementation — no OpenAI SDK required.
//!
//! Run: `OPENAI_API_KEY=sk-... cargo run -p laminae --example ego_openai`
//!
//! Requires:
//! - `OPENAI_API_KEY` environment variable
//! - Ollama running locally (for Id/Superego)

use laminae::ollama::OllamaClient;
use laminae::psyche::{EgoBackend, PsycheConfig, PsycheEngine};
use tokio::sync::mpsc;

/// OpenAI Ego backend — calls the Chat Completions API.
///
/// Supports both blocking and streaming responses.
struct OpenAIEgo {
    api_key: String,
    model: String,
    http: reqwest::Client,
}

impl OpenAIEgo {
    fn new(api_key: String) -> Self {
        Self {
            api_key,
            model: "gpt-4o".to_string(),
            http: reqwest::Client::new(),
        }
    }
}

impl EgoBackend for OpenAIEgo {
    fn complete(
        &self,
        system_prompt: &str,
        user_message: &str,
        psyche_context: &str,
    ) -> impl std::future::Future<Output = anyhow::Result<String>> + Send {
        let full_system = if psyche_context.is_empty() {
            system_prompt.to_string()
        } else {
            format!("{psyche_context}\n\n{system_prompt}")
        };

        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": full_system },
                { "role": "user", "content": user_message }
            ]
        });

        let req = self
            .http
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&body);

        async move {
            let resp = req.send().await?;
            let status = resp.status();

            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("OpenAI API error ({status}): {body}");
            }

            let json: serde_json::Value = resp.json().await?;

            let text = json["choices"]
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|choice| choice["message"]["content"].as_str())
                .unwrap_or("")
                .to_string();

            Ok(text)
        }
    }

    /// Streaming implementation using OpenAI's SSE stream.
    fn complete_streaming(
        &self,
        system_prompt: &str,
        user_message: &str,
        psyche_context: &str,
    ) -> impl std::future::Future<Output = anyhow::Result<mpsc::Receiver<String>>> + Send {
        let full_system = if psyche_context.is_empty() {
            system_prompt.to_string()
        } else {
            format!("{psyche_context}\n\n{system_prompt}")
        };

        let body = serde_json::json!({
            "model": self.model,
            "stream": true,
            "messages": [
                { "role": "system", "content": full_system },
                { "role": "user", "content": user_message }
            ]
        });

        let req = self
            .http
            .post("https://api.openai.com/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json")
            .json(&body);

        async move {
            let resp = req.send().await?;

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                anyhow::bail!("OpenAI streaming error ({status}): {body}");
            }

            let (tx, rx) = mpsc::channel(64);

            tokio::spawn(async move {
                use futures_util::StreamExt;

                let mut stream = resp.bytes_stream();
                let mut buffer = String::new();

                while let Some(chunk) = stream.next().await {
                    let bytes = match chunk {
                        Ok(b) => b,
                        Err(_) => break,
                    };

                    buffer.push_str(&String::from_utf8_lossy(&bytes));

                    while let Some(newline_pos) = buffer.find('\n') {
                        let line = buffer[..newline_pos].to_string();
                        buffer = buffer[newline_pos + 1..].to_string();

                        let line = line.trim();
                        if line.is_empty() || !line.starts_with("data: ") {
                            continue;
                        }

                        let data = &line[6..];
                        if data == "[DONE]" {
                            return;
                        }

                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                            if let Some(content) = json["choices"]
                                .as_array()
                                .and_then(|a| a.first())
                                .and_then(|c| c["delta"]["content"].as_str())
                            {
                                if !content.is_empty()
                                    && tx.send(content.to_string()).await.is_err()
                                {
                                    return;
                                }
                            }
                        }
                    }
                }
            });

            Ok(rx)
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("laminae=info")
        .init();

    let api_key = std::env::var("OPENAI_API_KEY").expect("Set OPENAI_API_KEY environment variable");

    let ollama = OllamaClient::new();
    let ego = OpenAIEgo::new(api_key);

    let config = {
        let mut c = PsycheConfig::default();
        c.ego_system_prompt = "You are a helpful, concise AI assistant.".into();
        c
    };

    let engine = PsycheEngine::with_config(ollama, ego, config);

    // Check readiness
    let readiness = engine.check_readiness().await;
    println!("Psyche readiness: {:?}\n", readiness);

    // Complex message — full Psyche pipeline
    println!("━━━ Full Pipeline (Id + Superego → GPT-4o) ━━━\n");
    let response = engine
        .reply(
            "Compare the trade-offs between using Rust and Go for building \
         a high-throughput message queue system.",
        )
        .await?;
    println!("{response}\n");

    // Streaming example
    println!("━━━ Streaming ━━━\n");
    let mut rx = engine
        .reply_streaming("Explain monads in one paragraph.")
        .await?;

    use laminae::psyche::PsycheEvent;
    while let Some(event) = rx.recv().await {
        match event {
            PsycheEvent::PhaseChange { phase } => {
                println!("[Phase: {phase:?}]");
            }
            PsycheEvent::EgoChunk { text } => {
                print!("{text}");
            }
            _ => {}
        }
    }
    println!("\n");

    Ok(())
}
