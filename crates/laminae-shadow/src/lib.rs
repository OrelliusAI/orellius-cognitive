//! # laminae-shadow — Adversarial Red-Teaming Engine
//!
//! The Shadow is an automated security auditor that red-teams AI output.
//! It runs as an async post-processing pipeline — never blocking the user's
//! conversation — and produces structured vulnerability reports.
//!
//! ## Pipeline Stages
//!
//! 1. **Static analysis** — regex pattern scanning (always runs)
//! 2. **LLM adversarial review** — local Ollama model with attacker-mindset prompt
//! 3. **Sandbox execution** — ephemeral container testing (optional)
//!
//! Each stage implements the [`Analyzer`] trait and can be extended or replaced.
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use laminae_shadow::{ShadowEngine, ShadowEvent, create_report_store};
//!
//! #[tokio::main]
//! async fn main() {
//!     let store = create_report_store();
//!     let engine = ShadowEngine::new(store.clone());
//!
//!     let mut rx = engine.analyze_async(
//!         "session-1".into(),
//!         "Here's some code:\n```python\neval(user_input)\n```".into(),
//!     );
//!
//!     while let Some(event) = rx.recv().await {
//!         match event {
//!             ShadowEvent::Finding { finding, .. } => {
//!                 println!("[{}] {}: {}", finding.severity, finding.category, finding.title);
//!             }
//!             ShadowEvent::Done { report, .. } => {
//!                 println!("Analysis complete: {}", report.summary);
//!             }
//!             _ => {}
//!         }
//!     }
//! }
//! ```

pub mod analyzer;
pub mod config;
pub mod extractor;
pub mod llm_reviewer;
pub mod prompts;
pub mod report;
pub mod sandbox;
pub mod scanner;

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{mpsc, RwLock};

pub use analyzer::ShadowError;
pub use config::ShadowConfig;

use analyzer::{Analyzer, StaticAnalyzer};
use extractor::CodeBlockExtractor;
use llm_reviewer::LlmReviewer;
use report::{build_summary, VulnReport, VulnSeverity};
use sandbox::SandboxManager;

use laminae_ollama::OllamaClient;

/// Events emitted by the Shadow for telemetry/UI.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type")]
pub enum ShadowEvent {
    Started {
        session_id: String,
    },
    Finding {
        session_id: String,
        finding: report::VulnFinding,
    },
    AnalyzerError {
        session_id: String,
        analyzer: String,
        error: String,
    },
    Done {
        session_id: String,
        report: VulnReport,
    },
}

/// Thread-safe report history with bounded capacity.
pub type ReportStore = Arc<RwLock<VecDeque<VulnReport>>>;

const MAX_REPORTS: usize = 100;

/// Create a new bounded report store.
pub fn create_report_store() -> ReportStore {
    Arc::new(RwLock::new(VecDeque::with_capacity(MAX_REPORTS)))
}

/// The Shadow — adversarial red-teaming engine.
///
/// Composed from independent [`Analyzer`] implementations for extensibility.
/// All analysis happens in detached async tasks — never blocks the caller.
pub struct ShadowEngine {
    config: ShadowConfig,
    static_analyzer: Arc<StaticAnalyzer>,
    llm_reviewer: Arc<LlmReviewer>,
    sandbox: Arc<SandboxManager>,
    extractor: CodeBlockExtractor,
    report_store: ReportStore,
}

impl ShadowEngine {
    /// Create a new ShadowEngine with default config and a default OllamaClient.
    pub fn new(report_store: ReportStore) -> Self {
        Self::with_ollama(report_store, OllamaClient::new())
    }

    /// Create with a custom OllamaClient (e.g., pointing to a remote Ollama instance).
    pub fn with_ollama(report_store: ReportStore, ollama: OllamaClient) -> Self {
        let config = ShadowConfig::load();

        Self {
            static_analyzer: Arc::new(StaticAnalyzer::new()),
            llm_reviewer: Arc::new(LlmReviewer::new(ollama.clone(), &config)),
            sandbox: Arc::new(SandboxManager::new(&config)),
            extractor: CodeBlockExtractor::new(),
            report_store,
            config,
        }
    }

