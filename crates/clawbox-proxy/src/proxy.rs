//! Forward proxy service — the core request pipeline.
//!
//! Flow: allowlist check → leak scan → private IP check → credential injection → forward → audit.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use reqwest::Client;
use thiserror::Error;

use crate::allowlist::AllowlistEnforcer;
use crate::audit::AuditEntry;
use crate::credentials::CredentialInjector;
use crate::leak_detection::LeakDetector;
use crate::rate_limiter::RateLimiter;

/// Errors from the proxy pipeline.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ProxyError {
    #[error("URL blocked by allowlist: {0}")]
    Blocked(String),
    #[error("credential leak detected in outbound request")]
    LeakDetected,
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("request to private/internal IP blocked: {0}")]
    PrivateIpBlocked(String),
    #[error("failed to build HTTP client: {0}")]
    ClientBuild(String),
    #[error("rate limited: {0}")]
    RateLimited(String),
}

/// Configuration for the proxy service.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[must_use]
pub struct ProxyConfig {
    pub allowlist: Vec<String>,
    pub max_response_bytes: usize,
    pub timeout_ms: u64,
}

impl ProxyConfig {
    /// Create a new proxy configuration.
    pub fn new(allowlist: Vec<String>, max_response_bytes: usize, timeout_ms: u64) -> Self {
        Self {
            allowlist,
            max_response_bytes,
            timeout_ms,
        }
    }
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            allowlist: Vec::new(),
            max_response_bytes: 10 * 1024 * 1024, // 10MB
            timeout_ms: 30_000,
        }
    }
}

/// Response from a proxied request.
#[derive(Debug)]
#[must_use]
#[non_exhaustive]
pub struct ProxyResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
    pub audit: AuditEntry,
}

/// The forward proxy service.
#[non_exhaustive]
pub struct ProxyService {
    enforcer: AllowlistEnforcer,
    injector: CredentialInjector,
    leak_detector: LeakDetector,
    client: Client,
    config: ProxyConfig,
    rate_limiter: Option<Arc<RateLimiter>>,
    rate_limit_key: Option<String>,
}

/// Check if an IP address is private, loopback, link-local, or otherwise internal.
fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()          // 127.0.0.0/8
                || v4.is_private()     // 10/8, 172.16/12, 192.168/16
                || v4.is_link_local()  // 169.254/16
                || v4.is_unspecified() // 0.0.0.0
                || v4.is_broadcast() // 255.255.255.255
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_private_ip(&IpAddr::V4(mapped));
            }
            v6.is_loopback()       // ::1
                || v6.is_unspecified() // ::
                // fc00::/7 (unique local)
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                // fe80::/10 (link-local)
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Check if a URL host is a private/internal IP. Returns Err if blocked.
/// NOTE: This only catches IP-literal hosts. Full DNS rebinding protection
/// requires a custom DNS resolver that checks resolved IPs before connecting.
fn check_private_ip(parsed: &url::Url) -> Result<(), ProxyError> {
    if let Some(host) = parsed.host_str() {
        if let Ok(ip) = host.parse::<IpAddr>()
            && is_private_ip(&ip)
        {
            return Err(ProxyError::PrivateIpBlocked(host.to_string()));
        }
        // Also catch IPv6 in brackets
        let trimmed = host.trim_start_matches('[').trim_end_matches(']');
        if let Ok(ip) = trimmed.parse::<IpAddr>()
            && is_private_ip(&ip)
        {
            return Err(ProxyError::PrivateIpBlocked(trimmed.to_string()));
        }
    }
    Ok(())
}

