//! # laminae-psyche — Multi-Agent Cognitive Pipeline
//!
//! A Freudian-inspired architecture where AI responses are shaped by
//! three agents working in concert:
//!
//! - **Id** — The creative/instinctual agent. Generates raw ideas, emotional
//!   undertones, and unconventional angles. Runs via a local LLM (Ollama).
//!
//! - **Superego** — The safety/ethics agent. Evaluates requests for risks,
//!   boundaries, and appropriateness. Runs via a local LLM (Ollama).
//!
//! - **Ego** — The executor. Takes the user's message, enriched with invisible
//!   context from Id and Superego, and produces the final response. This is
//!   YOUR LLM — bring any backend (Claude, GPT, local, custom).
//!
//! ## The Key Insight
//!
//! Id and Superego run on small, fast, local models (zero cost). Their output
//! is compressed into "context signals" that are injected into the Ego's prompt
//! as invisible system context. The Ego never sees raw Id/Superego output —
//! it receives distilled creative angles and safety boundaries that shape its
//! response without the user knowing.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use laminae_psyche::{PsycheEngine, EgoBackend};
//! use laminae_ollama::OllamaClient;
//!
//! // Implement EgoBackend for your LLM
//! struct MyEgo;
//!
//! impl EgoBackend for MyEgo {
//!     fn complete(&self, _system: &str, _user_msg: &str, _context: &str)
//!         -> impl std::future::Future<Output = anyhow::Result<String>> + Send
//!     {
//!         async { Ok("Hello!".to_string()) }
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let ollama = OllamaClient::new();
//!     let ego = MyEgo;
//!     let engine = PsycheEngine::new(ollama, ego);
//!
//!     let response = engine.reply("What is creativity?").await?;
//!     println!("{response}");
//!     Ok(())
//! }
//! ```

pub mod prompts;

use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::mpsc;

use laminae_ollama::OllamaClient;
use prompts::{
    classify_tier, ego_context, ego_context_cop, id_prompt, id_prompt_cop, should_skip_psyche,
    superego_prompt, superego_prompt_cop, ResponseTier,
};

// ── Typed Errors ──

/// Typed errors for the Psyche engine.
#[derive(Debug, Error)]
pub enum PsycheError {
    /// An LLM backend (Ollama or Ego) returned an error.
    #[error("backend error: {0}")]
    BackendError(#[from] anyhow::Error),

    /// Configuration is invalid or missing required values.
    #[error("config error: {0}")]
    ConfigError(String),

    /// Superego blocked the request for safety reasons.
    #[error("blocked: {0}")]
    Blocked(String),

    /// An operation timed out waiting for a response.
    #[error("timeout")]
    Timeout,
}

// ── Traits — Bring Your Own Backend ──

/// Trait for the Ego executor. Implement this to plug in any LLM.
///
/// The Ego receives the user's message along with invisible context
/// from Id and Superego. It produces the final user-facing response.
pub trait EgoBackend: Send + Sync {
    /// Generate a response given system prompt, user message, and Psyche context.
    ///
    /// The `context` parameter contains distilled Id/Superego signals that should
    /// be prepended to the system prompt (invisible to the user).
    fn complete(
        &self,
        system_prompt: &str,
        user_message: &str,
        psyche_context: &str,
    ) -> impl std::future::Future<Output = Result<String>> + Send;

    /// Streaming variant. Default implementation falls back to non-streaming.
    fn complete_streaming(
        &self,
        system_prompt: &str,
        user_message: &str,
        psyche_context: &str,
    ) -> impl std::future::Future<Output = Result<mpsc::Receiver<String>>> + Send {
        async {
            let result = self
                .complete(system_prompt, user_message, psyche_context)
                .await?;
            let (tx, rx) = mpsc::channel(1);
            let _ = tx.send(result).await;
            Ok(rx)
        }
    }
}

// ── Events ──

/// Events emitted during Psyche processing, for UI telemetry.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum PsycheEvent {
    /// Processing phase changed.
    PhaseChange { phase: Phase },
    /// Id agent produced output (streaming).
    IdChunk { text: String },
    /// Id agent finished.
    IdDone { full_text: String },
    /// Superego agent produced output (streaming).
    SuperegoChunk { text: String },
    /// Superego agent finished.
    SuperegoDone { full_text: String },
    /// Ego is generating (streaming).
    EgoChunk { text: String },
}

/// Processing phases for UI display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Thinking,
    EnsuringSafety,
    Responding,
}

