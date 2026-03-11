//! # laminae-openai — OpenAI-Compatible Backend for Laminae
//!
//! First-class [`EgoBackend`] implementation for OpenAI and any API that
//! follows the OpenAI chat completions format (Groq, Together, DeepSeek,
//! local servers like Ollama's OpenAI-compatible endpoint, etc.).
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use laminae_openai::OpenAIBackend;
//! use laminae_psyche::EgoBackend;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let openai = OpenAIBackend::from_env()?;
//!     let response = openai.complete(
//!         "You are a helpful assistant.",
//!         "What is Rust?",
//!         "",
//!     ).await?;
//!     println!("{response}");
//!     Ok(())
//! }
//! ```
//!
//! ## Compatible Providers
//!
//! ```rust,no_run
//! use laminae_openai::OpenAIBackend;
//!
//! // Groq
//! let groq = OpenAIBackend::groq("gsk_...");
//!
//! // Together AI
//! let together = OpenAIBackend::together("tok_...");
//!
//! // DeepSeek
//! let deepseek = OpenAIBackend::deepseek("sk-...");
//!
//! // Local server (e.g., Ollama, vLLM, llama.cpp)
//! let local = OpenAIBackend::local("http://localhost:11434/v1");
//! ```
//!
//! ## With Psyche Engine
//!
//! ```rust,ignore
//! use laminae_openai::OpenAIBackend;
//! use laminae_psyche::PsycheEngine;
//! use laminae_ollama::OllamaClient;
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let openai = OpenAIBackend::from_env()?;
//!     let engine = PsycheEngine::new(OllamaClient::new(), openai);
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

/// Typed errors for the OpenAI-compatible backend.
#[derive(Debug, Error)]
pub enum OpenAIError {
    /// Failed to connect to the API server.
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

/// Configuration for an OpenAI-compatible backend.
///
/// Works with OpenAI, Groq, Together AI, DeepSeek, and any server
/// implementing the `/chat/completions` endpoint.
///
/// # Examples
///
/// ```
/// use laminae_openai::OpenAIConfig;
///
/// let config = {
///     let mut c = OpenAIConfig::default();
///     c.model = "gpt-4o".to_string();
///     c.max_tokens = Some(4096);
///     c.temperature = Some(0.7);
///     c
/// };
/// assert_eq!(config.base_url, "https://api.openai.com/v1");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OpenAIConfig {
    /// API key for authentication.
    #[serde(skip_serializing, default)]
    pub api_key: String,

    /// Model identifier (e.g., "gpt-4o", "llama-3.1-70b-versatile").
    #[serde(default = "default_model")]
    pub model: String,

    /// Base URL for the API (without trailing slash).
    #[serde(default = "default_base_url")]
    pub base_url: String,

    /// Maximum tokens to generate. `None` lets the API decide.
    #[serde(default)]
    pub max_tokens: Option<u32>,

    /// Sampling temperature (0.0 - 2.0). `None` uses the API default.
    #[serde(default)]
    pub temperature: Option<f32>,

    /// OpenAI organization ID. Optional.
    #[serde(default)]
    pub organization: Option<String>,

    /// Request timeout.
    #[serde(default = "default_timeout")]
    pub timeout: Duration,
}

fn default_model() -> String {
    "gpt-4o".to_string()
}

fn default_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

fn default_timeout() -> Duration {
    Duration::from_secs(120)
}

impl Default for OpenAIConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: default_model(),
            base_url: default_base_url(),
            max_tokens: None,
            temperature: None,
            organization: None,
            timeout: default_timeout(),
        }
    }
}

impl OpenAIConfig {
    /// Clamp temperature to the valid range [0.0, 2.0].
    pub fn clamp(&mut self) {
        if let Some(t) = &mut self.temperature {
            *t = t.clamp(0.0, 2.0);
        }
    }
}

// ── Backend ──

