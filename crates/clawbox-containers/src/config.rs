//! Container configuration — security settings and defaults.

/// Default Docker image for agent containers.
pub const DEFAULT_AGENT_IMAGE: &str = "ghcr.io/n0xmare/clawbox-agent:latest";

/// Default container memory limit (MB).
pub const DEFAULT_MEMORY_MB: u64 = 512;

/// Default container CPU shares.
pub const DEFAULT_CPU_SHARES: u32 = 1024;

/// Default execution timeout (ms).
pub const DEFAULT_TIMEOUT_MS: u64 = 120_000;

/// Default allowed image prefixes for container spawning.
pub const DEFAULT_ALLOWED_IMAGE_PREFIXES: &[&str] =
    &["ghcr.io/n0xmare/", "alpine:", "ubuntu:", "debian:"];

/// Security configuration for sandboxed containers.
#[derive(Debug, Clone)]
#[non_exhaustive]
#[must_use]
pub struct ContainerSecurityConfig {
    /// User to run as inside the container (uid:gid).
    pub user: String,
    /// Mount the root filesystem as read-only.
    pub readonly_rootfs: bool,
    /// Drop all Linux capabilities.
    pub drop_all_caps: bool,
    /// Prevent gaining new privileges via setuid/setgid.
    pub no_new_privileges: bool,
    /// Tmpfs mounts (writable scratch space).
    pub tmpfs_mounts: Vec<String>,
    /// Allowed image prefixes for container spawning.
    pub allowed_image_prefixes: Vec<String>,
}

impl ContainerSecurityConfig {
    /// Create a new security config with all hardening enabled.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for ContainerSecurityConfig {
    fn default() -> Self {
        Self {
            user: "1000:1000".into(),
            readonly_rootfs: true,
            drop_all_caps: true,
            no_new_privileges: true,
            tmpfs_mounts: vec!["/tmp:rw,noexec,size=256m".into()],
            allowed_image_prefixes: DEFAULT_ALLOWED_IMAGE_PREFIXES
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }
}
