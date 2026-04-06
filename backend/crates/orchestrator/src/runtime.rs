//! Agent runtime abstraction and Docker implementation.
//!
//! Defines the [`AgentRuntime`] trait for managing agent container lifecycles,
//! and provides [`DockerRuntime`] as the Phase 1 implementation using bollard.

use std::collections::HashMap;

use async_trait::async_trait;
use bollard::container::{
    Config as ContainerConfig, CreateContainerOptions, RemoveContainerOptions,
    StartContainerOptions, StopContainerOptions,
};
use bollard::container::NetworkingConfig;
use bollard::models::{
    EndpointSettings, HostConfig, RestartPolicy, RestartPolicyNameEnum,
};
use bollard::Docker;
use oneclick_shared::config::Config;
use oneclick_shared::errors::{AppError, AppResult};
use oneclick_shared::models::agent::Agent;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Core abstraction for agent container runtimes.
///
/// Phase 1 uses [`DockerRuntime`]. Future phases will add `CriuRuntime`
/// (checkpoint/restore) and `FirecrackerRuntime` (microVM isolation).
#[async_trait]
pub trait AgentRuntime: Send + Sync {
    /// Create a new agent container and return the container ID.
    async fn create_agent(&self, agent: &Agent, config: &Config) -> AppResult<String>;

    /// Start a stopped agent container.
    async fn start_agent(&self, container_id: &str) -> AppResult<()>;

    /// Stop a running agent container.
    async fn stop_agent(&self, container_id: &str) -> AppResult<()>;

    /// Remove an agent container and its volumes.
    async fn destroy_agent(&self, container_id: &str) -> AppResult<()>;

    /// Check if the agent's HTTP endpoint is healthy.
    ///
    /// Returns `true` if the agent responded successfully, `false` otherwise.
    async fn health_check(&self, container_id: &str) -> AppResult<bool>;
}

/// Docker-based agent runtime using the bollard client.
///
/// Communicates with the Docker daemon via the local socket to create,
/// start, stop, and remove agent containers.
pub struct DockerRuntime {
    docker: Docker,
}

impl DockerRuntime {
    /// Connect to the local Docker daemon.
    pub fn new() -> AppResult<Self> {
        let docker = Docker::connect_with_local_defaults().map_err(|e| {
            error!("Failed to connect to Docker daemon: {e}");
            AppError::Internal(format!("Docker connection failed: {e}"))
        })?;
        info!("Connected to Docker daemon");
        Ok(Self { docker })
    }

    /// Build the container name from user ID and agent ID to avoid collisions.
    ///
    /// This is a utility used by the orchestrator service when inserting a new
    /// Agent row. The `create_agent` impl reads back from `agent.container_name`.
    pub fn container_name(user_id: &Uuid, agent_id: &Uuid) -> String {
        let user_short = &user_id.to_string()[..8];
        let agent_short = &agent_id.to_string()[..8];
        format!("agent-{user_short}-{agent_short}")
    }
}

/// Parse a human-readable memory string (e.g. "512m", "1g") into bytes.
///
/// Supports suffixes: `k`/`K` (KiB), `m`/`M` (MiB), `g`/`G` (GiB).
/// Returns an error for invalid formats.
fn parse_memory_limit(limit: &str) -> AppResult<i64> {
    let limit = limit.trim();
    if limit.is_empty() {
        return Err(AppError::Internal("Empty memory limit".into()));
    }

    let (num_str, multiplier) = match limit.as_bytes().last() {
        Some(b'k' | b'K') => (&limit[..limit.len() - 1], 1024i64),
        Some(b'm' | b'M') => (&limit[..limit.len() - 1], 1024i64 * 1024),
        Some(b'g' | b'G') => (&limit[..limit.len() - 1], 1024i64 * 1024 * 1024),
        _ => (limit, 1i64),
    };

    let value: i64 = num_str.parse().map_err(|_| {
        AppError::Internal(format!("Invalid memory limit format: {limit}"))
    })?;

    Ok(value * multiplier)
}

/// Convert a fractional CPU limit (e.g. 0.5) to Docker nano-CPUs.
fn cpu_to_nano(cpu: f64) -> i64 {
    (cpu * 1_000_000_000.0) as i64
}

