//! # laminae-anthropic — Anthropic Claude Backend for Laminae
//!
//! First-class [`EgoBackend`] implementation for the Anthropic Messages API.
//! Supports both blocking and streaming completions via Claude models.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use laminae_anthropic::ClaudeBackend;
//! use laminae_psyche::EgoBackend;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let claude = ClaudeBackend::from_env()?;
//!     let response = claude.complete(
//!         "You are a helpful assistant.",
//!         "What is Rust?",
//!         "",
//!     ).await?;
//!     println!("{response}");
//!     Ok(())
//! }
//! ```
//!
//! ## With Psyche Engine
//!
//! ```rust,ignore
//! use laminae_anthropic::ClaudeBackend;
//! use laminae_psyche::PsycheEngine;
//! use laminae_ollama::OllamaClient;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let claude = ClaudeBackend::from_env()?;
//!     let engine = PsycheEngine::new(OllamaClient::new(), claude);
//!     let response = engine.reply("Explain quantum computing").await?;
//!     println!("{response}");
//!     Ok(())
//! }
//! ```

use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use laminae_psyche::EgoBackend;

// ── Typed Errors ──

/// Typed errors for the Anthropic Claude backend.
#[derive(Debug, Error)]
pub enum ClaudeError {
    /// Failed to connect to the Anthropic API.
    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    /// Request timed out.
    #[error("request timed out")]
    Timeout,

    /// The API returned a response that could not be parsed.
    #[error("invalid response: {0}")]
    InvalidResponse(String),

    /// Authentication failed (invalid or missing API key).
    #[error("authentication failed: {0}")]
    AuthError(String),

    /// The API returned an HTTP error with a structured message.
    #[error("API error (HTTP {status}): [{error_type}] {message}")]
    ApiError {
        status: u16,
        error_type: String,
        message: String,
    },

    /// Rate limit exceeded.
    #[error("rate limited: retry after {retry_after_secs}s")]
    RateLimited { retry_after_secs: u64 },

    /// Failed to build the HTTP client (e.g. missing TLS certificates).
    #[error("HTTP client error: {0}")]
    HttpClient(String),
}

// ── Configuration ──

/// Configuration for the Anthropic Claude backend.
///
/// # Examples
///
/// ```
/// use laminae_anthropic::ClaudeConfig;
///
/// let config = {
///     let mut c = ClaudeConfig::default();
///     c.model = "claude-sonnet-4-20250514".to_string();
///     c.max_tokens = 4096;
///     c.temperature = Some(0.7);
///     c
/// };
/// assert_eq!(config.model, "claude-sonnet-4-20250514");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ClaudeConfig {
    /// Anthropic API key.
    #[serde(skip_serializing, default)]
    pub api_key: String,

    /// Model identifier (e.g., "claude-sonnet-4-20250514", "claude-opus-4-20250514").
    #[serde(default = "default_model")]
    pub model: String,

    /// Maximum tokens to generate.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,

    /// API base URL. Override for proxies or compatible endpoints.
    #[serde(default = "default_base_url")]
    pub base_url: String,

    /// Sampling temperature (0.0 - 1.0). `None` uses the API default.
    #[serde(default)]
    pub temperature: Option<f32>,

    /// Request timeout.
    #[serde(default = "default_timeout")]
    pub timeout: Duration,
}

fn default_model() -> String {
    "claude-sonnet-4-20250514".to_string()
}

fn default_max_tokens() -> u32 {
    4096
}

fn default_base_url() -> String {
    "https://api.anthropic.com".to_string()
}

fn default_timeout() -> Duration {
    Duration::from_secs(120)
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: default_model(),
            max_tokens: default_max_tokens(),
            base_url: default_base_url(),
            temperature: None,
            timeout: default_timeout(),
        }
    }
}

impl ClaudeConfig {
    /// Clamp temperature to the valid range [0.0, 1.0].
    pub fn clamp(&mut self) {
        if let Some(t) = &mut self.temperature {
            *t = t.clamp(0.0, 1.0);
        }
    }
}

// ── Backend ──

/// Anthropic Claude backend implementing [`EgoBackend`].
///
/// Sends requests to the Anthropic Messages API (`/v1/messages`).
///
/// # Examples
///
/// ```rust,no_run
/// use laminae_anthropic::ClaudeBackend;
///
/// // From environment variable ANTHROPIC_API_KEY
/// let claude = ClaudeBackend::from_env().unwrap();
///
/// // From explicit key
/// let claude = ClaudeBackend::new("sk-ant-...");
///
/// // With builder methods
/// let claude = ClaudeBackend::new("sk-ant-...")
///     .with_model("claude-opus-4-20250514")
///     .with_temperature(0.5)
///     .with_max_tokens(8192);
/// ```
#[derive(Debug)]
pub struct ClaudeBackend {
    client: Client,
    config: ClaudeConfig,
}