/// OpenAI-compatible backend implementing [`EgoBackend`].
///
/// Sends requests to any server implementing the OpenAI chat completions
/// API format (`/chat/completions`).
///
/// # Examples
///
/// ```rust,no_run
/// use laminae_openai::OpenAIBackend;
///
/// // From environment variable OPENAI_API_KEY
/// let openai = OpenAIBackend::from_env().unwrap();
///
/// // From explicit key
/// let openai = OpenAIBackend::new("sk-...");
///
/// // With builder methods
/// let openai = OpenAIBackend::new("sk-...")
///     .with_model("gpt-4o-mini")
///     .with_temperature(0.5)
///     .with_max_tokens(4096);
/// ```
#[derive(Debug)]
pub struct OpenAIBackend {
    client: Client,
    config: OpenAIConfig,
}

impl OpenAIBackend {
    /// Create a new backend with the given API key and default settings.
    ///
    /// Uses `gpt-4o` as the default model and `https://api.openai.com/v1`
    /// as the default base URL.
    ///
    /// # Examples
    ///
    /// ```
    /// use laminae_openai::OpenAIBackend;
    ///
    /// let openai = OpenAIBackend::new("sk-test-key");
    /// ```
    pub fn new(api_key: impl Into<String>) -> Self {
        let config = OpenAIConfig {
            api_key: api_key.into(),
            ..Default::default()
        };
        Self::with_config(config).expect("failed to build HTTP client with default config")
    }

    /// Create a backend from the `OPENAI_API_KEY` environment variable.
    ///
    /// # Errors
    ///
    /// Returns `Err` if `OPENAI_API_KEY` is not set or is empty.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use laminae_openai::OpenAIBackend;
    ///
    /// let openai = OpenAIBackend::from_env()
    ///     .expect("OPENAI_API_KEY must be set");
    /// ```
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("OPENAI_API_KEY").context(
            "OPENAI_API_KEY environment variable not set. \
             Get your API key from https://platform.openai.com/api-keys",
        )?;

        if api_key.is_empty() {
            anyhow::bail!(
                "OPENAI_API_KEY environment variable is empty. \
                 Set it to your OpenAI API key."
            );
        }