// ── Configuration ──

/// Tuning parameters for the Psyche engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct PsycheConfig {
    /// Model name for the Id agent (creative force).
    #[serde(default = "default_id_model")]
    pub id_model: String,

    /// Model name for the Superego agent (safety evaluator).
    #[serde(default = "default_superego_model")]
    pub superego_model: String,

    /// Temperature for Id (higher = more creative). Range: 0.0–2.0
    #[serde(default = "default_id_temp")]
    pub id_temperature: f32,

    /// Temperature for Superego (lower = more strict). Range: 0.0–2.0
    #[serde(default = "default_superego_temp")]
    pub superego_temperature: f32,

    /// Max tokens for Id response.
    #[serde(default = "default_id_tokens")]
    pub id_max_tokens: i32,

    /// Max tokens for Superego response.
    #[serde(default = "default_superego_tokens")]
    pub superego_max_tokens: i32,

    /// Weight of Id influence on Ego (0.0 = ignored, 1.0 = dominant).
    #[serde(default = "default_id_weight")]
    pub id_weight: f32,

    /// Weight of Superego influence on Ego (0.0 = ignored, 1.0 = dominant).
    #[serde(default = "default_superego_weight")]
    pub superego_weight: f32,

    /// Max tokens for COP (Compressed Output Protocol) mode.
    #[serde(default = "default_cop_tokens")]
    pub cop_max_tokens: i32,

    /// Timeout for COP mode in seconds.
    #[serde(default = "default_cop_timeout")]
    pub cop_timeout_secs: u64,

    /// System prompt for the Ego (provided by the SDK user).
    #[serde(default)]
    pub ego_system_prompt: String,
}

fn default_id_model() -> String {
    "qwen2.5:7b".to_string()
}
fn default_superego_model() -> String {
    "qwen2.5:7b".to_string()
}
fn default_id_temp() -> f32 {
    0.9
}
fn default_superego_temp() -> f32 {
    0.3
}
fn default_id_tokens() -> i32 {
    512
}
fn default_superego_tokens() -> i32 {
    256
}
fn default_id_weight() -> f32 {
    0.6
}
fn default_superego_weight() -> f32 {
    0.4
}
fn default_cop_tokens() -> i32 {
    80
}
fn default_cop_timeout() -> u64 {
    15
}

impl Default for PsycheConfig {
    fn default() -> Self {
        Self {
            id_model: default_id_model(),
            superego_model: default_superego_model(),
            id_temperature: default_id_temp(),
            superego_temperature: default_superego_temp(),
            id_max_tokens: default_id_tokens(),
            superego_max_tokens: default_superego_tokens(),
            id_weight: default_id_weight(),
            superego_weight: default_superego_weight(),
            cop_max_tokens: default_cop_tokens(),
            cop_timeout_secs: default_cop_timeout(),
            ego_system_prompt: String::new(),
        }
    }
}

impl PsycheConfig {
    /// Clamp all values to sane ranges.
    pub fn clamp(&mut self) {
        self.id_temperature = self.id_temperature.clamp(0.0, 2.0);
        self.superego_temperature = self.superego_temperature.clamp(0.0, 2.0);
        self.id_weight = self.id_weight.clamp(0.0, 1.0);
        self.superego_weight = self.superego_weight.clamp(0.0, 1.0);
        self.id_max_tokens = self.id_max_tokens.clamp(50, 4096);
        self.superego_max_tokens = self.superego_max_tokens.clamp(50, 4096);
        self.cop_max_tokens = self.cop_max_tokens.clamp(20, 500);
        self.cop_timeout_secs = self.cop_timeout_secs.clamp(3, 60);
    }

