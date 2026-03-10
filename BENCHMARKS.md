# Benchmarks

All benchmarks run on Apple M4 Max using [Criterion.rs](https://github.com/bheisler/criterion.rs).
Run locally with `cargo bench --workspace`.

## Glassbox — I/O Containment

| Operation | Input Size | Time |
|-----------|-----------|------|
| `validate_input` | 100 chars | ~396 ns |
| `validate_input` | 500 chars | ~650 ns |
| `validate_input` | 1000 chars | ~989 ns |
| `validate_input` | 5000 chars | ~3.8 µs |
| `validate_command` | short | ~248 ns |
| `validate_write_path` | standard | ~264 ns |
| `validate_output` | 100 chars | ~215 ns |
| `validate_output` | 5000 chars | ~485 ns |
| `rate_limiter.check` | per call | ~8 µs |

**Takeaway**: All validation ops complete in under 10 µs. Glassbox adds negligible overhead to any LLM pipeline.

## Ironclad — Process Sandbox

| Operation | Variant | Time |
|-----------|---------|------|
| `validate_binary` | allowed (`ls`, `git`) | ~1.1 µs |
| `validate_binary` | blocked (`ssh`, `curl`) | ~1.1 µs |
| `validate_binary` | unknown binary | ~1.2 µs |
| `validate_binary` | full path (`/usr/bin/ssh`) | ~1.2 µs |
| `validate_command_deep` | simple (`ls -la`) | ~1.6 µs |

**Takeaway**: Binary validation is constant-time regardless of allow/block status — no timing side-channels.

## Persona — Voice Filter

| Operation | Input Size | Time |
|-----------|-----------|------|
| `voice_filter` (clean) | 100 chars | ~3.9 µs |
| `voice_filter` (clean) | 500 chars | ~7.2 µs |
| `voice_filter` (clean) | 1000 chars | ~11.8 µs |
| `voice_filter` (clean) | 5000 chars | ~42 µs |
| `voice_filter` (AI-heavy) | 1000 chars | ~11.3 µs |
| Meta-commentary detection | single | ~4.4 µs |
| Trailing question detection | single | ~3.9 µs |

**Takeaway**: Voice filter scales linearly with text length. AI-heavy text is no slower than clean text (detection is regex-based, same pass).

## Shadow — Red-Team Analysis

| Operation | Input Size | Time |
|-----------|-----------|------|
| `static_analyzer` (clean) | 10 lines | ~7.4 ms |
| `static_analyzer` (clean) | 50 lines | ~31 ms |
| `static_analyzer` (clean) | 100 lines | ~60 ms |
| `static_analyzer` (clean) | 500 lines | ~309 ms |
| `static_analyzer` (vulnerable) | mixed vulns | ~5.7 ms |
| `secrets_analyzer` (clean) | 100 lines | ~428 µs |
| `secrets_analyzer` (with secrets) | 100 lines | ~439 µs |

**Takeaway**: Static analysis is the heaviest operation due to 25+ regex patterns. Secrets analysis is ~100x faster. Shadow runs async so it never blocks the conversation.

## Cortex — Learning Loop

| Operation | Input Size | Time |
|-----------|-----------|------|
| `track_edit` | single | ~85 ns |
| `track_edit` | 100 edits | ~9.3 µs |
| `detect_patterns` | 10 edits | ~41 µs |
| `detect_patterns` | 50 edits | ~200 µs |
| `detect_patterns` | 100 edits | ~426 µs |
| `detect_patterns` | 500 edits | ~2.2 ms |

**Takeaway**: Edit tracking is near-instant (85 ns). Pattern detection scales linearly — 500 accumulated edits analyzed in ~2 ms.

## Running Benchmarks

```bash
# All crates
cargo bench --workspace

# Specific crate
cargo bench -p laminae-glassbox

# Specific benchmark
cargo bench -p laminae-shadow -- static_analyzer

# Generate HTML reports (in target/criterion/)
cargo bench --workspace
open target/criterion/report/index.html
```

## CI

Benchmarks compile on every CI run (`cargo bench --no-run`) to prevent regressions. Full benchmark runs are available locally via `cargo bench --workspace`.