        Ok(Self::with_config(OpenAIConfig {
            api_key,
            ..Default::default()
        })?)
    }

    /// Create a backend with full configuration control.
    ///
    /// # Examples
    ///
    /// ```
    /// use laminae_openai::{OpenAIBackend, OpenAIConfig};
    ///
    /// let config = {
    ///     let mut c = OpenAIConfig::default();
    ///     c.api_key = "sk-test".to_string();
    ///     c.model = "gpt-4o-mini".to_string();
    ///     c.max_tokens = Some(2048);
    ///     c
    /// };
    /// let openai = OpenAIBackend::with_config(config)?;
    /// # Ok::<(), laminae_openai::OpenAIError>(())
    /// ```
    pub fn with_config(mut config: OpenAIConfig) -> Result<Self, OpenAIError> {
        config.clamp();
        let client = Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|e| OpenAIError::HttpClient(e.to_string()))?;
        Ok(Self { client, config })
    }

    // ── Convenience Constructors for Popular Providers ──

    /// Create a backend for [Groq](https://groq.com).
    ///
    /// Uses `llama-3.3-70b-versatile` as the default model.
    ///
    /// # Examples
    ///
    /// ```
    /// use laminae_openai::OpenAIBackend;
    ///
    /// let groq = OpenAIBackend::groq("gsk_test_key");
    /// assert_eq!(groq.config().base_url, "https://api.groq.com/openai/v1");
    /// ```
    pub fn groq(api_key: impl Into<String>) -> Self {
        Self::with_config(OpenAIConfig {
            api_key: api_key.into(),
            model: "llama-3.3-70b-versatile".to_string(),
            base_url: "https://api.groq.com/openai/v1".to_string(),
            ..Default::default()
        })
        .expect("failed to build HTTP client")
    }

    /// Create a backend for [Together AI](https://together.ai).
    ///
    /// Uses `meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo` as the default model.
    ///
    /// # Examples
    ///
    /// ```
    /// use laminae_openai::OpenAIBackend;
    ///
    /// let together = OpenAIBackend::together("tok_test_key");
    /// assert_eq!(together.config().base_url, "https://api.together.xyz/v1");
    /// ```
    pub fn together(api_key: impl Into<String>) -> Self {
        Self::with_config(OpenAIConfig {
            api_key: api_key.into(),
            model: "meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo".to_string(),
            base_url: "https://api.together.xyz/v1".to_string(),
            ..Default::default()
        })
        .expect("failed to build HTTP client")
    }

    /// Create a backend for [DeepSeek](https://deepseek.com).
    ///
    /// Uses `deepseek-chat` as the default model.
    ///
    /// # Examples
    ///
    /// ```
    /// use laminae_openai::OpenAIBackend;
    ///
    /// let deepseek = OpenAIBackend::deepseek("sk-test");
    /// assert_eq!(deepseek.config().base_url, "https://api.deepseek.com/v1");
    /// ```
    pub fn deepseek(api_key: impl Into<String>) -> Self {
        Self::with_config(OpenAIConfig {
            api_key: api_key.into(),
            model: "deepseek-chat".to_string(),
            base_url: "https://api.deepseek.com/v1".to_string(),
            ..Default::default()
        })
        .expect("failed to build HTTP client")
    }

    /// Create a backend for a local OpenAI-compatible server.
    ///
    /// No API key required. Uses `default` as the model name (most local
    /// servers accept any model name and use whichever is loaded).
    ///
    /// # Examples
    ///
    /// ```
    /// use laminae_openai::OpenAIBackend;
    ///
    /// // Ollama's OpenAI-compatible endpoint
    /// let local = OpenAIBackend::local("http://localhost:11434/v1");
    ///
    /// // llama.cpp server
    /// let local = OpenAIBackend::local("http://localhost:8080/v1");
    /// ```
    pub fn local(base_url: impl Into<String>) -> Self {
        Self::with_config(OpenAIConfig {
            api_key: String::new(),
            model: "default".to_string(),
            base_url: base_url.into(),
            ..Default::default()
        })
        .expect("failed to build HTTP client")
    }

    /// Set the model to use.
    ///
    /// # Examples
    ///
    /// ```
    /// use laminae_openai::OpenAIBackend;
    ///
    /// let openai = OpenAIBackend::new("key").with_model("gpt-4o-mini");
    /// ```
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.config.model = model.into();
        self
    }

    /// Set the sampling temperature (clamped to 0.0 - 2.0).
    ///
    /// # Examples
    ///
    /// ```
    /// use laminae_openai::OpenAIBackend;
    ///
    /// let openai = OpenAIBackend::new("key").with_temperature(0.7);
    /// ```
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.config.temperature = Some(temperature.clamp(0.0, 2.0));
        self
    }

    /// Set the maximum number of tokens to generate.
    ///
    /// # Examples
    ///
    /// ```
    /// use laminae_openai::OpenAIBackend;
    ///
    /// let openai = OpenAIBackend::new("key").with_max_tokens(4096);
    /// ```
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.config.max_tokens = Some(max_tokens);
        self
    }

    /// Set the OpenAI organization ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use laminae_openai::OpenAIBackend;
    ///
    /// let openai = OpenAIBackend::new("key")
    ///     .with_organization("org-abc123");
    /// ```
    pub fn with_organization(mut self, org: impl Into<String>) -> Self {
        self.config.organization = Some(org.into());
        self
    }

    /// Access the current configuration.
    pub fn config(&self) -> &OpenAIConfig {
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

    /// Build the JSON request body for the chat completions API.
    fn build_request_body(
        &self,
        system: &str,
        user_message: &str,
        stream: bool,
    ) -> serde_json::Value {
        let mut messages = Vec::new();

        if !system.is_empty() {
            messages.push(serde_json::json!({
                "role": "system",
                "content": system
            }));
        }

        messages.push(serde_json::json!({
            "role": "user",
            "content": user_message
        }));

        let mut body = serde_json::json!({
            "model": self.config.model,
            "messages": messages,
        });

        if let Some(max_tokens) = self.config.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
        }

        if let Some(temp) = self.config.temperature {
            body["temperature"] = serde_json::json!(temp);
        }

        if stream {
            body["stream"] = serde_json::json!(true);
        }

        body
    }

    /// Build an HTTP request for the chat completions endpoint.
    fn build_http_request(&self, body: &serde_json::Value) -> reqwest::RequestBuilder {
        let url = format!("{}/chat/completions", self.config.base_url);
        let mut req = self
            .client
            .post(&url)
            .header("content-type", "application/json");

        if !self.config.api_key.is_empty() {
            req = req.header("authorization", format!("Bearer {}", self.config.api_key));
        }

        if let Some(org) = &self.config.organization {
            req = req.header("openai-organization", org);
        }

        req.json(body)
    }
}

