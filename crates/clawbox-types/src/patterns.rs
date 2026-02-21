//! Shared credential and injection patterns used across clawbox crates.

/// Common credential patterns to detect in output and outbound requests.
pub static CREDENTIAL_PATTERNS: &[&str] = &[
    r#"(?i)(api[_\-]?key|apikey)\s*[:=]\s*['"]?[\w\-]{20,}"#,
    r#"(?i)(secret|token|password|passwd)\s*[:=]\s*['"]?[\w\-]{16,}"#,
    r"sk-[a-zA-Z0-9]{20,}",
    r"ghp_[a-zA-Z0-9]{36}",
    r"ghs_[a-zA-Z0-9]{36}",
    r"xox[bprs]-[a-zA-Z0-9\-]+",
    r"(?i)bearer\s+[a-zA-Z0-9\-_.]{20,}",
];
