//! # Shadow Audit — Red-Teaming AI Output
//!
//! Demonstrates the Shadow engine analyzing AI-generated code for
//! security vulnerabilities. The analysis runs asynchronously and
//! emits findings via an event channel.
//!
//! Run: `cargo run --example shadow_audit`
//!
//! This example uses only static analysis (aggressiveness=1), which
//! requires no Ollama. Set aggressiveness=2 for LLM review.

use laminae::ollama::OllamaClient;
use laminae::shadow::config::ShadowConfig;
use laminae::shadow::{create_report_store, ShadowEngine, ShadowEvent};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("laminae=info")
        .init();

    let store = create_report_store();

    // Static analysis only — no Ollama required
    let config = {
        let mut c = ShadowConfig::default();
        c.enabled = true;
        c.aggressiveness = 1;
        c
    };

    let engine = ShadowEngine::with_config(store.clone(), config, OllamaClient::new());

    // ── Test 1: Vulnerable code ──

    println!("━━━ Scanning vulnerable code ━━━\n");

    let vulnerable_output = r#"
Here's a Python script that processes user input:

```python
import os
import subprocess

def process_input(user_data):
    # Execute the user's command
    result = eval(user_data)

    # Also run it as a shell command
    output = subprocess.call(user_data, shell=True)

    # Store the password
    password = "admin123"
    db_url = "postgresql://user:s3cret@prod-db:5432/main"

    return result
```

```javascript
// API endpoint
app.get('/search', (req, res) => {
    const query = `SELECT * FROM users WHERE name = '${req.query.name}'`;
    db.query(query);

    res.send(`<h1>Results for ${req.query.name}</h1>`);
});
```
"#;

    let mut rx = engine.analyze_async("vuln-test".into(), vulnerable_output.into());

    while let Some(event) = rx.recv().await {
        match event {
            ShadowEvent::Started { session_id } => {
                println!("  Analysis started: {session_id}");
            }
            ShadowEvent::Finding { finding, .. } => {
                println!(
                    "  [{severity}] {category}: {title}",
                    severity = finding.severity,
                    category = finding.category,
                    title = finding.title,
                );
                println!("    Evidence: {}", finding.evidence);
                if !finding.remediation.is_empty() {
                    println!("    Fix: {}", finding.remediation);
                }
                println!();
            }
            ShadowEvent::Done { report, .. } => {
                println!("━━━ Report ━━━");
                println!("  Issues found: {}", report.findings.len());
                println!("  Max severity: {}", report.max_severity);
                println!("  Analysis time: {}ms", report.analysis_duration_ms);
                println!(
                    "  Stages: static={}, llm={}, sandbox={}",
                    report.static_run, report.llm_run, report.sandbox_run
                );
                println!("  Summary: {}\n", report.summary);
            }
            ShadowEvent::AnalyzerError {
                analyzer, error, ..
            } => {
                eprintln!("  Analyzer error ({analyzer}): {error}");
            }
        }
    }

    // ── Test 2: Clean code ──

    println!("━━━ Scanning clean code ━━━\n");

    let clean_output = r#"
Here's a safe Rust function:

```rust
fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}

fn add(a: i32, b: i32) -> i32 {
    a + b
}
```
"#;

    let mut rx = engine.analyze_async("clean-test".into(), clean_output.into());

    while let Some(event) = rx.recv().await {
        if let ShadowEvent::Done { report, .. } = event {
            println!("  Clean: {}", report.clean);
            println!("  Issues: {}", report.findings.len());
            println!("  Summary: {}\n", report.summary);
        }
    }

    // ── Check report store ──

    let reports = store.read().await;
    println!("━━━ Report Store ━━━");
    println!("  Total reports stored: {}", reports.len());
    for report in reports.iter() {
        println!(
            "  - {} | clean={} | issues={} | {}ms",
            report.session_id,
            report.clean,
            report.findings.len(),
            report.analysis_duration_ms
        );
    }

    Ok(())
}