    /// Load from a JSON file, falling back to defaults.
    pub fn load_from(path: &std::path::Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let mut config: Self = serde_json::from_str(&content).unwrap_or_default();
                config.clamp();
                config
            }
            Err(_) => Self::default(),
        }
    }

    /// Set the Id agent model name.
    pub fn with_id_model(mut self, model: impl Into<String>) -> Self {
        self.id_model = model.into();
        self
    }

    /// Set the Superego agent model name.
    pub fn with_superego_model(mut self, model: impl Into<String>) -> Self {
        self.superego_model = model.into();
        self
    }

    /// Set the Id agent temperature (clamped to 0.0 - 2.0).
    pub fn with_id_temperature(mut self, temperature: f32) -> Self {
        self.id_temperature = temperature.clamp(0.0, 2.0);
        self
    }

    /// Set the Ego system prompt.
    pub fn with_ego_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.ego_system_prompt = prompt.into();
        self
    }

    /// Generate a human-readable weight instruction for the Ego.
    pub fn weight_instruction(&self) -> String {
        let id_desc = match self.id_weight {
            w if w < 0.2 => "minimal",
            w if w < 0.5 => "moderate",
            w if w < 0.8 => "significant",
            _ => "dominant",
        };
        let superego_desc = match self.superego_weight {
            w if w < 0.2 => "minimal",
            w if w < 0.5 => "moderate",
            w if w < 0.8 => "significant",
            _ => "dominant",
        };
        format!(
            "Creative influence: {} ({:.0}%). Safety influence: {} ({:.0}%).",
            id_desc,
            self.id_weight * 100.0,
            superego_desc,
            self.superego_weight * 100.0,
        )
    }
}

// ── Readiness Check ──

/// Result of checking if Psyche can run.
#[derive(Debug, Clone, Serialize)]
pub struct PsycheReadiness {
    pub ollama_available: bool,
    pub id_model_available: bool,
    pub superego_model_available: bool,
    pub ready: bool,
}

// ── Core Engine ──

/// The Psyche Engine — multi-agent cognitive pipeline.
///
/// Orchestrates Id, Superego, and Ego to produce shaped AI responses.
pub struct PsycheEngine<E: EgoBackend> {
    ollama: OllamaClient,
    ego: Arc<E>,
    config: PsycheConfig,
    extra_context: Option<String>,
}

