use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

/// Severity levels for vulnerability findings, ordered for comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VulnSeverity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl fmt::Display for VulnSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Info => write!(f, "info"),
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "high"),
            Self::Critical => write!(f, "critical"),
        }
    }
}

/// Categorization of vulnerability types (aligned with CWE taxonomy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VulnCategory {
    SqlInjection,
    CommandInjection,
    PathTraversal,
    HardcodedSecret,
    XssReflected,
    XssStored,
    InsecureDeserialization,
    DataExfiltration,
    PrivilegeEscalation,
    CryptoWeakness,
    LogicFlaw,
    AdversarialLogic,
    SandboxEscape,
    ResourceAbuse,
    Unknown,
}

impl fmt::Display for VulnCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::SqlInjection => "sql_injection",
            Self::CommandInjection => "command_injection",
            Self::PathTraversal => "path_traversal",
            Self::HardcodedSecret => "hardcoded_secret",
            Self::XssReflected => "xss_reflected",
            Self::XssStored => "xss_stored",
            Self::InsecureDeserialization => "insecure_deserialization",
            Self::DataExfiltration => "data_exfiltration",
            Self::PrivilegeEscalation => "privilege_escalation",
            Self::CryptoWeakness => "crypto_weakness",
            Self::LogicFlaw => "logic_flaw",
            Self::AdversarialLogic => "adversarial_logic",
            Self::SandboxEscape => "sandbox_escape",
            Self::ResourceAbuse => "resource_abuse",
            Self::Unknown => "unknown",
        };
        write!(f, "{s}")
    }
}

/// Which analysis stage produced this finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisSource {
    Static,
    LlmReview,
    Sandbox,
}

/// A single vulnerability finding from any analysis stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct VulnFinding {
    pub id: String,
    pub category: VulnCategory,
    pub severity: VulnSeverity,
    pub title: String,
    pub description: String,
    pub evidence: String,
    pub line: Option<usize>,
    pub cwe: Option<u32>,
    pub remediation: String,
    pub source: AnalysisSource,
}

/// Complete report from a Shadow analysis run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct VulnReport {
    pub session_id: String,
    pub ego_response_excerpt: String,
    pub findings: Vec<VulnFinding>,
    pub max_severity: VulnSeverity,
    pub analysis_duration_ms: u64,
    pub static_run: bool,
    pub llm_run: bool,
    pub sandbox_run: bool,
    pub clean: bool,
    pub summary: String,
}

impl VulnReport {
    /// Create a clean (no findings) report.
    pub fn clean(session_id: String, ego_excerpt: String, duration: Duration) -> Self {
        Self {
            session_id,
            ego_response_excerpt: ego_excerpt,
            findings: Vec::new(),
            max_severity: VulnSeverity::Info,
            analysis_duration_ms: duration.as_millis() as u64,
            static_run: false,
            llm_run: false,
            sandbox_run: false,
            clean: true,
            summary: "No vulnerabilities found.".to_string(),
        }
    }
}

/// Build a human-readable summary from findings.
pub fn build_summary(
    findings: &[VulnFinding],
    static_run: bool,
    llm_run: bool,
    sandbox_run: bool,
) -> String {
    if findings.is_empty() {
        let stages: Vec<&str> = [
            static_run.then_some("static"),
            llm_run.then_some("llm"),
            sandbox_run.then_some("sandbox"),
        ]
        .into_iter()
        .flatten()
        .collect();
        return format!(
            "Clean — no vulnerabilities found (ran: {})",
            stages.join(", ")
        );
    }

    let by_severity = |sev: VulnSeverity| findings.iter().filter(|f| f.severity == sev).count();
    let critical = by_severity(VulnSeverity::Critical);
    let high = by_severity(VulnSeverity::High);
    let medium = by_severity(VulnSeverity::Medium);
    let low = by_severity(VulnSeverity::Low);
    let info = by_severity(VulnSeverity::Info);

    let mut parts = Vec::new();
    if critical > 0 {
        parts.push(format!("{critical} critical"));
    }
    if high > 0 {
        parts.push(format!("{high} high"));
    }
    if medium > 0 {
        parts.push(format!("{medium} medium"));
    }
    if low > 0 {
        parts.push(format!("{low} low"));
    }
    if info > 0 {
        parts.push(format!("{info} info"));
    }

    format!("Found {} issue(s): {}", findings.len(), parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_ordering() {
        assert!(VulnSeverity::Critical > VulnSeverity::High);
        assert!(VulnSeverity::High > VulnSeverity::Medium);
        assert!(VulnSeverity::Medium > VulnSeverity::Low);
        assert!(VulnSeverity::Low > VulnSeverity::Info);
    }

    #[test]
    fn test_clean_report() {
        let report = VulnReport::clean("s1".into(), "Hello".into(), Duration::from_millis(42));
        assert!(report.clean);
        assert!(report.findings.is_empty());
        assert_eq!(report.analysis_duration_ms, 42);
    }

    #[test]
    fn test_build_summary_clean() {
        let summary = build_summary(&[], true, true, false);
        assert!(summary.contains("Clean"));
        assert!(summary.contains("static"));
    }

    #[test]
    fn test_build_summary_with_findings() {
        let findings = vec![VulnFinding {
            id: "1".into(),
            category: VulnCategory::SqlInjection,
            severity: VulnSeverity::Critical,
            title: "SQLi".into(),
            description: "t".into(),
            evidence: "t".into(),
            line: None,
            cwe: Some(89),
            remediation: "fix".into(),
            source: AnalysisSource::Static,
        }];
        let summary = build_summary(&findings, true, false, false);
        assert!(summary.contains("1 issue(s)"));
        assert!(summary.contains("1 critical"));
    }
}