#[async_trait]
impl AgentRuntime for DockerRuntime {
    async fn create_agent(&self, agent: &Agent, config: &Config) -> AppResult<String> {
        // Use the container name stored on the Agent row (set by the orchestrator)
        // to maintain a single source of truth.
        let name = agent
            .container_name
            .as_deref()
            .ok_or_else(|| AppError::Internal("Agent has no container_name set".into()))?;
        let memory = parse_memory_limit(&config.agent_memory_limit)?;
        let nano_cpus = cpu_to_nano(config.agent_cpu_limit);

        info!(
            agent_id = %agent.id,
            user_id = %agent.user_id,
            container_name = %name,
            "Creating agent container"
        );

        // Ensure model uses openrouter/ prefix so OpenClaw routes through our proxy.
        // Without this, OpenClaw tries to call providers (groq, anthropic) directly.
        let proxy_model = if agent.model.starts_with("openrouter/") {
            agent.model.clone()
        } else {
            format!("openrouter/{}", agent.model)
        };

        let env = vec![
            // Route LLM traffic through backend proxy (not directly to providers)
            "OPENROUTER_BASE_URL=http://backend:8080/internal/llm/v1".to_string(),
            format!("DEFAULT_MODEL={}", proxy_model),
            format!("AGENT_MODEL={}", proxy_model),
            format!("AGENT_NAME=agent-{}", &agent.id.to_string()[..8]),
            "AGENT_PORT=3000".to_string(),
            "OPENCLAW_GATEWAY_TOKEN=oneclick-internal".to_string(),
            // Constrain Node.js heap to avoid OOM during GC spikes
            "NODE_OPTIONS=--max-old-space-size=1280".to_string(),
            // Encode internal auth in the API key so OpenClaw sends it as
            // Authorization: Bearer header. Format: secret|agent_id|user_id
            format!(
                "OPENROUTER_API_KEY={}|{}|{}",
                config.internal_secret, agent.id, agent.user_id
            ),
            // OneClick agent tools plugin env vars — used by oneclick-tools.js
            // to call backend internal APIs (schedules, notifications)
            "ONECLICK_BACKEND_URL=http://backend:8080".to_string(),
            format!("ONECLICK_AGENT_ID={}", agent.id),
            format!("ONECLICK_USER_ID={}", agent.user_id),
            format!("ONECLICK_INTERNAL_SECRET={}", config.internal_secret),
        ];

        let mut labels = HashMap::new();
        labels.insert("oneclick.agent_id".to_string(), agent.id.to_string());
        labels.insert("oneclick.user_id".to_string(), agent.user_id.to_string());

        let mut endpoints = HashMap::new();
        endpoints.insert(
            config.docker_network.clone(),
            EndpointSettings {
                ..Default::default()
            },
        );

        let host_config = HostConfig {
            memory: Some(memory),
            nano_cpus: Some(nano_cpus),
            restart_policy: Some(RestartPolicy {
                name: Some(RestartPolicyNameEnum::NO),
                maximum_retry_count: None,
            }),
            // Bind a named volume for OpenClaw state persistence across stop/start
            binds: Some(vec![
                format!("oneclick-agent-{name}:/home/node/.openclaw"),
            ]),
            ..Default::default()
        };

        let container_config = ContainerConfig {
            image: Some(config.agent_image.clone()),
            env: Some(env),
            labels: Some(labels),
            host_config: Some(host_config),
            networking_config: Some(NetworkingConfig {
                endpoints_config: endpoints,
            }),
            // OpenClaw requires a TTY to run the gateway process
            tty: Some(true),
            ..Default::default()
        };

        let options = CreateContainerOptions {
            name: name.clone(),
            platform: None,
        };

        let response = self
            .docker
            .create_container(Some(options), container_config)
            .await
            .map_err(|e| {
                error!(agent_id = %agent.id, error = %e, "Failed to create container");
                AppError::Internal(format!("Container creation failed: {e}"))
            })?;

        info!(
            agent_id = %agent.id,
            container_id = %response.id,
            "Agent container created"
        );

        Ok(response.id)
    }

    async fn start_agent(&self, container_id: &str) -> AppResult<()> {
        info!(container_id, "Starting agent container");

        self.docker
            .start_container(container_id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|e| {
                error!(container_id, error = %e, "Failed to start container");
                AppError::AgentUnavailable(format!("Container start failed: {e}"))
            })?;

        info!(container_id, "Agent container started");
        Ok(())
    }