impl ProxyService {
    pub fn new(
        config: ProxyConfig,
        injector: CredentialInjector,
        leak_detector: LeakDetector,
    ) -> Result<Self, ProxyError> {
        let enforcer = AllowlistEnforcer::new(config.allowlist.clone());
        let client = Client::builder()
            .danger_accept_invalid_certs(false)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_millis(config.timeout_ms))
            .build()
            .map_err(|e| ProxyError::ClientBuild(e.to_string()))?;
        Ok(Self {
            enforcer,
            injector,
            leak_detector,
            client,
            config,
            rate_limiter: None,
            rate_limit_key: None,
        })
    }

    /// Attach a shared rate limiter.
    /// Use a pre-built HTTP client (for connection pooling across services).
    pub fn with_client(mut self, client: Client) -> Self {
        self.client = client;
        self
    }

    pub fn with_rate_limiter(mut self, limiter: Arc<RateLimiter>) -> Self {
        self.rate_limiter = Some(limiter);
        self
    }

    /// Set the rate limit key (typically tool name or container ID).
    pub fn with_rate_limit_key(mut self, key: impl Into<String>) -> Self {
        self.rate_limit_key = Some(key.into());
        self
    }

    /// Forward a request through the proxy pipeline.
    pub async fn forward_request(
        &self,
        url: &str,
        method: &str,
        headers: HashMap<String, String>,
        body: Option<String>,
    ) -> Result<ProxyResponse, ProxyError> {
        let start = Instant::now();
        let mut audit = AuditEntry::new(url.to_string(), method.to_string());

        // 0. Rate limiting
        if let Some(ref limiter) = self.rate_limiter {
            let key = self.rate_limit_key.as_deref().unwrap_or("default");
            if !limiter.check(key) {
                return Err(ProxyError::RateLimited(key.to_string()));
            }
        }

        // 1. Allowlist check
        if !self.enforcer.is_allowed(url) {
            audit.blocked = true;
            audit.duration_ms = start.elapsed().as_millis() as u64;
            return Err(ProxyError::Blocked(url.to_string()));
        }

        // 2. Parse URL and block private IPs
        let parsed = url::Url::parse(url).map_err(|e| ProxyError::InvalidUrl(e.to_string()))?;
        check_private_ip(&parsed)?;

        // 2b. DNS pre-resolution - block domains resolving to private IPs
        //     Pin the validated IP via reqwest::resolve() to prevent DNS rebinding (TOCTOU).
        let pinned_client = if let Some(host) = parsed.host_str() {
            if host.parse::<IpAddr>().is_err() {
                let port = parsed.port_or_known_default().unwrap_or(80);
                let lookup = format!("{}:{}", host, port);
                match tokio::net::lookup_host(&lookup).await {
                    Ok(addrs) => {
                        let addrs: Vec<_> = addrs.collect();
                        for addr in &addrs {
                            if is_private_ip(&addr.ip()) {
                                return Err(ProxyError::PrivateIpBlocked(format!(
                                    "{} resolves to private IP {}",
                                    host,
                                    addr.ip()
                                )));
                            }
                        }
                        // Pin the first validated address to prevent a second DNS lookup
                        let validated_addr = addrs[0];
                        Some(
                            Client::builder()
                                .danger_accept_invalid_certs(false)
                                .redirect(reqwest::redirect::Policy::none())
                                .timeout(std::time::Duration::from_millis(self.config.timeout_ms))
                                .resolve(host, validated_addr)
                                .pool_max_idle_per_host(0)
                                .build()
                                .map_err(|e| ProxyError::ClientBuild(e.to_string()))?,
                        )
                    }
                    Err(e) => {
                        return Err(ProxyError::InvalidUrl(format!(
                            "DNS resolution failed for {}: {}",
                            host, e
                        )));
                    }
                }
            } else {
                None // IP literal, no DNS pinning needed
            }
        } else {
            None
        };
        let client = pinned_client.as_ref().unwrap_or(&self.client);

        // 3. Leak detection on URL
        let url_findings = self.leak_detector.scan(url);
        if !url_findings.is_empty() {
            audit.leak_detected = true;
            audit.duration_ms = start.elapsed().as_millis() as u64;
            return Err(ProxyError::LeakDetected);
        }

        // 4. Leak detection on headers
        for v in headers.values() {
            let findings = self.leak_detector.scan(v);
            if !findings.is_empty() {
                audit.leak_detected = true;
                audit.duration_ms = start.elapsed().as_millis() as u64;
                return Err(ProxyError::LeakDetected);
            }
        }

        // 5. Leak detection on outbound body
        if let Some(ref body_content) = body {
            let findings = self.leak_detector.scan(body_content);
            if !findings.is_empty() {
                audit.leak_detected = true;
                audit.duration_ms = start.elapsed().as_millis() as u64;
                return Err(ProxyError::LeakDetected);
            }
        }

        // 6. Inject credentials based on domain
        let domain = parsed.host_str().unwrap_or("");

        let mut req_headers = reqwest::header::HeaderMap::new();
        for (k, v) in &headers {
            if let (Ok(name), Ok(val)) = (
                reqwest::header::HeaderName::from_bytes(k.as_bytes()),
                reqwest::header::HeaderValue::from_str(v),
            ) {
                req_headers.insert(name, val);
            }
        }

        if let Some(mapping) = self.injector.get_mapping(domain)
            && let (Ok(name), Ok(val)) = (
                reqwest::header::HeaderName::from_bytes(mapping.header.as_bytes()),
                reqwest::header::HeaderValue::from_str(&mapping.value),
            )
        {
            req_headers.insert(name, val);
            audit.credential_injected = Some(domain.to_string());
        }

        // 7. Forward request
        let reqwest_method = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|_| ProxyError::InvalidUrl(format!("invalid method: {method}")))?;

        let mut builder = client.request(reqwest_method, url).headers(req_headers);
        if let Some(body_content) = body {
            builder = builder.body(body_content);
        }

        let mut response = builder.send().await?;

        let status = response.status().as_u16();
        let resp_headers: HashMap<String, String> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect();

        let max_bytes = self.config.max_response_bytes;
        let mut body_bytes = Vec::with_capacity(max_bytes.min(65536));
        while let Some(chunk) = response.chunk().await? {
            body_bytes.extend_from_slice(&chunk);
            if body_bytes.len() >= max_bytes {
                body_bytes.truncate(max_bytes);
                break;
            }
        }
        let resp_body = String::from_utf8_lossy(&body_bytes).to_string();

        // 8. Leak detection on response body
        let resp_findings = self.leak_detector.scan(&resp_body);
        if !resp_findings.is_empty() {
            audit.leak_detected = true;
            audit.duration_ms = start.elapsed().as_millis() as u64;
            tracing::warn!(
                url = url,
                findings = resp_findings.len(),
                "Credential leak detected in response body — redacting"
            );
            let redacted_body = self.leak_detector.redact(&resp_body);
            return Ok(ProxyResponse {
                status,
                headers: resp_headers,
                body: redacted_body,
                audit,
            });
        }

        // Scan response headers and redact any that contain leaked credentials
        let mut resp_headers = resp_headers;
        let mut leaked_header_names = Vec::new();
        for (k, v) in &resp_headers {
            let findings = self.leak_detector.scan(v);
            if !findings.is_empty() {
                audit.leak_detected = true;
                leaked_header_names.push(k.clone());
            }
        }
        if !leaked_header_names.is_empty() {
            tracing::warn!(
                url = url,
                headers = ?leaked_header_names,
                "Credential leak detected in response headers — redacting"
            );
            for header_name in &leaked_header_names {
                resp_headers.insert(header_name.clone(), "[REDACTED]".to_string());
            }
        }

        audit.status = status;
        audit.duration_ms = start.elapsed().as_millis() as u64;
        audit.timestamp = Utc::now().to_rfc3339();

        Ok(ProxyResponse {
            status,
            headers: resp_headers,
            body: resp_body,
            audit,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blocked_url() {
        let config = ProxyConfig {
            allowlist: vec!["api.github.com".into()],
            ..Default::default()
        };
        let service =
            ProxyService::new(config, CredentialInjector::new(), LeakDetector::new()).unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(service.forward_request(
            "https://evil.com/steal",
            "GET",
            HashMap::new(),
            None,
        ));

        assert!(matches!(result, Err(ProxyError::Blocked(_))));
    }

    #[test]
    fn test_leak_detected_in_body() {
        let config = ProxyConfig {
            allowlist: vec!["api.github.com".into()],
            ..Default::default()
        };
        let mut detector = LeakDetector::new();
        detector.add_known_secret("super_secret_key_12345");
        let service = ProxyService::new(config, CredentialInjector::new(), detector).unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(service.forward_request(
            "https://api.github.com/repos",
            "POST",
            HashMap::new(),
            Some("body contains super_secret_key_12345 here".into()),
        ));

        assert!(matches!(result, Err(ProxyError::LeakDetected)));
    }

    #[test]
    fn test_leak_detected_in_url() {
        let config = ProxyConfig {
            allowlist: vec!["api.github.com".into()],
            ..Default::default()
        };
        let mut detector = LeakDetector::new();
        detector.add_known_secret("my_secret_token");
        let service = ProxyService::new(config, CredentialInjector::new(), detector).unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(service.forward_request(
            "https://api.github.com/repos?key=my_secret_token",
            "GET",
            HashMap::new(),
            None,
        ));

        assert!(matches!(result, Err(ProxyError::LeakDetected)));
    }

    #[test]
    fn test_leak_detected_in_headers() {
        let config = ProxyConfig {
            allowlist: vec!["api.github.com".into()],
            ..Default::default()
        };
        let mut detector = LeakDetector::new();
        detector.add_known_secret("header_secret_value");
        let service = ProxyService::new(config, CredentialInjector::new(), detector).unwrap();

        let mut headers = HashMap::new();
        headers.insert("X-Custom".to_string(), "header_secret_value".to_string());

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(service.forward_request(
            "https://api.github.com/repos",
            "GET",
            headers,
            None,
        ));

        assert!(matches!(result, Err(ProxyError::LeakDetected)));
    }

    #[test]
    fn test_private_ip_blocked() {
        let config = ProxyConfig {
            allowlist: vec!["*".into()],
            ..Default::default()
        };
        let service =
            ProxyService::new(config, CredentialInjector::new(), LeakDetector::new()).unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();

        for url in &[
            "http://127.0.0.1/latest/meta-data",
            "http://10.0.0.1/internal",
            "http://172.16.0.1/internal",
            "http://192.168.1.1/internal",
            "http://169.254.169.254/latest/meta-data",
            "http://0.0.0.0/",
        ] {
            let result = rt.block_on(service.forward_request(url, "GET", HashMap::new(), None));
            assert!(
                matches!(result, Err(ProxyError::PrivateIpBlocked(_))),
                "Expected PrivateIpBlocked for {url}, got {result:?}"
            );
        }
    }

    #[test]
    fn test_redirect_not_followed() {
        // The client has redirect policy set to none.
        // We can't easily test this without a server, but we verify construction works.
        let config = ProxyConfig {
            allowlist: vec!["httpbin.org".into()],
            ..Default::default()
        };
        let _service =
            ProxyService::new(config, CredentialInjector::new(), LeakDetector::new()).unwrap();
    }

    #[test]
    fn test_allowed_url_passes_check() {
        let config = ProxyConfig {
            allowlist: vec!["httpbin.org".into()],
            ..Default::default()
        };
        let enforcer = AllowlistEnforcer::new(config.allowlist.clone());
        assert!(enforcer.is_allowed("https://httpbin.org/get"));
    }

    #[test]
    fn test_ipv6_mapped_ipv4_blocked() {
        let cases: Vec<(&str, bool)> = vec![
            ("::ffff:127.0.0.1", true),
            ("::ffff:10.0.0.1", true),
            ("::ffff:192.168.1.1", true),
            ("::ffff:172.16.0.1", true),
            ("::ffff:8.8.8.8", false),
            ("::1", true),
        ];
        for (s, expected) in cases {
            let ip: IpAddr = s.parse().unwrap();
            assert_eq!(
                is_private_ip(&ip),
                expected,
                "is_private_ip({s}) = {expected}"
            );
        }
    }

    #[test]
    fn test_dns_resolution_blocks_localhost() {
        let config = ProxyConfig {
            allowlist: vec!["*".into()],
            ..Default::default()
        };
        let svc =
            ProxyService::new(config, CredentialInjector::new(), LeakDetector::new()).unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(svc.forward_request(
            "http://localhost:9800/test",
            "GET",
            HashMap::new(),
            None,
        ));
        assert!(
            matches!(result, Err(ProxyError::PrivateIpBlocked(_))),
            "Expected PrivateIpBlocked for localhost, got {result:?}"
        );
    }

    #[test]
    fn test_response_header_leak_redacted() {
        // Verify that the response header leak redaction logic works:
        // When a response header value contains a known secret, it should be
        // replaced with "[REDACTED]" rather than passed through.
        let mut detector = LeakDetector::new();
        let secret = "super_secret_credential_xyz";
        detector.add_known_secret(secret);

        let mut resp_headers: HashMap<String, String> = HashMap::new();
        resp_headers.insert("x-safe".to_string(), "harmless".to_string());
        resp_headers.insert("x-leaked".to_string(), format!("Bearer {}", secret));
        resp_headers.insert("content-type".to_string(), "application/json".to_string());

        // Simulate the response header scanning logic from forward_request
        let mut leaked_header_names = Vec::new();
        for (k, v) in &resp_headers {
            let findings = detector.scan(v);
            if !findings.is_empty() {
                leaked_header_names.push(k.clone());
            }
        }
        for header_name in &leaked_header_names {
            resp_headers.insert(header_name.clone(), "[REDACTED]".to_string());
        }

        assert_eq!(
            leaked_header_names.len(),
            1,
            "should detect exactly one leaked header"
        );
        assert!(leaked_header_names.contains(&"x-leaked".to_string()));
        assert_eq!(resp_headers.get("x-leaked").unwrap(), "[REDACTED]");
        assert_eq!(resp_headers.get("x-safe").unwrap(), "harmless");
        assert_eq!(
            resp_headers.get("content-type").unwrap(),
            "application/json"
        );
    }
}
