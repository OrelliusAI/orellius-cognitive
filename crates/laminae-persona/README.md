# laminae-persona

**Voice persona extraction and style enforcement for LLM output.**

Part of the [Laminae](https://github.com/Orellius/Laminae) SDK — the missing layer between raw LLMs and production AI.

## What It Does

Extracts a writing personality from text samples and enforces it on LLM output. Platform-agnostic — works for emails, docs, chat, code reviews, support tickets.

1. **Extract** — Feed it 20-100 text samples → get a structured persona (tone, humor, vocabulary, formality, perspective, emotional style, narrative preference)
2. **Compile** — Turn a persona into a compact prompt block for any LLM
3. **Filter** — Post-generation voice filter catches AI-sounding output
4. **Track** — Voice DNA tracks distinctive phrases confirmed by reuse

## Installation

```toml
[dependencies]
laminae-persona = "0.1"
tokio = { version = "1", features = ["full"] }
```

## Usage

### Extract a persona from text samples

```rust,no_run
use laminae_persona::{PersonaExtractor, WeightedSample};

let extractor = PersonaExtractor::new("qwen2.5:7b");

let samples = vec![
    WeightedSample::from("Ship it. Fix it later. Perfection is the enemy of done."),
    WeightedSample::from("Nobody reads your README. Write code that doesn't need one."),
    WeightedSample::with_weight("Hot take: most abstractions are just job security.", 3.0),
];

let persona = extractor.extract(&samples).await?;
println!("Tone: {:?}", persona.voice.tone_words);
println!("Style: {}", persona.voice.writing_style);
```

### Compile to a prompt block

```rust,no_run
use laminae_persona::compile_persona;

let prompt_block = compile_persona(&persona);
// Inject into your LLM's system prompt:
let system = format!("{prompt_block}\n\nYou are a helpful assistant.");
```

### Post-generation voice filter

```rust
use laminae_persona::{VoiceFilter, VoiceFilterConfig};

let filter = VoiceFilter::new(VoiceFilterConfig::default());

let result = filter.check("It's important to note that shipping fast is crucial.");
assert!(!result.passed);
// result.violations: ["AI vocabulary detected: it's important to note"]
// result.retry_hints: ["DO NOT use formal/academic language..."]
// result.cleaned: auto-fixed version of the text

let result = filter.check("Ship fast, break things.");
assert!(result.passed); // Clean human-sounding text
```

### Voice DNA tracking

```rust
use laminae_persona::VoiceDna;

let mut dna = VoiceDna::empty();
dna.record_success("Nobody reads your README. Ship it or delete it.");
dna.record_success("Nobody reads docs. Just write better code.");
// "Nobody reads" is now a confirmed DNA phrase
```

## Voice Filter Layers

The filter runs 6 detection layers in sequence:

| Layer | Detects | Auto-fixes |
|-------|---------|-----------|
| AI Vocabulary | 60+ academic/formal phrases (furthermore, multifaceted, etc.) | Flags for retry |
| Meta-commentary | "The post highlights...", "This article shows..." | Strips opener |
| Multi-paragraph | Unnecessary `\n\n` structure | Joins paragraphs |
| Trailing Questions | "What do you think?", "How will this play out?" | Strips question |
| Em-dashes | Overuse of `—` (LLM signature) | Replaces with periods |
| Length | Exceeds sentence/character limits | Truncates |

## The 7 Voice Dimensions

| Dimension | Example |
|-----------|---------|
| **Tone words** | sharp, witty, blunt, assertive |
| **Writing style** | "Short punchy sentences with no padding" |
| **Humor style** | "Dry sarcasm, never forced" |
| **Emotional range** | "Controlled intensity — calm until provoked" |
| **Sentence length** | VeryShort, Short, Medium, Long |
| **Punctuation** | "Heavy periods, rare commas, occasional dashes" |
| **Voice summary** | "You write like a telegram operator with opinions..." |

## License

Apache-2.0 — Copyright 2026 Orel Ohayon.
