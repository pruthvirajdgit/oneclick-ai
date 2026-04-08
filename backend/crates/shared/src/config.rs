use serde::Deserialize;

/// Application configuration loaded from environment variables.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// PostgreSQL connection string
    pub database_url: String,

    /// Redis connection string
    pub redis_url: String,

    /// Secret key for JWT signing
    pub jwt_secret: String,

    /// JWT token expiry in hours
    #[serde(default = "default_jwt_expiry")]
    pub jwt_expiry_hours: u64,

    /// Groq API key for LLM inference
    pub groq_api_key: String,

    /// OpenRouter API key (fallback provider)
    pub openrouter_api_key: String,

    /// Docker image for agent containers
    #[serde(default = "default_agent_image")]
    pub agent_image: String,

    /// Memory limit per agent container (e.g., "512m")
    #[serde(default = "default_agent_memory")]
    pub agent_memory_limit: String,

    /// CPU limit per agent container (e.g., 0.5)
    #[serde(default = "default_agent_cpu")]
    pub agent_cpu_limit: f64,

    /// Maximum number of agents across all users
    #[serde(default = "default_max_agents")]
    pub max_agents: u32,

    /// Daily request limit for free tier users
    #[serde(default = "default_daily_limit")]
    pub free_tier_daily_limit: u32,

    /// Minutes of inactivity before an agent is stopped
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout_minutes: u32,

    /// Docker network name for agent containers
    #[serde(default = "default_docker_network")]
    pub docker_network: String,

    /// Shared secret for internal agent→backend calls (required, no default)
    pub internal_secret: String,

    /// Comma-separated list of allowed CORS origins (e.g. "http://localhost:3000,https://app.oneclick.ai").
    /// Use "*" to allow any origin (dev only).
    #[serde(default = "default_cors_origins")]
    pub cors_allowed_origins: String,

    // ── Firecracker runtime config ──────────────────────────────────────

    /// Which agent runtime to use: "docker" or "firecracker"
    #[serde(default = "default_agent_runtime")]
    pub agent_runtime: String,

    /// Path to the Firecracker-compatible kernel image
    #[serde(default = "default_fc_kernel_path")]
    pub fc_kernel_path: String,

    /// Path to the template rootfs image (copied per-VM)
    #[serde(default = "default_fc_rootfs_template")]
    pub fc_rootfs_template: String,

    /// Directory for VM snapshot storage
    #[serde(default = "default_fc_snapshot_dir")]
    pub fc_snapshot_dir: String,

    /// Directory for per-VM rootfs copies and state
    #[serde(default = "default_fc_vm_dir")]
    pub fc_vm_dir: String,

    /// Number of vCPUs per Firecracker VM
    #[serde(default = "default_fc_vcpu_count")]
    pub fc_vcpu_count: u32,

    /// Memory size in MiB per Firecracker VM
    #[serde(default = "default_fc_mem_size_mib")]
    pub fc_mem_size_mib: u32,

    /// TAP device name prefix (e.g. "tap" → tap0, tap1, ...)
    #[serde(default = "default_fc_tap_prefix")]
    pub fc_tap_prefix: String,

    /// Number of TAP devices in the pool
    #[serde(default = "default_fc_tap_count")]
    pub fc_tap_count: u32,

    /// Subnet prefix for TAP networking (e.g. "172.16")
    #[serde(default = "default_fc_subnet_prefix")]
    pub fc_subnet_prefix: String,
}