impl ClaudeBackend {
    /// Create a new backend with the given API key and default settings.
    ///
    /// Uses `claude-sonnet-4-20250514` as the default model.
    ///
    /// # Examples
    ///
    /// ```
    /// use laminae_anthropic::ClaudeBackend;
    ///
    /// let claude = ClaudeBackend::new("sk-ant-test-key");
    /// ```
    pub fn new(api_key: impl Into<String>) -> Self {
        let config = ClaudeConfig {
            api_key: api_key.into(),
            ..Default::default()
        };
        Self::with_config(config).expect("failed to build HTTP client with default config")
    }

    /// Create a backend from the `ANTHROPIC_API_KEY` environment variable.
    ///
    /// Returns an error with a descriptive message if the variable is not set.
    ///
    /// # Errors
    ///
    /// Returns `Err` if `ANTHROPIC_API_KEY` is not set or is empty.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use laminae_anthropic::ClaudeBackend;
    ///
    /// let claude = ClaudeBackend::from_env()
    ///     .expect("ANTHROPIC_API_KEY must be set");
    /// ```
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").context(
            "ANTHROPIC_API_KEY environment variable not set. \
             Get your API key from https://console.anthropic.com/settings/keys",
        )?;

        if api_key.is_empty() {
            anyhow::bail!(
                "ANTHROPIC_API_KEY environment variable is empty. \
                 Set it to your Anthropic API key."
            );
        }

        Ok(Self::with_config(ClaudeConfig {
            api_key,
            ..Default::default()
        })?)
    }

    /// Create a backend with full configuration control.
    ///
    /// # Examples
    ///
    /// ```
    /// use laminae_anthropic::{ClaudeBackend, ClaudeConfig};
    ///
    /// let config = {
    ///     let mut c = ClaudeConfig::default();
    ///     c.api_key = "sk-ant-test".to_string();
    ///     c.model = "claude-opus-4-20250514".to_string();
    ///     c.max_tokens = 8192;
    ///     c
    /// };
    /// let claude = ClaudeBackend::with_config(config)?;
    /// # Ok::<(), laminae_anthropic::ClaudeError>(())
    /// ```
    pub fn with_config(mut config: ClaudeConfig) -> Result<Self, ClaudeError> {
        config.clamp();
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| ClaudeError::HttpClient(e.to_string()))?;
        Ok(Self { client, config })
    }

    /// Set the model to use.
    ///
    /// # Examples
    ///
    /// ```
    /// use laminae_anthropic::ClaudeBackend;
    ///
    /// let claude = ClaudeBackend::new("key")
    ///     .with_model("claude-opus-4-20250514");
    /// ```
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.config.model = model.into();
        self
    }

    /// Set the sampling temperature (clamped to 0.0 - 1.0).
    ///
    /// # Examples
    ///
    /// ```
    /// use laminae_anthropic::ClaudeBackend;
    ///
    /// let claude = ClaudeBackend::new("key").with_temperature(0.7);
    /// ```
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.config.temperature = Some(temperature.clamp(0.0, 1.0));
        self
    }

    /// Set the maximum number of tokens to generate.
    ///
    /// # Examples
    ///
    /// ```
    /// use laminae_anthropic::ClaudeBackend;
    ///
    /// let claude = ClaudeBackend::new("key").with_max_tokens(8192);
    /// ```
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.config.max_tokens = max_tokens;
        self
    }

    /// Access the current configuration.
    pub fn config(&self) -> &ClaudeConfig {
        &self.config
    }

    /// Build the system text by merging system_prompt and psyche_context.
    fn build_system(system_prompt: &str, psyche_context: &str) -> String {
        match (system_prompt.is_empty(), psyche_context.is_empty()) {
            (true, true) => String::new(),
            (true, false) => psyche_context.to_string(),
            (false, true) => system_prompt.to_string(),
            (false, false) => format!("{system_prompt}\n\n{psyche_context}"),
        }
    }

    /// Build the JSON request body for the Messages API.
    fn build_request_body(
        &self,
        system: &str,
        user_message: &str,
        stream: bool,
    ) -> serde_json::Value {
        let mut body = serde_json::json!({
            "model": self.config.model,
            "max_tokens": self.config.max_tokens,
            "messages": [
                { "role": "user", "content": user_message }
            ],
        });

        if !system.is_empty() {
            body["system"] = serde_json::json!(system);
        }

        if let Some(temp) = self.config.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        if stream {
            body["stream"] = serde_json::json!(true);
        }

        body
    }

    /// Send a request to the Anthropic Messages API.
    fn build_http_request(&self, body: &serde_json::Value) -> reqwest::RequestBuilder {
        let url = format!("{}/v1/messages", self.config.base_url);
        self.client
            .post(&url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(body)
    }
}