impl<E: EgoBackend + 'static> PsycheEngine<E> {
    /// Create a new PsycheEngine with default configuration.
    pub fn new(ollama: OllamaClient, ego: E) -> Self {
        Self {
            ollama,
            ego: Arc::new(ego),
            config: PsycheConfig::default(),
            extra_context: None,
        }
    }

    /// Create with explicit configuration.
    pub fn with_config(ollama: OllamaClient, ego: E, config: PsycheConfig) -> Self {
        Self {
            ollama,
            ego: Arc::new(ego),
            config,
            extra_context: None,
        }
    }

    /// Set additional context to inject into Ego prompts.
    pub fn set_extra_context(&mut self, context: Option<String>) {
        self.extra_context = context;
    }

    /// Update configuration.
    pub fn set_config(&mut self, config: PsycheConfig) {
        self.config = config;
    }

    pub fn config(&self) -> &PsycheConfig {
        &self.config
    }

    /// Check if Ollama and required models are available.
    pub async fn check_readiness(&self) -> PsycheReadiness {
        let ollama_available = self.ollama.is_available().await;
        let (id_model, superego_model) = if ollama_available {
            tokio::join!(
                self.ollama.has_model(&self.config.id_model),
                self.ollama.has_model(&self.config.superego_model),
            )
        } else {
            (false, false)
        };

        PsycheReadiness {
            ollama_available,
            id_model_available: id_model,
            superego_model_available: superego_model,
            ready: ollama_available && id_model && superego_model,
        }
    }

    /// Generate a response using the full Psyche pipeline.
    ///
    /// Automatically classifies the request into a response tier:
    /// - **Skip**: Simple messages bypass Psyche entirely (direct to Ego).
    /// - **Light**: Uses COP (Compressed Output Protocol) for fast Id/Superego.
    /// - **Full**: Full Id + Superego pipeline with complete prose output.
    pub async fn reply(&self, user_message: &str) -> Result<String> {
        // Tier classification
        if should_skip_psyche(user_message) {
            return self.ego_direct(user_message).await;
        }

        let tier = classify_tier(user_message);

        match tier {
            ResponseTier::Skip => self.ego_direct(user_message).await,
            ResponseTier::Light => self.reply_cop(user_message).await,
            ResponseTier::Full => self.reply_full(user_message).await,
        }
    }

    /// Direct Ego call (no Id/Superego).
    async fn ego_direct(&self, user_message: &str) -> Result<String> {
        let system = &self.config.ego_system_prompt;
        let extra = self.extra_context.as_deref().unwrap_or("");
        self.ego.complete(system, user_message, extra).await
    }

    /// COP mode — compressed Id/Superego signals with timeout.
    async fn reply_cop(&self, user_message: &str) -> Result<String> {
        let timeout = std::time::Duration::from_secs(self.config.cop_timeout_secs);

        let (id_result, superego_result) = tokio::join!(
            tokio::time::timeout(timeout, self.run_id_cop(user_message)),
            tokio::time::timeout(timeout, self.run_superego_cop(user_message)),
        );

        let id_output = id_result.ok().and_then(|r| r.ok()).unwrap_or_default();
        let superego_output = superego_result
            .ok()
            .and_then(|r| r.ok())
            .unwrap_or_default();

        // Check for BLOCK verdict from Superego
        if let Some(block_reason) = prompts::extract_block_reason(&superego_output) {
            return Err(PsycheError::Blocked(block_reason).into());
        }

        let context = ego_context_cop(&id_output, &superego_output, &self.config);
        let full_context = match &self.extra_context {
            Some(extra) => format!("{context}\n\n{extra}"),
            None => context,
        };

        self.ego
            .complete(&self.config.ego_system_prompt, user_message, &full_context)
            .await
    }

    /// Full pipeline — complete Id + Superego prose.
    async fn reply_full(&self, user_message: &str) -> Result<String> {
        let (id_result, superego_result) =
            tokio::join!(self.run_id(user_message), self.run_superego(user_message),);

        let id_output = id_result.unwrap_or_default();
        let superego_output = superego_result.unwrap_or_default();

        if let Some(block_reason) = prompts::extract_block_reason(&superego_output) {
            return Err(PsycheError::Blocked(block_reason).into());
        }

        let context = ego_context(&id_output, &superego_output, &self.config);
        let full_context = match &self.extra_context {
            Some(extra) => format!("{context}\n\n{extra}"),
            None => context,
        };

        self.ego
            .complete(&self.config.ego_system_prompt, user_message, &full_context)
            .await
    }

    /// Reply with streaming events for UI telemetry.
    pub async fn reply_streaming(&self, user_message: &str) -> Result<mpsc::Receiver<PsycheEvent>> {
        let (tx, rx) = mpsc::channel::<PsycheEvent>(64);

        if should_skip_psyche(user_message) {
            let _ = tx
                .send(PsycheEvent::PhaseChange {
                    phase: Phase::Responding,
                })
                .await;
            let ego_rx = self
                .ego
                .complete_streaming(
                    &self.config.ego_system_prompt,
                    user_message,
                    self.extra_context.as_deref().unwrap_or(""),
                )
                .await?;
            Self::forward_ego_chunks(tx, ego_rx);
            return Ok(rx);
        }

        let tier = classify_tier(user_message);
        let user_msg = user_message.to_string();
        let ollama = self.ollama.clone();
        let config = self.config.clone();
        let ego = Arc::clone(&self.ego);
        let extra = self.extra_context.clone();

        tokio::spawn(async move {
            let _ = tx
                .send(PsycheEvent::PhaseChange {
                    phase: Phase::Thinking,
                })
                .await;

            let (id_output, superego_output) = match tier {
                ResponseTier::Skip => (String::new(), String::new()),
                ResponseTier::Light => {
                    let timeout = std::time::Duration::from_secs(config.cop_timeout_secs);
                    let id_fut = ollama.complete(
                        &config.id_model,
                        id_prompt_cop(),
                        &user_msg,
                        config.id_temperature,
                        config.cop_max_tokens,
                    );
                    let superego_fut = ollama.complete(
                        &config.superego_model,
                        superego_prompt_cop(),
                        &user_msg,
                        config.superego_temperature,
                        config.cop_max_tokens,
                    );
                    let (id_r, se_r) = tokio::join!(
                        tokio::time::timeout(timeout, id_fut),
                        tokio::time::timeout(timeout, superego_fut),
                    );
                    (
                        id_r.ok().and_then(|r| r.ok()).unwrap_or_default(),
                        se_r.ok().and_then(|r| r.ok()).unwrap_or_default(),
                    )
                }
                ResponseTier::Full => {
                    let id_fut = ollama.complete(
                        &config.id_model,
                        id_prompt(),
                        &user_msg,
                        config.id_temperature,
                        config.id_max_tokens,
                    );
                    let superego_fut = ollama.complete(
                        &config.superego_model,
                        superego_prompt(),
                        &user_msg,
                        config.superego_temperature,
                        config.superego_max_tokens,
                    );
                    let (id_r, se_r) = tokio::join!(id_fut, superego_fut);
                    (id_r.unwrap_or_default(), se_r.unwrap_or_default())
                }
            };

            let _ = tx
                .send(PsycheEvent::IdDone {
                    full_text: id_output.clone(),
                })
                .await;

            let _ = tx
                .send(PsycheEvent::PhaseChange {
                    phase: Phase::EnsuringSafety,
                })
                .await;
            let _ = tx
                .send(PsycheEvent::SuperegoDone {
                    full_text: superego_output.clone(),
                })
                .await;

            if let Some(block_reason) = prompts::extract_block_reason(&superego_output) {
                let _ = tx.send(PsycheEvent::EgoChunk { text: block_reason }).await;
                return;
            }

            let context = match tier {
                ResponseTier::Light | ResponseTier::Skip => {
                    ego_context_cop(&id_output, &superego_output, &config)
                }
                ResponseTier::Full => ego_context(&id_output, &superego_output, &config),
            };

            let full_context = match &extra {
                Some(e) => format!("{context}\n\n{e}"),
                None => context,
            };

            let _ = tx
                .send(PsycheEvent::PhaseChange {
                    phase: Phase::Responding,
                })
                .await;

            match ego
                .complete_streaming(&config.ego_system_prompt, &user_msg, &full_context)
                .await
            {
                Ok(mut ego_rx) => {
                    while let Some(chunk) = ego_rx.recv().await {
                        if tx
                            .send(PsycheEvent::EgoChunk { text: chunk })
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Ego streaming error: {e}");
                    let _ = tx
                        .send(PsycheEvent::EgoChunk {
                            text: format!("Error: {e}"),
                        })
                        .await;
                }
            }
        });

        Ok(rx)
    }

    // ── Internal Helpers ──

    async fn run_id(&self, user_message: &str) -> Result<String> {
        self.ollama
            .complete(
                &self.config.id_model,
                id_prompt(),
                user_message,
                self.config.id_temperature,
                self.config.id_max_tokens,
            )
            .await
    }

    async fn run_superego(&self, user_message: &str) -> Result<String> {
        self.ollama
            .complete(
                &self.config.superego_model,
                superego_prompt(),
                user_message,
                self.config.superego_temperature,
                self.config.superego_max_tokens,
            )
            .await
    }

    async fn run_id_cop(&self, user_message: &str) -> Result<String> {
        self.ollama
            .complete(
                &self.config.id_model,
                id_prompt_cop(),
                user_message,
                self.config.id_temperature,
                self.config.cop_max_tokens,
            )
            .await
    }

    async fn run_superego_cop(&self, user_message: &str) -> Result<String> {
        self.ollama
            .complete(
                &self.config.superego_model,
                superego_prompt_cop(),
                user_message,
                self.config.superego_temperature,
                self.config.cop_max_tokens,
            )
            .await
    }

    fn forward_ego_chunks(tx: mpsc::Sender<PsycheEvent>, mut ego_rx: mpsc::Receiver<String>) {
        tokio::spawn(async move {
            while let Some(chunk) = ego_rx.recv().await {
                if tx
                    .send(PsycheEvent::EgoChunk { text: chunk })
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockEgo;

    impl EgoBackend for MockEgo {
        fn complete(
            &self,
            _system: &str,
            user_msg: &str,
            _ctx: &str,
        ) -> impl std::future::Future<Output = Result<String>> + Send {
            let msg = format!("Echo: {user_msg}");
            async move { Ok(msg) }
        }
    }

    #[test]
    fn test_config_defaults() {
        let config = PsycheConfig::default();
        assert_eq!(config.id_weight, 0.6);
        assert_eq!(config.superego_weight, 0.4);
    }

    #[test]
    fn test_config_clamp() {
        let mut config = PsycheConfig {
            id_temperature: 5.0,
            superego_weight: -1.0,
            ..Default::default()
        };
        config.clamp();
        assert_eq!(config.id_temperature, 2.0);
        assert_eq!(config.superego_weight, 0.0);
    }

    #[test]
    fn test_weight_instruction() {
        let config = PsycheConfig::default();
        let instruction = config.weight_instruction();
        assert!(instruction.contains("Creative"));
        assert!(instruction.contains("Safety"));
    }

    #[tokio::test]
    async fn test_skip_psyche_for_greetings() {
        let ollama = OllamaClient::new();
        let engine = PsycheEngine::new(ollama, MockEgo);
        // "hello" should skip Psyche and go directly to Ego
        let result = engine.reply("hello").await.unwrap();
        assert!(result.contains("Echo: hello"));
    }

    #[test]
    fn test_serde_roundtrip() {
        let config = PsycheConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let parsed: PsycheConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id_weight, config.id_weight);
    }
}
