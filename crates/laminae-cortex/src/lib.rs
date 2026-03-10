//! # laminae-cortex — Self-Improving Learning Loop for LLM Applications
//!
//! Tracks how users edit AI-generated output and converts those patterns
//! into reusable instructions that make future output better — without
//! fine-tuning.
//!
//! ## What It Does
//!
//! 1. **Track** — Record pairs of (AI output, user's edited version)
//! 2. **Detect** — Identify edit patterns: shortening, tone shifts,
//!    AI phrase removal, question removal, structural changes
//! 3. **Learn** — Use an LLM to convert edit diffs into natural-language
//!    instructions ("Never start with 'I think'", "Keep under 2 sentences")
//! 4. **Apply** — Get learned instructions as a prompt block for injection
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use laminae_cortex::{Cortex, CortexConfig};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let mut cortex = Cortex::new(CortexConfig::default());
//!
//!     // Track an edit: AI said X, user changed it to Y
//!     cortex.track_edit(
//!         "It's important to note that Rust is a systems language.",
//!         "Rust is a systems language.",
//!     );
//!
//!     // Detect patterns from tracked edits
//!     let patterns = cortex.detect_patterns();
//!     for p in &patterns {
//!         println!("{:?}: {:.0}% of edits", p.pattern_type, p.frequency_pct);
//!     }
//!
//!     // Get learned instructions as a prompt block
//!     let hints = cortex.get_prompt_block();
//!     println!("{hints}");
//!
//!     Ok(())
//! }
//! ```

mod detector;
#[cfg(not(target_arch = "wasm32"))]
mod learner;
mod store;
mod tracker;

pub use detector::{detect_patterns, EditPattern, PatternType};
#[cfg(not(target_arch = "wasm32"))]
pub use learner::EditLearner;
pub use store::{InstructionStore, LearnedInstruction};
pub use tracker::EditRecord;

/// Configuration for the Cortex learning loop.
#[derive(Debug, Clone)]
pub struct CortexConfig {
    /// Minimum number of edits before pattern detection activates (default: 5).
    pub min_edits_for_detection: usize,
    /// Minimum frequency (%) for a pattern to be reported (default: 20.0).
    pub min_pattern_frequency: f64,
    /// Maximum stored instructions (FIFO drop when exceeded, default: 50).
    pub max_instructions: usize,
    /// Word overlap threshold for instruction deduplication (default: 0.8).
    pub dedup_threshold: f64,
    /// Maximum instructions to include in prompt block (default: 8).
    pub max_prompt_instructions: usize,
}

impl Default for CortexConfig {
    fn default() -> Self {
        Self {
            min_edits_for_detection: 5,
            min_pattern_frequency: 20.0,
            max_instructions: 50,
            dedup_threshold: 0.8,
            max_prompt_instructions: 8,
        }
    }
}

/// The main Cortex engine — tracks edits, detects patterns, manages instructions.
pub struct Cortex {
    config: CortexConfig,
    edits: Vec<EditRecord>,
    instructions: InstructionStore,
}

impl Cortex {
    /// Create a new Cortex with the given configuration.
    pub fn new(config: CortexConfig) -> Self {
        let max_instructions = config.max_instructions;
        let dedup_threshold = config.dedup_threshold;
        Self {
            config,
            edits: Vec::new(),
            instructions: InstructionStore::new(max_instructions, dedup_threshold),
        }
    }

    /// Load existing instructions (e.g., from a previous session).
    pub fn with_instructions(mut self, instructions: Vec<LearnedInstruction>) -> Self {
        for inst in instructions {
            self.instructions.add(inst);
        }
        self
    }

    /// Load existing edit history.
    pub fn with_edits(mut self, edits: Vec<EditRecord>) -> Self {
        self.edits = edits;
        self
    }

    /// Track a user edit: the AI generated `original`, the user posted `edited`.
    pub fn track_edit(&mut self, original: &str, edited: &str) {
        self.edits.push(EditRecord::new(original, edited));
    }

    /// Detect patterns from all tracked edits.
    ///
    /// Only returns patterns that meet the minimum frequency threshold.
    pub fn detect_patterns(&self) -> Vec<EditPattern> {
        if self.edits.len() < self.config.min_edits_for_detection {
            return Vec::new();
        }

        let edited: Vec<&EditRecord> = self.edits.iter().filter(|e| e.was_edited).collect();
        if edited.is_empty() {
            return Vec::new();
        }

        detect_patterns(&edited, self.config.min_pattern_frequency)
    }

