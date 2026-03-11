//! # Laminae — The Missing Layer Between Raw LLMs and Production AI
//!
//! Laminae is a modular SDK that adds personality, voice, safety, learning,
//! and containment to any AI application. Each layer works independently or
//! together as a full stack.
//!
//! ## The Layers
//!
//! | Layer | Crate | What It Does |
//! |-------|-------|-------------|
//! | **Psyche** | [`laminae-psyche`] | Multi-agent cognitive pipeline (Id + Superego → Ego) |
//! | **Persona** | [`laminae-persona`] | Voice extraction and style enforcement |
//! | **Cortex** | [`laminae-cortex`] | Self-improving learning loop from user edits |
//! | **Shadow** | [`laminae-shadow`] | Adversarial red-teaming of AI output |
//! | **Ironclad** | [`laminae-ironclad`] | Process-level execution sandbox |
//! | **Glassbox** | [`laminae-glassbox`] | Input/output containment layer |
//!
//! Plus [`laminae-ollama`] for local LLM inference via Ollama.
//!
//! ## Quick Start
//!
//! ```toml
//! [dependencies]
//! laminae = "0.4"
//! ```
//!
//! Use individual crates for fine-grained control, or this meta-crate
//! for the full stack.

// ── Layers available on ALL platforms (including WASM) ──

/// Voice persona extraction and style enforcement — learns how a person
/// writes and keeps LLM output on-voice.
pub use laminae_persona as persona;

/// Self-improving learning loop — tracks user edits, detects patterns,
/// converts corrections into reusable instructions.
pub use laminae_cortex as cortex;

/// Input/output containment — rate limiting, command blocklists,
/// immutable zones, injection prevention.
pub use laminae_glassbox as glassbox;

// ── Layers that require native OS features (not available in WASM) ──

/// Multi-agent cognitive pipeline — personality and safety through
/// Id (creative), Superego (safety), and Ego (your LLM).
#[cfg(not(target_arch = "wasm32"))]
pub use laminae_psyche as psyche;

/// Adversarial red-teaming engine — automated security auditing
/// of AI output via static analysis, LLM review, and sandbox execution.
#[cfg(not(target_arch = "wasm32"))]
pub use laminae_shadow as shadow;

/// Process-level execution sandbox — command whitelist, network filter,
/// resource watchdog with SIGKILL.
#[cfg(not(target_arch = "wasm32"))]
pub use laminae_ironclad as ironclad;

/// Ollama client for local LLM inference.
#[cfg(not(target_arch = "wasm32"))]
pub use laminae_ollama as ollama;

/// Anthropic Claude backend — first-class EgoBackend for Claude models.
#[cfg(all(not(target_arch = "wasm32"), feature = "anthropic"))]
pub use laminae_anthropic as anthropic;

/// OpenAI-compatible backend — EgoBackend for OpenAI, Groq, Together, DeepSeek, and local servers.
#[cfg(all(not(target_arch = "wasm32"), feature = "openai"))]
pub use laminae_openai as openai;
