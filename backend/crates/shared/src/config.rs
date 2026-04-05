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
        };

        // Validate at least one LLM provider key is set.
        if config.groq_api_key.is_empty() && config.openrouter_api_key.is_empty() {
            anyhow::bail!(
                "At least one LLM provider key must be set (GROQ_API_KEY or OPENROUTER_API_KEY)"
            );
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