// ── API Response Types ──

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: MessageContent,
}

#[derive(Debug, Deserialize)]
struct MessageContent {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: ApiError,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: String,
    #[serde(default, rename = "type")]
    error_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
}

// ── EgoBackend Implementation ──

impl EgoBackend for OpenAIBackend {
    /// Send a chat completion request to an OpenAI-compatible API.
    ///
    /// Merges `system_prompt` and `psyche_context` into a system message,
    /// sends the user message, and returns the assistant's response content.
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
            debug!(model = %body["model"], "Sending completion request to OpenAI-compatible API");

            let response = request
                .send()
                .await
                .context("failed to send request to OpenAI-compatible API")?;

            let status = response.status();
            if !status.is_success() {
                let body_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "<failed to read response body>".to_string());

                if let Ok(err) = serde_json::from_str::<ErrorResponse>(&body_text) {
                    let err_type = err.error.error_type.as_deref().unwrap_or("unknown");
                    anyhow::bail!(
                        "OpenAI API error (HTTP {status}): [{err_type}] {msg}",
                        msg = err.error.message,
                    );
                }

                anyhow::bail!("OpenAI API error (HTTP {status}): {body_text}");
            }

            let result: ChatCompletionResponse = response
                .json()
                .await
                .context("failed to parse OpenAI response JSON")?;

            let text = result
                .choices
                .into_iter()
                .next()
                .and_then(|c| c.message.content)
                .unwrap_or_default();

            if text.is_empty() {
                warn!("OpenAI-compatible API returned empty content");
            }

            Ok(text)
        }
    }

    /// Stream a chat completion from an OpenAI-compatible API.
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
            debug!(model = %body["model"], "Sending streaming request to OpenAI-compatible API");

            let response = request
                .send()
                .await
                .context("failed to send streaming request to OpenAI-compatible API")?;

            let status = response.status();
            if !status.is_success() {
                let body_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "<failed to read response body>".to_string());

                if let Ok(err) = serde_json::from_str::<ErrorResponse>(&body_text) {
                    let err_type = err.error.error_type.as_deref().unwrap_or("unknown");
                    anyhow::bail!(
                        "OpenAI API error (HTTP {status}): [{err_type}] {msg}",
                        msg = err.error.message,
                    );
                }

                anyhow::bail!("OpenAI API error (HTTP {status}): {body_text}");
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
                                warn!("Non-UTF8 chunk from stream: {e}");
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

                        // Parse the JSON chunk
                        let chunk: StreamChunk = match serde_json::from_str(data) {
                            Ok(c) => c,
                            Err(_) => continue,
                        };

                        // Extract content from delta
                        if let Some(choice) = chunk.choices.into_iter().next() {
                            if let Some(content) = choice.delta.content {
                                if !content.is_empty() && tx.send(content).await.is_err() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = OpenAIConfig::default();
        assert_eq!(config.model, "gpt-4o");
        assert_eq!(config.base_url, "https://api.openai.com/v1");
        assert!(config.max_tokens.is_none());
        assert!(config.temperature.is_none());
        assert!(config.organization.is_none());
        assert_eq!(config.timeout, Duration::from_secs(120));
    }

    #[test]
    fn test_builder_pattern() {
        let backend = OpenAIBackend::new("test-key")
            .with_model("gpt-4o-mini")
            .with_temperature(0.7)
            .with_max_tokens(4096)
            .with_organization("org-test");

        assert_eq!(backend.config().model, "gpt-4o-mini");
        assert_eq!(backend.config().temperature, Some(0.7));
        assert_eq!(backend.config().max_tokens, Some(4096));
        assert_eq!(backend.config().organization.as_deref(), Some("org-test"));
    }

    #[test]
    fn test_temperature_clamping() {
        let backend = OpenAIBackend::new("key").with_temperature(5.0);
        assert_eq!(backend.config().temperature, Some(2.0));

        let backend = OpenAIBackend::new("key").with_temperature(-1.0);
        assert_eq!(backend.config().temperature, Some(0.0));

        let backend = OpenAIBackend::new("key").with_temperature(1.5);
        assert_eq!(backend.config().temperature, Some(1.5));
    }

    #[test]
    fn test_groq_constructor() {
        let backend = OpenAIBackend::groq("gsk_test");
        assert_eq!(backend.config().base_url, "https://api.groq.com/openai/v1");
        assert_eq!(backend.config().model, "llama-3.3-70b-versatile");
    }

    #[test]
    fn test_together_constructor() {
        let backend = OpenAIBackend::together("tok_test");
        assert_eq!(backend.config().base_url, "https://api.together.xyz/v1");
        assert!(backend.config().model.contains("llama"));
    }

    #[test]
    fn test_deepseek_constructor() {
        let backend = OpenAIBackend::deepseek("sk_test");
        assert_eq!(backend.config().base_url, "https://api.deepseek.com/v1");
        assert_eq!(backend.config().model, "deepseek-chat");
    }

    #[test]
    fn test_local_constructor() {
        let backend = OpenAIBackend::local("http://localhost:8080/v1");
        assert_eq!(backend.config().base_url, "http://localhost:8080/v1");
        assert!(backend.config().api_key.is_empty());
    }

    #[test]
    fn test_build_system_both() {
        let result = OpenAIBackend::build_system("You are helpful.", "Be creative.");
        assert_eq!(result, "You are helpful.\n\nBe creative.");
    }

    #[test]
    fn test_build_system_only_prompt() {
        let result = OpenAIBackend::build_system("You are helpful.", "");
        assert_eq!(result, "You are helpful.");
    }

    #[test]
    fn test_build_system_only_context() {
        let result = OpenAIBackend::build_system("", "Be creative.");
        assert_eq!(result, "Be creative.");
    }

    #[test]
    fn test_build_system_empty() {
        let result = OpenAIBackend::build_system("", "");
        assert_eq!(result, "");
    }

    #[test]
    fn test_request_body_basic() {
        let backend = OpenAIBackend::new("key");
        let body = backend.build_request_body("system text", "hello", false);

        assert_eq!(body["model"], "gpt-4o");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "system text");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"], "hello");
        assert!(body.get("stream").is_none());
        assert!(body.get("temperature").is_none());
        assert!(body.get("max_tokens").is_none());
    }

    #[test]
    fn test_request_body_with_options() {
        let backend = OpenAIBackend::new("key")
            .with_temperature(0.5)
            .with_max_tokens(2048);
        let body = backend.build_request_body("sys", "msg", true);

        assert_eq!(body["temperature"], 0.5);
        assert_eq!(body["max_tokens"], 2048);
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn test_request_body_no_system_when_empty() {
        let backend = OpenAIBackend::new("key");
        let body = backend.build_request_body("", "hello", false);

        // Should only have the user message, no system message
        assert_eq!(body["messages"].as_array().unwrap().len(), 1);
        assert_eq!(body["messages"][0]["role"], "user");
    }

    #[test]
    fn test_config_serialization() {
        let config = OpenAIConfig {
            api_key: "secret".to_string(),
            model: "gpt-4o-mini".to_string(),
            max_tokens: Some(2048),
            temperature: Some(0.8),
            organization: Some("org-test".to_string()),
            ..Default::default()
        };

        let json = serde_json::to_string(&config).unwrap();
        // API key should be skipped during serialization
        assert!(!json.contains("secret"));

        let parsed: OpenAIConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.model, "gpt-4o-mini");
        assert_eq!(parsed.max_tokens, Some(2048));
        assert_eq!(parsed.temperature, Some(0.8));
        assert_eq!(parsed.organization.as_deref(), Some("org-test"));
        assert!(parsed.api_key.is_empty());
    }

    #[test]
    fn test_from_env_missing() {
        std::env::remove_var("OPENAI_API_KEY");
        let result = OpenAIBackend::from_env();
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("OPENAI_API_KEY"));
    }
}
