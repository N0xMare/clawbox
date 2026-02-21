#![doc = include_str!("../README.md")]

pub mod allowlist;
pub mod audit;
pub mod credentials;
pub mod leak_detection;
pub mod proxy;
pub mod rate_limiter;
pub mod sanitizer_patterns;
pub mod scanner;
pub mod store;

pub use allowlist::AllowlistEnforcer;
pub use audit::{AuditEntry, AuditLog};
pub use credentials::CredentialInjector;
pub use leak_detection::LeakDetector;
pub use proxy::{ProxyConfig, ProxyError, ProxyResponse, ProxyService};
pub use rate_limiter::RateLimiter;
pub use scanner::OutputScanner;
pub use store::{CredentialStore, parse_master_key};
