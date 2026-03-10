# laminae-cortex

**Self-improving learning loop for LLM applications.**

Part of the [Laminae](https://github.com/Orellius/Laminae) SDK — the missing layer between raw LLMs and production AI.

## What It Does

Tracks how users edit AI-generated output and converts those corrections into reusable instructions — without fine-tuning. The AI gets better with every edit.

1. **Track** — Record pairs of (AI output, user's edited version)
2. **Detect** — Identify edit patterns: shortening, tone shifts, AI phrase removal, question removal
3. **Learn** — Use an LLM to convert edit diffs into natural-language instructions
4. **Apply** — Get learned instructions as a prompt block for injection into any LLM

## Installation

```toml
[dependencies]
laminae-cortex = "0.1"
tokio = { version = "1", features = ["full"] }
```

## Usage

### Track edits and detect patterns

```rust
use laminae_cortex::{Cortex, CortexConfig};

let mut cortex = Cortex::new(CortexConfig {
    min_edits_for_detection: 3,
    ..Default::default()
});

// Track what the AI generated vs what the user actually posted
cortex.track_edit(
    "It's worth noting that Rust is fast.",
    "Rust is fast.",
);
cortex.track_edit(
    "Furthermore, the type system is robust and comprehensive.",
    "The type system catches bugs at compile time.",
);
cortex.track_edit(
    "Moving forward, we should consider using Rust. What do you think?",
    "We should use Rust.",
);

// Detect recurring patterns
let patterns = cortex.detect_patterns();
for p in &patterns {
    println!("{:?}: {:.0}% of edits ({} occurrences)",
        p.pattern_type, p.frequency_pct, p.count);
}
// → Shortened: 100% (3 occurrences)
// → RemovedAiPhrases: 100% (3 occurrences)
// → RemovedQuestion: 33% (1 occurrence)
```

### Get prompt block for injection

```rust
let hints = cortex.get_prompt_block();
println!("{hints}");
// --- USER PREFERENCES (learned from actual edits) ---
// - Never use academic hedging phrases
// - Keep sentences short and direct
// - Don't end with questions
// ---
```

### LLM-powered instruction learning

```rust,no_run
// Use a local LLM to analyze specific edits
let instruction = cortex.learn_from_edit(
    "It's important to note that this approach has several advantages.",
    "This approach works better.",
    "qwen2.5:3b",
).await?;
// → Some("Remove hedging phrases, be direct")
```

### Statistics

```rust
let stats = cortex.stats();
println!("Edit rate: {:.0}%", stats.edit_rate * 100.0);
println!("Instructions learned: {}", stats.instruction_count);
```

## Detected Pattern Types

| Pattern | Trigger | Example |
|---------|---------|---------|
| `Shortened` | User removes 20%+ of text | "Long verbose sentence..." → "Short." |
| `RemovedQuestion` | Trailing question deleted | "Good point. Thoughts?" → "Good point." |
| `RemovedOpener` | First sentence rewritten (<30% overlap) | "The article shows..." → "This matters because..." |
| `RemovedAiPhrases` | AI-sounding phrases stripped | "It's worth noting that X" → "X" |
| `AddedContent` | User expands 30%+ beyond AI output | "Short." → "Short, but here's why..." |
| `SimplifiedLanguage` | Complex words replaced | "unprecedented" → "new" |
| `ChangedToneSofter` | Gentler language substituted | "You must..." → "You could..." |
| `ChangedToneStronger` | Bolder language substituted | "Perhaps consider..." → "Always do..." |

## Configuration

```rust
use laminae_cortex::CortexConfig;

let config = CortexConfig {
    min_edits_for_detection: 5,   // Wait for 5 edits before detecting patterns
    min_pattern_frequency: 20.0,  // Only report patterns in 20%+ of edits
    max_instructions: 50,         // Keep at most 50 instructions (FIFO)
    dedup_threshold: 0.8,         // 80% word overlap = duplicate
    max_prompt_instructions: 8,   // Include top 8 in prompt block
};
```

## License

Apache-2.0 — Copyright 2026 Orel Ohayon.
