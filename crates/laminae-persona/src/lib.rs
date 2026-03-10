//! # laminae-persona — Voice Persona Extraction & Style Enforcement
//!
//! Extracts a writing personality from text samples and enforces it on LLM output.
//! Platform-agnostic — works for any text: tweets, emails, docs, code reviews, chat.
//!
//! ## What It Does
//!
//! 1. **Extract** — Feed it 20-100 text samples → get a structured persona
//!    (tone, humor, vocabulary, formality, perspective, emotional style, narrative preference)
//! 2. **Compile** — Turn a persona into a compact prompt block for any LLM
//! 3. **Filter** — Post-generation voice filter catches AI-sounding output
//! 4. **Track** — Voice DNA tracks distinctive phrases confirmed by reuse
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use laminae_persona::{PersonaExtractor, VoiceFilter, VoiceFilterConfig, compile_persona};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let extractor = PersonaExtractor::new("qwen2.5:7b");
//!
//!     let samples = vec![
//!         "Ship it. Fix it later. Perfection is the enemy of done.".into(),
//!         "Nobody reads your README. Write code that doesn't need one.".into(),
//!         "Hot take: most abstractions are just job security.".into(),
//!     ];
//!
//!     let persona = extractor.extract(&samples).await?;
//!     println!("Tone: {:?}", persona.voice.tone_words);
//!     println!("Prompt:\n{}", compile_persona(&persona));
//!
//!     let filter = VoiceFilter::new(VoiceFilterConfig::default());
//!     let result = filter.check("It's important to note that shipping fast is crucial.");
//!     println!("Passed: {}, Violations: {:?}", result.passed, result.violations);
//!
//!     Ok(())
//! }
//! ```

mod dna;
#[cfg(not(target_arch = "wasm32"))]
mod extractor;
mod filter;
mod model;
mod prompt;

pub use dna::VoiceDna;
#[cfg(not(target_arch = "wasm32"))]
pub use extractor::PersonaExtractor;
pub use filter::{VoiceCheckResult, VoiceFilter, VoiceFilterConfig};
pub use model::*;
pub use prompt::compile_persona;