    async fn stop_agent(&self, container_id: &str) -> AppResult<()> {
        info!(container_id, "Stopping agent container");

        let options = StopContainerOptions { t: 10 };

        match self.docker.stop_container(container_id, Some(options)).await {
            Ok(()) => {
                info!(container_id, "Agent container stopped");
                Ok(())
            }
            Err(e) if e.to_string().contains("is not running") => {
                warn!(container_id, "Container already stopped");
                Ok(())
            }
            Err(e) => {
                error!(container_id, error = %e, "Failed to stop container");
                Err(AppError::AgentUnavailable(format!("Container stop failed: {e}")))
            }
        }
    }

    async fn destroy_agent(&self, container_id: &str) -> AppResult<()> {
        info!(container_id, "Destroying agent container");

        // Inspect container to learn its name so we can remove the named volume.
        let container_name = match self.docker.inspect_container(container_id, None).await {
            Ok(info) => info
                .name
                .map(|n| n.trim_start_matches('/').to_string()),
            Err(e) => {
                warn!(container_id, error = %e, "Failed to inspect container before removal");
                None
            }
        };

        let options = RemoveContainerOptions {
            force: true,
            v: true, // remove anonymous volumes
            ..Default::default()
        };

        self.docker
            .remove_container(container_id, Some(options))
            .await
            .map_err(|e| {
                error!(container_id, error = %e, "Failed to remove container");
                AppError::Internal(format!("Container removal failed: {e}"))
            })?;

        // Named volumes are not removed by `v: true` — clean up explicitly.
        if let Some(name) = container_name {
            let volume_name = format!("oneclick-agent-{name}");
            if let Err(e) = self.docker.remove_volume(&volume_name, None).await {
                warn!(
                    container_id,
                    volume = %volume_name,
                    error = %e,
                    "Failed to remove named volume (non-fatal)"
                );
            } else {
                info!(container_id, volume = %volume_name, "Named volume removed");
            }
        }

        info!(container_id, "Agent container destroyed");
        Ok(())
    }

    async fn health_check(&self, container_id: &str) -> AppResult<bool> {
        let inspect = self
            .docker
            .inspect_container(container_id, None)
            .await
            .map_err(|e| {
                error!(container_id, error = %e, "Failed to inspect container for health check");
                AppError::AgentUnavailable(format!("Container inspect failed: {e}"))
            })?;

        // Check container state — if it's not running, it's not healthy
        let running = inspect
            .state
            .as_ref()
            .and_then(|s| s.running)
            .unwrap_or(false);

        if !running {
            warn!(container_id, "Container is not running during health check");
            return Ok(false);
        }

        // If Docker health check is configured, use its status
        if let Some(health) = inspect.state.as_ref().and_then(|s| s.health.as_ref()) {
            if let Some(status) = &health.status {
                let status_str: &str = status.as_ref();
                let healthy = status_str == "healthy";
                if status_str != "starting" {
                    info!(container_id, status = status_str, "Docker health status");
                }
                return Ok(healthy);
            }
        }

        // Fallback: no Docker HEALTHCHECK configured.
        // Warn loudly — production agent images should define HEALTHCHECK.
        warn!(
            container_id,
            "Container running but no HEALTHCHECK configured — assuming healthy. \
             Define a HEALTHCHECK in the agent Dockerfile for reliable readiness detection."
        );
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_memory_limit_megabytes() {
        assert_eq!(parse_memory_limit("512m").unwrap(), 536_870_912);
        assert_eq!(parse_memory_limit("512M").unwrap(), 536_870_912);
    }

    #[test]
    fn test_parse_memory_limit_gigabytes() {
        assert_eq!(parse_memory_limit("1g").unwrap(), 1_073_741_824);
        assert_eq!(parse_memory_limit("2G").unwrap(), 2_147_483_648);
    }

    #[test]
    fn test_parse_memory_limit_kilobytes() {
        assert_eq!(parse_memory_limit("1024k").unwrap(), 1_048_576);
    }

    #[test]
    fn test_parse_memory_limit_bytes() {
        assert_eq!(parse_memory_limit("536870912").unwrap(), 536_870_912);
    }

    #[test]
    fn test_parse_memory_limit_invalid() {
        assert!(parse_memory_limit("abc").is_err());
        assert!(parse_memory_limit("").is_err());
    }

    #[test]
    fn test_cpu_to_nano() {
        assert_eq!(cpu_to_nano(0.5), 500_000_000);
        assert_eq!(cpu_to_nano(1.0), 1_000_000_000);
        assert_eq!(cpu_to_nano(0.25), 250_000_000);
    }
}
