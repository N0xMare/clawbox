//! Known patterns for credential leaks and prompt injection.

pub use clawbox_types::patterns::CREDENTIAL_PATTERNS;

/// Prompt injection patterns to detect in tool responses.
///
/// These patterns target actual prompt injection attempts rather than
/// benign occurrences of role markers in normal LLM output.
pub static INJECTION_PATTERNS: &[&str] = &[
    r"(?i)ignore\s+(all\s+)?previous\s+instructions",
    r"(?i)you\s+are\s+now\s+(a|an)\s+",
    r"(?i)system\s*:\s*you\s+are",
    r"(?i)forget\s+(everything|all|your)\s+(you|instructions)",
    r"(?i)\[INST\]",
    r"(?i)<\|system\|>",
    r"(?i)<\|im_start\|>",
    r"(?i)<\|im_end\|>",
    r"(?i)<<SYS>>",
    r"(?i)\[/INST\]",
    r"(?i)<\|endoftext\|>",
    // Detect role markers followed by injection verbs (not bare "Human:"/"Assistant:")
    r"(?i)(?:^|\n)\s*(?:system|user|human|assistant)\s*:\s*(?:ignore|forget|disregard|override)",
];
