# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] - 2026-03-11

### Breaking Changes
- `PsycheEngine::reply()` now returns `Err(PsycheError::Blocked(...))` instead of `Ok(block_reason)` when the Superego blocks a request. Callers that previously matched on `Ok(text)` to detect blocks must now handle the `Err` path and downcast through anyhow.
- `ClaudeBackend::with_config()` and `OpenAIBackend::with_config()` now return `Result<Self, ClaudeError>` / `Result<Self, OpenAIError>` instead of panicking on HTTP client build failure. New `HttpClient` error variant added to both error enums.
- Removed `HealingSuggestion` struct and `healing_suggestion` field from `VulnReport` (was dead code, never populated).
- `StaticAnalyzer::with_extra_rules()` now returns `Result<Self, regex::Error>` instead of silently discarding rules with invalid patterns.
- `ShadowRule` fields changed from `&'static str` to `Cow<'static, str>` to support runtime rule construction.
- `#[non_exhaustive]` added to `VulnReport`, `VulnFinding`, `ShadowConfig`, `PsycheConfig`, `ClaudeConfig`, `OpenAIConfig`. Config structs support `..Default::default()` for external construction. `VulnReport` and `VulnFinding` are engine-produced types not intended for direct construction.

### Added
- MSRV (1.75) CI job to catch compatibility regressions.
- Python bindings CI job using `PyO3/maturin-action`.
- `ShadowRule` public struct with `Cow<'static, str>` fields for runtime-defined vulnerability rules.
- `StaticAnalyzer::with_extra_rules()` constructor for extending the static analyzer with custom rules.
- `ClaudeError::HttpClient` and `OpenAIError::HttpClient` error variants for recoverable HTTP client construction failures.
- `ClaudeBackend::from_env()` and `OpenAIBackend::from_env()` now propagate HTTP client errors instead of panicking.

### Changed
- `VulnCategory::Display` replaced serde_json heap allocation with zero-alloc `match` (performance).
- `DependencyAnalyzer` now maps findings to proper `VulnCategory` variants instead of hardcoding `Unknown`.
- Sandbox `detect_runtime()` cached via `tokio::sync::OnceCell` (avoids spawning 2 Docker/Podman subprocesses per analysis).
- Finding deduplication now includes title in the key to prevent collapsing different rules with matching evidence.

### Fixed
- `log_glassbox_event` and `TracingLogger::log` now include the `category` parameter in all log output (was silently dropped).
- `ShadowConfig::load_from` warns on malformed JSON instead of silently falling back to defaults.
- `ShadowConfig::load_from` file-read error path now runs `clamp()` to maintain the API contract.

### Removed
- Dead `HealingSuggestion` struct (was never generated or populated).
- Phantom `chrono` dependency from `laminae-python` (was unused).

## [0.3.1] - 2026-03-10

### Security
- Fixed macOS Seatbelt network policy: replaced wildcard port 443 with per-host rules from whitelisted_hosts.
- Fixed Linux sandbox: fail-closed when network isolation (unshare) fails with NetworkPolicy::None.
- Restricted DNS rules to system resolvers only (127.0.0.1, ::1) on macOS sandbox.
- Windows sandbox now applies Job Object memory and process limits.
- Added Unicode NFKC normalization in Glassbox to prevent fullwidth character bypasses.
- Expanded environment variable scrubbing (added GOOGLE_APPLICATION_CREDENTIALS, AZURE_CLIENT_SECRET, NPM_TOKEN, DOCKER_AUTH_CONFIG, KUBECONFIG, FIREBASE_TOKEN, HEROKU_API_KEY, DIGITALOCEAN_ACCESS_TOKEN).

### Added
- Typed error enums: `IroncladError`, `ShadowError`, `PsycheError`, `OllamaError`, `ClaudeError`, `OpenAIError`.
- `#[must_use]` on Glassbox and Ironclad validation functions.
- `ShadowConfig` and `ShadowError` re-exported from crate root.
- `PsycheConfig` builder methods: `with_id_model()`, `with_superego_model()`, `with_id_temperature()`, `with_ego_system_prompt()`.
- `CHANGELOG.md` following keepachangelog format.
- `cargo-audit` security scanning in CI.
- Doc-test compilation in CI.

### Changed
- Pre-compiled regexes in Shadow static/dependency/secrets analyzers (5-10x performance improvement).
- Rate limiter in Glassbox now prunes stale entries to prevent memory growth.
- Fixed `pid as i32` overflow in Ironclad process tree termination.
- Renamed internal `uuid_v4()` to `generate_finding_id()` for accuracy.

### Fixed
- Version strings in lib.rs (was 0.1) and docs (was 0.2) now match actual version.
- MSRV consistently set to 1.75 across README, CONTRIBUTING.md, and Cargo.toml.

## [0.3.0] - 2026-03-10

### Added
- Python bindings via PyO3 for Glassbox, VoiceFilter, and Cortex.
- WASM support for Glassbox, Persona (voice filter), and Cortex.
- Windows sandbox support via Job Objects in Ironclad.
- First-class Anthropic (Claude) and OpenAI-compatible EgoBackend crates.
- Criterion.rs benchmarks across all crates.
- Quality scoring in Shadow with `SecretsAnalyzer` and `DependencyAnalyzer`.
- Shadow sandbox execution stage for ephemeral container testing.
- Documentation site.

### Changed
- MSRV raised to 1.75.

## [0.2.0] - 2026-01-15

### Added
- Cortex crate for self-improving learning from user edits.
- Persona crate for voice extraction and style enforcement.
- Shadow adversarial red-teaming engine with static analysis and LLM review.
- Ironclad process sandbox with command whitelist and resource watchdog.
- Glassbox I/O containment with input/output validation, command filtering, and rate limiting.
- Ollama HTTP client crate.

### Changed
- Workspace restructured into independent crates.

[Unreleased]: https://github.com/orellius/laminae/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/orellius/laminae/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/orellius/laminae/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/orellius/laminae/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/orellius/laminae/releases/tag/v0.2.0