impl Config {
    /// Load configuration from environment variables.
    ///
    /// Reads `.env` file if present, then loads all required env vars.
    pub fn from_env() -> anyhow::Result<Self> {
        dotenvy::dotenv().ok();

        let config = Self {
            database_url: env_required("DATABASE_URL")?,
            redis_url: env_or("REDIS_URL", "redis://127.0.0.1:6379"),
            jwt_secret: env_required("JWT_SECRET")?,
            jwt_expiry_hours: env_parse("JWT_EXPIRY_HOURS", default_jwt_expiry()),
            groq_api_key: env_or("GROQ_API_KEY", ""),
            openrouter_api_key: env_or("OPENROUTER_API_KEY", ""),
            agent_image: env_or("AGENT_IMAGE", &default_agent_image()),
            agent_memory_limit: env_or("AGENT_MEMORY_LIMIT", &default_agent_memory()),
            agent_cpu_limit: env_parse("AGENT_CPU_LIMIT", default_agent_cpu()),
            max_agents: env_parse("MAX_AGENTS", default_max_agents()),
            free_tier_daily_limit: env_parse("FREE_TIER_DAILY_LIMIT", default_daily_limit()),
            idle_timeout_minutes: env_parse("IDLE_TIMEOUT_MINUTES", default_idle_timeout()),
            docker_network: env_or("DOCKER_NETWORK", &default_docker_network()),
            internal_secret: env_required("INTERNAL_SECRET")?,
            cors_allowed_origins: env_or("CORS_ALLOWED_ORIGINS", &default_cors_origins()),
            agent_runtime: env_or("AGENT_RUNTIME", &default_agent_runtime()),
            fc_kernel_path: env_or("FC_KERNEL_PATH", &default_fc_kernel_path()),
            fc_rootfs_template: env_or("FC_ROOTFS_TEMPLATE", &default_fc_rootfs_template()),
            fc_snapshot_dir: env_or("FC_SNAPSHOT_DIR", &default_fc_snapshot_dir()),
            fc_vm_dir: env_or("FC_VM_DIR", &default_fc_vm_dir()),
            fc_vcpu_count: env_parse("FC_VCPU_COUNT", default_fc_vcpu_count()),
            fc_mem_size_mib: env_parse("FC_MEM_SIZE_MIB", default_fc_mem_size_mib()),
            fc_tap_prefix: env_or("FC_TAP_PREFIX", &default_fc_tap_prefix()),
            fc_tap_count: env_parse("FC_TAP_COUNT", default_fc_tap_count()),
            fc_subnet_prefix: env_or("FC_SUBNET_PREFIX", &default_fc_subnet_prefix()),
        };

        // Validate at least one LLM provider key is set.
        if config.groq_api_key.is_empty() && config.openrouter_api_key.is_empty() {
            anyhow::bail!(
                "At least one LLM provider key must be set (GROQ_API_KEY or OPENROUTER_API_KEY)"
            );
        }

        // Validate Firecracker-specific paths when that runtime is selected.
        if config.agent_runtime == "firecracker" {
            if !std::path::Path::new(&config.fc_kernel_path).exists() {
                anyhow::bail!(
                    "FC_KERNEL_PATH does not exist: {}",
                    config.fc_kernel_path
                );
            }
            if !std::path::Path::new(&config.fc_rootfs_template).exists() {
                anyhow::bail!(
                    "FC_ROOTFS_TEMPLATE does not exist: {}",
                    config.fc_rootfs_template
                );
            }
            if config.fc_vcpu_count == 0 || config.fc_vcpu_count > 255 {
                anyhow::bail!(
                    "FC_VCPU_COUNT must be 1–255, got {}",
                    config.fc_vcpu_count
                );
            }
            if config.fc_tap_count == 0 || config.fc_tap_count > 64 {
                anyhow::bail!(
                    "FC_TAP_COUNT must be 1–64, got {}",
                    config.fc_tap_count
                );
            }
        }

        Ok(config)
    }
}

fn env_required(key: &str) -> anyhow::Result<String> {
    std::env::var(key).map_err(|_| anyhow::anyhow!("Missing required env var: {}", key))
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn default_jwt_expiry() -> u64 { 24 }
fn default_agent_image() -> String { "oneclick-agent:latest".into() }
fn default_agent_memory() -> String { "512m".into() }
fn default_agent_cpu() -> f64 { 0.5 }
fn default_max_agents() -> u32 { 100 }
fn default_daily_limit() -> u32 { 50 }
fn default_idle_timeout() -> u32 { 15 }
fn default_docker_network() -> String { "oneclick-net".into() }
fn default_cors_origins() -> String { "*".into() }
fn default_agent_runtime() -> String { "docker".into() }
fn default_fc_kernel_path() -> String { "/opt/firecracker/vmlinux-6.1".into() }
fn default_fc_rootfs_template() -> String { "/opt/firecracker/rootfs-openclaw.ext4".into() }
fn default_fc_snapshot_dir() -> String { "/var/lib/oneclick/snapshots".into() }
fn default_fc_vm_dir() -> String { "/var/lib/oneclick/vms".into() }
fn default_fc_vcpu_count() -> u32 { 2 }
fn default_fc_mem_size_mib() -> u32 { 1536 }
fn default_fc_tap_prefix() -> String { "tap".into() }
fn default_fc_tap_count() -> u32 { 16 }
fn default_fc_subnet_prefix() -> String { "172.16".into() }