// ── API Response Types ──

#[derive(Debug, Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    #[serde(default)]
    text: String,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: ApiError,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
    #[serde(rename = "type")]
    error_type: String,
}

#[derive(Debug, Deserialize)]
struct StreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: Option<StreamDelta>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    #[serde(default)]
    text: Option<String>,
}

// ── EgoBackend Implementation ──

impl EgoBackend for ClaudeBackend {
    /// Send a completion request to the Anthropic Messages API.
    ///
    /// Merges `system_prompt` and `psyche_context` into the `system` field,
    /// sends the user message, and returns the text content from the response.
    fn complete(
        &self,
        system_prompt: &str,
        user_message: &str,
        psyche_context: &str,
    ) -> impl std::future::Future<Output = Result<String>> + Send {
        let system = Self::build_system(system_prompt, psyche_context);
        let body = self.build_request_body(&system, user_message, false);
        let request = self.build_http_request(&body);

        async move {
            debug!(model = %body["model"], "Sending completion request to Anthropic");

            let response = request
                .send()
                .await
                .context("failed to send request to Anthropic API")?;

            let status = response.status();
            if !status.is_success() {
                let body_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "<failed to read response body>".to_string());

                // Try to parse structured error
                if let Ok(err) = serde_json::from_str::<ErrorResponse>(&body_text) {
                    anyhow::bail!(
                        "Anthropic API error (HTTP {status}): [{err_type}] {msg}",
                        err_type = err.error.error_type,
                        msg = err.error.message,
                    );
                }

                anyhow::bail!("Anthropic API error (HTTP {status}): {body_text}");
            }

            let result: MessagesResponse = response
                .json()
                .await
                .context("failed to parse Anthropic response JSON")?;

            let text = result
                .content
                .into_iter()
                .filter_map(|block| {
                    if block.text.is_empty() {
                        None
                    } else {
                        Some(block.text)
                    }
                })
                .collect::<Vec<_>>()
                .join("");

            if text.is_empty() {
                warn!("Anthropic returned empty content");
            }

            Ok(text)
        }
    }

    /// Stream a completion from the Anthropic Messages API.
    ///
    /// Returns an `mpsc::Receiver<String>` that yields text chunks as they
    /// arrive via Server-Sent Events.
    fn complete_streaming(
        &self,
        system_prompt: &str,
        user_message: &str,
        psyche_context: &str,
    ) -> impl std::future::Future<Output = Result<mpsc::Receiver<String>>> + Send {
        let system = Self::build_system(system_prompt, psyche_context);
        let body = self.build_request_body(&system, user_message, true);
        let request = self.build_http_request(&body);

        async move {
            debug!(model = %body["model"], "Sending streaming request to Anthropic");

            let response = request
                .send()
                .await
                .context("failed to send streaming request to Anthropic API")?;

            let status = response.status();
            if !status.is_success() {
                let body_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "<failed to read response body>".to_string());

                if let Ok(err) = serde_json::from_str::<ErrorResponse>(&body_text) {
                    anyhow::bail!(
                        "Anthropic API error (HTTP {status}): [{err_type}] {msg}",
                        err_type = err.error.error_type,
                        msg = err.error.message,
                    );
                }

                anyhow::bail!("Anthropic API error (HTTP {status}): {body_text}");
            }

            let (tx, rx) = mpsc::channel::<String>(64);

            let mut stream = response.bytes_stream();

            tokio::spawn(async move {
                let mut buffer = String::new();

                while let Some(chunk_result) = stream.next().await {
                    let chunk = match chunk_result {
                        Ok(bytes) => match String::from_utf8(bytes.to_vec()) {
                            Ok(s) => s,
                            Err(e) => {
                                warn!("Non-UTF8 chunk from Anthropic stream: {e}");
                                continue;
                            }
                        },
                        Err(e) => {
                            warn!("Stream read error: {e}");
                            break;
                        }
                    };

                    buffer.push_str(&chunk);

                    // Process complete SSE lines from the buffer
                    while let Some(line_end) = buffer.find('\n') {
                        let line = buffer[..line_end].trim_end_matches('\r').to_string();
                        buffer = buffer[line_end + 1..].to_string();

                        // SSE data lines start with "data: "
                        let data = match line.strip_prefix("data: ") {
                            Some(d) => d,
                            None => continue,
                        };

                        // End of stream marker
                        if data == "[DONE]" {
                            return;
                        }

                        // Parse the JSON event
                        let event: StreamEvent = match serde_json::from_str(data) {
                            Ok(e) => e,
                            Err(_) => continue,
                        };

                        // Extract text from content_block_delta events
                        if event.event_type == "content_block_delta" {
                            if let Some(delta) = event.delta {
                                if let Some(text) = delta.text {
                                    if !text.is_empty() && tx.send(text).await.is_err() {
                                        return;
                                    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = ClaudeConfig::default();
        assert_eq!(config.model, "claude-sonnet-4-20250514");
        assert_eq!(config.max_tokens, 4096);
        assert_eq!(config.base_url, "https://api.anthropic.com");
        assert!(config.temperature.is_none());
        assert_eq!(config.timeout, Duration::from_secs(120));
    }

    #[test]
    fn test_builder_pattern() {
        let claude = ClaudeBackend::new("test-key")
            .with_model("claude-opus-4-20250514")
            .with_temperature(0.7)
            .with_max_tokens(8192);

        assert_eq!(claude.config().model, "claude-opus-4-20250514");
        assert_eq!(claude.config().temperature, Some(0.7));
        assert_eq!(claude.config().max_tokens, 8192);
    }

    #[test]
    fn test_temperature_clamping() {
        let claude = ClaudeBackend::new("key").with_temperature(5.0);
        assert_eq!(claude.config().temperature, Some(1.0));

        let claude = ClaudeBackend::new("key").with_temperature(-1.0);
        assert_eq!(claude.config().temperature, Some(0.0));

        let claude = ClaudeBackend::new("key").with_temperature(0.5);
        assert_eq!(claude.config().temperature, Some(0.5));
    }

    #[test]
    fn test_config_clamp() {
        let mut config = ClaudeConfig {
            temperature: Some(2.5),
            ..Default::default()
        };
        config.clamp();
        assert_eq!(config.temperature, Some(1.0));
    }

    #[test]
    fn test_build_system_both() {
        let result = ClaudeBackend::build_system("You are helpful.", "Be creative.");
        assert_eq!(result, "You are helpful.\n\nBe creative.");
    }

    #[test]
    fn test_build_system_only_prompt() {
        let result = ClaudeBackend::build_system("You are helpful.", "");
        assert_eq!(result, "You are helpful.");
    }

    #[test]
    fn test_build_system_only_context() {
        let result = ClaudeBackend::build_system("", "Be creative.");
        assert_eq!(result, "Be creative.");
    }

    #[test]
    fn test_build_system_empty() {
        let result = ClaudeBackend::build_system("", "");
        assert_eq!(result, "");
    }

    #[test]
    fn test_request_body_basic() {
        let claude = ClaudeBackend::new("key");
        let body = claude.build_request_body("system text", "hello", false);

        assert_eq!(body["model"], "claude-sonnet-4-20250514");
        assert_eq!(body["max_tokens"], 4096);
        assert_eq!(body["system"], "system text");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "hello");
        assert!(body.get("stream").is_none());
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn test_request_body_with_temperature_and_stream() {
        let claude = ClaudeBackend::new("key").with_temperature(0.5);
        let body = claude.build_request_body("sys", "msg", true);

        assert_eq!(body["temperature"], 0.5);
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn test_request_body_no_system_when_empty() {
        let claude = ClaudeBackend::new("key");
        let body = claude.build_request_body("", "hello", false);
        assert!(body.get("system").is_none());
    }

    #[test]
    fn test_config_serialization() {
        let config = ClaudeConfig {
            api_key: "secret".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            max_tokens: 2048,
            temperature: Some(0.8),
            ..Default::default()
        };

        let json = serde_json::to_string(&config).unwrap();
        // API key should be skipped during serialization
        assert!(!json.contains("secret"));

        let parsed: ClaudeConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model, "claude-sonnet-4-20250514");
        assert_eq!(parsed.max_tokens, 2048);
        assert_eq!(parsed.temperature, Some(0.8));
        // Deserialized key should be empty (default)
        assert!(parsed.api_key.is_empty());
    }

    #[test]
    fn test_from_env_missing() {
        // Ensure env var is not set
        std::env::remove_var("ANTHROPIC_API_KEY");
        let result = ClaudeBackend::from_env();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("ANTHROPIC_API_KEY"));
    }
}
