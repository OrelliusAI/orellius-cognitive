<p align="center">
  <img src=".github/icon.png" width="120" alt="Orellius Cognitive" />
</p>

<h3 align="center">Orellius Cognitive</h3>
<p align="center">AI safety and personality SDK — psyche, red-teaming, sandboxing in Rust.</p>

<p align="center">
  <img src="https://img.shields.io/badge/status-archived-red?style=flat-square" />
  <img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" />
</p>

---

> [!WARNING]
> **This project is archived and no longer maintained.**
> No further updates, fixes, or support will be provided. The code is left
> here as-is under the MIT license — fork it, modify it, ship it, do whatever
> you want with it. No warranty, no promises.

---

## What it was

A Rust SDK for building AI agents with structured personality, safety guardrails, and containment. Six architectural layers covering personality modeling, voice/style extraction, instruction learning, adversarial red-teaming, process-level sandboxing, and I/O audit. Python bindings via PyO3.

## Layers

- **Psyche** — Freudian triple-agent pipeline (superego, ego, id) for personality consistency
- **Persona** — voice extraction and style transfer from reference text
- **Cortex** — instruction learning and few-shot adaptation
- **Shadow** — red-teaming engine for adversarial testing
- **Ironclad** — process-level sandboxing with capability restrictions
- **Glassbox** — I/O containment with full audit trail

## Quickstart

```rust
use orellius_cognitive::{Psyche, IroncladSandbox};

let psyche = Psyche::builder()
    .superego("helpful, cautious, precise")
    .ego("technical writer")
    .build()?;

let sandbox = IroncladSandbox::new()
    .allow_network(false)
    .allow_fs_read(&["/data"])
    .spawn()?;
```

## Stack

Rust · tokio · PyO3 (Python bindings) · criterion benchmarks

## License

MIT. Fork it, ship it, do whatever.