    /// Use an LLM to analyze a specific edit and generate an instruction.
    ///
    /// The instruction is automatically added to the store (deduplicated).
    /// Not available on WASM (requires Ollama).
    #[cfg(not(target_arch = "wasm32"))]
    pub async fn learn_from_edit(
        &mut self,
        original: &str,
        edited: &str,
        model: &str,
    ) -> anyhow::Result<Option<LearnedInstruction>> {
        let learner = EditLearner::new(model);
        let instruction = learner.analyze(original, edited).await?;

        if let Some(inst) = instruction {
            self.instructions.add(inst.clone());
            Ok(Some(inst))
        } else {
            Ok(None)
        }
    }

    /// Get learned instructions formatted as a prompt block.
    ///
    /// Returns a string ready to inject into an LLM system prompt.
    pub fn get_prompt_block(&self) -> String {
        let top = self.instructions.top(self.config.max_prompt_instructions);
        if top.is_empty() {
            return String::new();
        }

        let mut block = "--- USER PREFERENCES (learned from actual edits) ---\n".to_string();
        for inst in &top {
            block.push_str(&format!("- {}\n", inst.text));
        }
        block.push_str("---");
        block
    }

    /// Get all tracked edits.
    pub fn edits(&self) -> &[EditRecord] {
        &self.edits
    }

    /// Get all learned instructions.
    pub fn instructions(&self) -> &InstructionStore {
        &self.instructions
    }

    /// Export instructions for persistence.
    pub fn export_instructions(&self) -> Vec<LearnedInstruction> {
        self.instructions.all()
    }

    /// Get edit statistics.
    pub fn stats(&self) -> CortexStats {
        let total = self.edits.len();
        let edited = self.edits.iter().filter(|e| e.was_edited).count();
        CortexStats {
            total_edits: total,
            edited_count: edited,
            unedited_count: total - edited,
            edit_rate: if total > 0 {
                edited as f64 / total as f64
            } else {
                0.0
            },
            instruction_count: self.instructions.len(),
            patterns: self.detect_patterns(),
        }
    }
}

/// Summary statistics for the Cortex learning loop.
#[derive(Debug)]
pub struct CortexStats {
    /// Total edit records tracked.
    pub total_edits: usize,
    /// Records where the user modified the AI output.
    pub edited_count: usize,
    /// Records where the user accepted the AI output as-is.
    pub unedited_count: usize,
    /// Fraction of outputs that were edited (0.0-1.0).
    pub edit_rate: f64,
    /// Number of learned instructions in the store.
    pub instruction_count: usize,
    /// Detected edit patterns.
    pub patterns: Vec<EditPattern>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cortex_basic_flow() {
        let mut cortex = Cortex::new(CortexConfig {
            min_edits_for_detection: 2,
            min_pattern_frequency: 10.0,
            ..Default::default()
        });

        // Track several edits where user shortens AI output
        for _ in 0..5 {
            cortex.track_edit(
                "It's important to note that this is a very long sentence with many unnecessary words that could be shortened significantly.",
                "This could be shorter.",
            );
        }

        let patterns = cortex.detect_patterns();
        assert!(!patterns.is_empty());
        assert!(patterns
            .iter()
            .any(|p| p.pattern_type == PatternType::Shortened));
    }

    #[test]
    fn test_cortex_below_threshold() {
        let cortex = Cortex::new(CortexConfig::default());
        // No edits tracked
        let patterns = cortex.detect_patterns();
        assert!(patterns.is_empty());
    }

    #[test]
    fn test_cortex_stats() {
        let mut cortex = Cortex::new(CortexConfig {
            min_edits_for_detection: 1,
            ..Default::default()
        });

        cortex.track_edit("AI output", "User edited version");
        cortex.track_edit("Another AI output", "Another AI output"); // not edited

        let stats = cortex.stats();
        assert_eq!(stats.total_edits, 2);
        assert_eq!(stats.edited_count, 1);
        assert_eq!(stats.unedited_count, 1);
        assert!((stats.edit_rate - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_prompt_block_empty() {
        let cortex = Cortex::new(CortexConfig::default());
        assert!(cortex.get_prompt_block().is_empty());
    }

    #[test]
    fn test_prompt_block_with_instructions() {
        let mut cortex = Cortex::new(CortexConfig::default());
        cortex.instructions.add(LearnedInstruction {
            text: "Never start with I think".into(),
            source_count: 3,
            added: chrono::Utc::now(),
        });

        let block = cortex.get_prompt_block();
        assert!(block.contains("Never start with I think"));
        assert!(block.contains("USER PREFERENCES"));
    }
}
