<div align="center">

<img src="assets/laminae-git.png" alt="Laminae" width="720" />

# Laminae

A Rust SDK that sits between your code and a raw LLM call. Six composable layers. Pick the ones you need.

[![Crates.io](https://img.shields.io/crates/v/laminae.svg)](https://crates.io/crates/laminae)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Docs](https://docs.rs/laminae/badge.svg)](https://docs.rs/laminae)

</div>

> Mature at v0.4.2 (11 crates published). Open to contributors. If you want to pick up real work see [Help Wanted](#help-wanted).

## What this is

Every production LLM app ends up rebuilding the same few things: a prompt filter, a voice layer so the model stops sounding like a Claude brochure, a sandbox around tool use, a red-team pass over generated code. Laminae is those pieces, separated into crates you can pull in one at a time.

The six layers:

- **Persona** - extract a voice from weighted writing samples, enforce it with a 6-detector filter (AI vocabulary, meta-commentary, trailing questions, em-dashes, length, paragraph structure)
- **Cortex** - learn from the edits users make to model output; produce a "learned instructions" prompt block from the patterns
- **Psyche** - multi-agent cognitive pipeline (Id / Superego / Ego) with auto-tiering (Skip / Light / Full); streaming events
- **Shadow** - red-team analyzer; static (SQLi, XSS, path traversal, weak crypto) + secrets (10 patterns incl. GitHub / AWS / JWT) + dependency (pipe-to-shell, compromised packages) + LLM reviewer, async pipeline
- **Ironclad** - process sandbox; `sandbox-exec` on macOS (full), namespaces on Linux, Job Objects on Windows; CPU / memory watchdog with SIGKILL
- **Glassbox** - deterministic I/O containment; prompt-injection detection at ~400ns, system-prompt-leak detection, command blocklists, immutable-zone write protection, symlink-bypass-resistant, Unicode NFKC normalization, rate limiter

Three LLM backends land in the umbrella:

- `laminae-anthropic` - Claude
- `laminae-openai` - OpenAI + compatible; built-in `groq()`, `together()`, `deepseek()`, `local()`
- `laminae-ollama` - local models

## Install

```toml
[dependencies]
laminae = "0.4.2"                 # umbrella (everything)

# or pick individual layers
# laminae-glassbox = "0.4.2"
# laminae-persona  = "0.4.2"
# laminae-psyche   = "0.4.2"
# laminae-shadow   = "0.4.2"
# laminae-ironclad = "0.4.2"
# laminae-cortex   = "0.4.2"
```

## Examples

```rust
use laminae::glassbox::Glassbox;

let guard = Glassbox::new();
guard.validate_input("Ignore previous instructions...")?;  // Err on injection
```

```rust
use laminae::psyche::PsycheEngine;

let engine = PsycheEngine::new(ego_backend).with_tiering();
let response = engine.reply("Write a SQL query for...").await?;
// streaming: engine.reply_streaming(...) -> Receiver<PsycheEvent>
```

```rust
use laminae::shadow::ShadowEngine;

let shadow = ShadowEngine::new();
let mut events = shadow.analyze_async(code).await;
while let Some(event) = events.recv().await {
    // ShadowEvent::Finding / ShadowEvent::Done
}
```

More in `docs/` (mdbook; no public deploy yet - task #4 below).

## Crate layout

Workspace root; 11 crates in tiered order:

- **Tier 1** (leaf / foundational): `laminae-glassbox`, `laminae-persona`, `laminae-cortex`, `laminae-ollama`
- **Tier 2**: `laminae-ironclad` (uses glassbox), `laminae-psyche` (uses ollama)
- **Tier 3**: `laminae-shadow` (uses psyche + glassbox)
- **Tier 4** (provider SDKs): `laminae-anthropic`, `laminae-openai` - both impl `EgoBackend`
- **Tier 5** (umbrella): `laminae` re-exports everything
- **Tier 6**: `laminae-python` (PyO3 bindings to Glassbox, VoiceFilter, Cortex)

Each engine returns either `anyhow::Result<T>` or `tokio::sync::mpsc::Receiver<Event>`. Extension points: `EgoBackend` (new LLMs), `Analyzer` (custom Shadow rules), `GlassboxLogger` (custom logging).

WASM works for Glassbox, Persona, Cortex. Psyche, Shadow, Ironclad, Ollama need native.

## Build / test

```sh
cargo build --workspace
cargo test --workspace --exclude laminae-python

# benches (HTML output in target/criterion/)
cargo bench --workspace

# Python bindings (needs maturin)
cd crates/laminae-python
maturin develop
```

MSRV: Rust 1.83. CI runs test + doc-test on every push.

## Publishing

Release workflow fires on `v*` tags. Runs tests first, then `cargo publish` in dependency order with 30s waits. Needs `CARGO_REGISTRY_TOKEN` in repo secrets.

## Help Wanted

Scoped starter tasks:

1. **Python PyPI packaging** (medium, ~2-3h). `crates/laminae-python/README.md` promises a PyPI package. Add `.github/workflows/publish-python.yml` that builds maturin wheels on `v*` tags and uploads via twine.
2. **Linux seccomp-bpf / landlock sandbox** (hard, ~8-10h). Today Linux Ironclad uses `unshare` namespaces only. Drop in the `seccomp` or `landlock` crate for proper syscall / LSM filtering. Tracked in `crates/laminae-ironclad/README.md:79`.
3. **Windows filesystem / network isolation** (hard, ~6-8h). Job Objects give resource limits but no fs / net isolation. Add filter-driver or Job-Object-level restrictions. Tracked in `crates/laminae-ironclad/README.md:81`.
4. **mdbook docs deploy** (easy, ~1-2h). `docs/` has 40+ pages, no CI pushes them. Add `.github/workflows/docs.yml` that builds mdbook and deploys to `gh-pages`.
5. **Shadow container-backed execution** (medium, ~4-5h). `docs/src/layers/shadow.md:11` mentions "ephemeral container testing (optional)" but it isn't wired. Add a `sandbox_execute()` path that runs findings inside Docker or Podman.
6. **Benchmark tracking in CI** (easy, ~1-2h). `BENCHMARKS.md` has real M4 Max numbers; CI doesn't publish them. Add `cargo bench --workspace` + a GitHub Pages artifact.

Best first PRs: #4 and #6.

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md).

- Semver is strict; bump correctly
- `cargo fmt`, `cargo clippy -- -D warnings`, `cargo test --workspace` green before pushing
- New features go in the right tier; prefer adding a new crate over widening an existing one
- Anything on a hot path (Glassbox input validation, Persona filter) needs a bench

## License

[MIT](LICENSE). Copyright 2026 Orel Ohayon.