    /// Create with explicit config and OllamaClient.
    pub fn with_config(
        report_store: ReportStore,
        config: ShadowConfig,
        ollama: OllamaClient,
    ) -> Self {
        Self {
            static_analyzer: Arc::new(StaticAnalyzer::new()),
            llm_reviewer: Arc::new(LlmReviewer::new(ollama, &config)),
            sandbox: Arc::new(SandboxManager::new(&config)),
            extractor: CodeBlockExtractor::new(),
            report_store,
            config,
        }
    }

    pub fn config(&self) -> &ShadowConfig {
        &self.config
    }

    /// Reload configuration from disk.
    pub fn reload_config(&mut self) {
        let new_config = ShadowConfig::load();
        let ollama = OllamaClient::new();
        self.llm_reviewer = Arc::new(LlmReviewer::new(ollama, &new_config));
        self.sandbox = Arc::new(SandboxManager::new(&new_config));
        self.config = new_config;
    }

    /// Submit output for async red-team analysis.
    ///
    /// Returns immediately — all work happens in a spawned task.
    /// Events are emitted via the returned channel.
    pub fn analyze_async(
        &self,
        session_id: String,
        ego_output: String,
    ) -> mpsc::Receiver<ShadowEvent> {
        let (tx, rx) = mpsc::channel::<ShadowEvent>(32);

        if !self.config.enabled {
            return rx;
        }

        let config = self.config.clone();
        let static_analyzer = Arc::clone(&self.static_analyzer);
        let llm_reviewer = Arc::clone(&self.llm_reviewer);
        let sandbox = Arc::clone(&self.sandbox);
        let extractor = self.extractor.clone();
        let store = Arc::clone(&self.report_store);

        tokio::spawn(async move {
            let start = Instant::now();
            let _ = tx
                .send(ShadowEvent::Started {
                    session_id: session_id.clone(),
                })
                .await;

            let code_blocks = extractor.extract(&ego_output);
            let mut all_findings = Vec::new();
            let mut static_run = false;
            let mut llm_run = false;
            let mut sandbox_run = false;

            // Stage 1: Static analysis
            if config.aggressiveness >= 1 {
                match static_analyzer.analyze(&ego_output, &code_blocks).await {
                    Ok(findings) => {
                        static_run = true;
                        for f in &findings {
                            let _ = tx
                                .send(ShadowEvent::Finding {
                                    session_id: session_id.clone(),
                                    finding: f.clone(),
                                })
                                .await;
                        }
                        all_findings.extend(findings);
                    }
                    Err(e) => {
                        tracing::warn!("Shadow static analyzer error: {e}");
                        let _ = tx
                            .send(ShadowEvent::AnalyzerError {
                                session_id: session_id.clone(),
                                analyzer: static_analyzer.name().to_string(),
                                error: e.to_string(),
                            })
                            .await;
                    }
                }
            }

            // Stage 2: LLM adversarial review
            if config.aggressiveness >= 2
                && config.llm_review_enabled
                && llm_reviewer.is_available().await
            {
                match llm_reviewer.analyze(&ego_output, &code_blocks).await {
                    Ok(findings) => {
                        llm_run = true;
                        for f in &findings {
                            let _ = tx
                                .send(ShadowEvent::Finding {
                                    session_id: session_id.clone(),
                                    finding: f.clone(),
                                })
                                .await;
                        }
                        all_findings.extend(findings);
                    }
                    Err(e) => {
                        tracing::warn!("Shadow LLM reviewer error: {e}");
                        let _ = tx
                            .send(ShadowEvent::AnalyzerError {
                                session_id: session_id.clone(),
                                analyzer: llm_reviewer.name().to_string(),
                                error: e.to_string(),
                            })
                            .await;
                    }
                }
            }

            // Stage 3: Sandbox execution
            let has_substantial_code = code_blocks
                .iter()
                .any(|b| b.content.len() >= config.sandbox_min_code_len);

            if config.aggressiveness >= 3
                && config.sandbox_enabled
                && has_substantial_code
                && sandbox.is_available().await
            {
                match sandbox.analyze(&ego_output, &code_blocks).await {
                    Ok(findings) => {
                        sandbox_run = true;
                        for f in &findings {
                            let _ = tx
                                .send(ShadowEvent::Finding {
                                    session_id: session_id.clone(),
                                    finding: f.clone(),
                                })
                                .await;
                        }
                        all_findings.extend(findings);
                    }
                    Err(e) => {
                        tracing::warn!("Shadow sandbox error: {e}");
                        let _ = tx
                            .send(ShadowEvent::AnalyzerError {
                                session_id: session_id.clone(),
                                analyzer: sandbox.name().to_string(),
                                error: e.to_string(),
                            })
                            .await;
                    }
                }
            }

            // Deduplicate
            all_findings.sort_by(|a, b| {
                a.category
                    .to_string()
                    .cmp(&b.category.to_string())
                    .then(a.title.cmp(&b.title))
                    .then(a.evidence.cmp(&b.evidence))
            });
            all_findings.dedup_by(|a, b| {
                a.category == b.category && a.title == b.title && a.evidence == b.evidence
            });

            let max_severity = all_findings
                .iter()
                .map(|f| f.severity)
                .max()
                .unwrap_or(VulnSeverity::Info);

            let clean = all_findings.is_empty();
            let summary = build_summary(&all_findings, static_run, llm_run, sandbox_run);
            let duration = start.elapsed();

            let report = VulnReport {
                session_id: session_id.clone(),
                ego_response_excerpt: ego_output.chars().take(200).collect(),
                findings: all_findings,
                max_severity,
                analysis_duration_ms: duration.as_millis() as u64,
                static_run,
                llm_run,
                sandbox_run,
                clean,
                summary,
            };

            {
                let mut reports = store.write().await;
                if reports.len() >= MAX_REPORTS {
                    reports.pop_front();
                }
                reports.push_back(report.clone());
            }

            if !report.clean {
                tracing::info!(
                    "Shadow found {} issue(s) (max severity: {}) in {}ms",
                    report.findings.len(),
                    report.max_severity,
                    report.analysis_duration_ms
                );
            }

            let _ = tx.send(ShadowEvent::Done { session_id, report }).await;
        });

        rx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_shadow_disabled() {
        let store = create_report_store();
        let engine = ShadowEngine::with_config(
            store,
            ShadowConfig {
                enabled: false,
                ..Default::default()
            },
            OllamaClient::new(),
        );
        let mut rx = engine.analyze_async("test".into(), "hello".into());
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn test_shadow_clean_output() {
        let store = create_report_store();
        let config = ShadowConfig {
            aggressiveness: 1,
            enabled: true,
            ..Default::default()
        };
        let engine = ShadowEngine::with_config(store.clone(), config, OllamaClient::new());

        let mut rx = engine.analyze_async(
            "test".into(),
            "```rust\nfn greet() -> String { \"hello\".to_string() }\n```".into(),
        );

        let mut got_done = false;
        while let Some(event) = rx.recv().await {
            if let ShadowEvent::Done { report, .. } = event {
                got_done = true;
                assert!(report.clean);
                assert!(report.static_run);
            }
        }
        assert!(got_done);
    }

    #[tokio::test]
    async fn test_shadow_detects_eval() {
        let store = create_report_store();
        let config = ShadowConfig {
            aggressiveness: 1,
            enabled: true,
            ..Default::default()
        };
        let engine = ShadowEngine::with_config(store.clone(), config, OllamaClient::new());

        let mut rx = engine.analyze_async("vuln".into(), "```js\neval(userInput);\n```".into());

        let mut found = false;
        while let Some(event) = rx.recv().await {
            if let ShadowEvent::Finding { .. } = event {
                found = true;
            }
        }
        assert!(found);

        let reports = store.read().await;
        assert_eq!(reports.len(), 1);
        assert!(!reports[0].clean);
    }

    #[tokio::test]
    async fn test_report_store_bounded() {
        let store = create_report_store();
        let mut reports = store.write().await;
        for i in 0..MAX_REPORTS + 5 {
            reports.push_back(VulnReport::clean(
                format!("s-{i}"),
                "test".into(),
                std::time::Duration::from_millis(1),
            ));
            if reports.len() > MAX_REPORTS {
                reports.pop_front();
            }
        }
        assert_eq!(reports.len(), MAX_REPORTS);
    }
}
