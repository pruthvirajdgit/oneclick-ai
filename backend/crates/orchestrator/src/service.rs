//! Orchestrator service — agent lifecycle management with locking and persistence.
//!
//! The [`Orchestrator`] coordinates between the [`AgentRuntime`] (container ops)
//! and the database (agent records). It uses per-agent locks via [`DashMap`] to
//! prevent race conditions when concurrent requests target the same agent.

use std::sync::Arc;

use chrono::Utc;
use dashmap::DashMap;
use sqlx::PgPool;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};
use uuid::Uuid;

use oneclick_shared::config::Config;
use oneclick_shared::errors::{AppError, AppResult};
use oneclick_shared::models::agent::{Agent, AgentStatus};

use crate::runtime::AgentRuntime;

/// Maximum number of health-check attempts after waking an agent.
const HEALTH_CHECK_RETRIES: u32 = 5;

/// Delay between health-check attempts.
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// Central service for managing agent lifecycles.
///
/// Wraps a runtime implementation and a database pool. Every mutation
/// on an agent is serialized through a per-agent async mutex stored
/// in a [`DashMap`], so concurrent wake/sleep/destroy calls on the
/// same agent never interleave.
pub struct Orchestrator {
    runtime: Arc<dyn AgentRuntime>,
    db: PgPool,
    /// Per-agent locks keyed by agent UUID.
    locks: DashMap<Uuid, Arc<tokio::sync::Mutex<()>>>,
}

impl Orchestrator {
    /// Create a new orchestrator with the given runtime and database pool.
    pub fn new(runtime: Arc<dyn AgentRuntime>, db: PgPool) -> Self {
        Self {
            runtime,
            db,
            locks: DashMap::new(),
        }
    }

    /// Acquire (or create) the per-agent lock.
    fn agent_lock(&self, agent_id: Uuid) -> Arc<tokio::sync::Mutex<()>> {
        self.locks
            .entry(agent_id)
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    // ---------------------------------------------------------------
    // Public API
    // ---------------------------------------------------------------

    /// Provision a new agent for the given user.
    ///
    /// 1. Checks capacity (total agent count vs `config.max_agents`).
    /// 2. Inserts a `Creating` record in the database.
    /// 3. Creates the container via the runtime.
    /// 4. Updates the record with the container ID and sets status to `Stopped`.
    ///
    /// Returns the fully-populated [`Agent`] row.
    pub async fn create_agent(
        &self,
        user_id: Uuid,
        model: &str,
        config: &Config,
    ) -> AppResult<Agent> {
        // --- capacity check ---
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM agents")
            .fetch_one(&self.db)
            .await?;

        if count >= config.max_agents as i64 {
            warn!(
                current = count,
                max = config.max_agents,
                "Agent capacity reached"
            );
            return Err(AppError::CapacityReached);
        }

        let container_name = format!("agent-{}", &user_id.to_string()[..8]);

        // --- insert DB record ---
        let agent: Agent = sqlx::query_as::<_, Agent>(
            "INSERT INTO agents (user_id, container_name, status, model) \
             VALUES ($1, $2, $3, $4) RETURNING *",
        )
        .bind(user_id)
        .bind(&container_name)
        .bind(AgentStatus::Creating)
        .bind(model)
        .fetch_one(&self.db)
        .await?;

        info!(agent_id = %agent.id, user_id = %user_id, "Agent record created");

        // --- create container ---
        let container_id = match self.runtime.create_agent(&agent, config).await {
            Ok(id) => id,
            Err(e) => {
                // Mark the agent as errored so it doesn't block future attempts.
                let _ = self.update_status(agent.id, AgentStatus::Error).await;
                return Err(e);
            }
        };

        // --- persist container_id and mark Stopped (not started yet) ---
        let agent: Agent = sqlx::query_as::<_, Agent>(
            "UPDATE agents SET container_id = $1, status = $2, updated_at = NOW() \
             WHERE id = $3 RETURNING *",
        )
        .bind(&container_id)
        .bind(AgentStatus::Stopped)
        .bind(agent.id)
        .fetch_one(&self.db)
        .await?;

        info!(agent_id = %agent.id, container_id = %container_id, "Agent provisioned");
        Ok(agent)
    }

    /// Wake a stopped agent: start the container, health-check, update DB.
    ///
    /// Acquires the per-agent lock so concurrent wake calls are serialized.
    pub async fn wake_agent(&self, agent_id: Uuid) -> AppResult<Agent> {
        let lock = self.agent_lock(agent_id);
        let _guard = lock.lock().await;

        let agent = self.get_agent(agent_id).await?;

        if agent.status == AgentStatus::Running {
            info!(agent_id = %agent_id, "Agent already running");
            return Ok(agent);
        }

        let container_id = agent
            .container_id
            .as_deref()
            .ok_or_else(|| AppError::Internal("Agent has no container ID".into()))?;

        // Start the container
        self.runtime.start_agent(container_id).await?;

        // Health-check with retries
        let healthy = self.poll_health(container_id).await;

        if !healthy {
            error!(agent_id = %agent_id, "Agent failed health check after wake");
            self.update_status(agent_id, AgentStatus::Error).await?;
            return Err(AppError::AgentUnavailable(
                "Agent did not become healthy after start".into(),
            ));
        }

        // Update DB: running + last_active
        let agent: Agent = sqlx::query_as::<_, Agent>(
            "UPDATE agents SET status = $1, last_active = $2, updated_at = NOW() \
             WHERE id = $3 RETURNING *",
        )
        .bind(AgentStatus::Running)
        .bind(Utc::now())
        .bind(agent_id)
        .fetch_one(&self.db)
        .await?;

        info!(agent_id = %agent_id, "Agent woken and healthy");
        Ok(agent)
    }

    /// Stop an agent container and mark it as `Stopped`.
    ///
    /// Acquires the per-agent lock.
    pub async fn sleep_agent(&self, agent_id: Uuid) -> AppResult<Agent> {
        let lock = self.agent_lock(agent_id);
        let _guard = lock.lock().await;

        let agent = self.get_agent(agent_id).await?;

        if agent.status == AgentStatus::Stopped {
            info!(agent_id = %agent_id, "Agent already stopped");
            return Ok(agent);
        }

        let container_id = agent
            .container_id
            .as_deref()
            .ok_or_else(|| AppError::Internal("Agent has no container ID".into()))?;

        self.runtime.stop_agent(container_id).await?;
        let agent = self.update_status(agent_id, AgentStatus::Stopped).await?;

        info!(agent_id = %agent_id, "Agent put to sleep");
        Ok(agent)
    }

    /// Permanently remove an agent: destroy container and delete DB record.
    ///
    /// Acquires the per-agent lock and removes it after cleanup.
    pub async fn destroy_agent(&self, agent_id: Uuid) -> AppResult<()> {
        let lock = self.agent_lock(agent_id);
        let _guard = lock.lock().await;

        let agent = self.get_agent(agent_id).await?;

        // Destroy the container if one exists
        if let Some(container_id) = &agent.container_id {
            if let Err(e) = self.runtime.destroy_agent(container_id).await {
                warn!(
                    agent_id = %agent_id,
                    error = %e,
                    "Container removal failed, proceeding with DB cleanup"
                );
            }
        }

        // Delete DB record
        sqlx::query("DELETE FROM agents WHERE id = $1")
            .bind(agent_id)
            .execute(&self.db)
            .await?;

        // Drop the per-agent lock entry
        drop(_guard);
        self.locks.remove(&agent_id);

        info!(agent_id = %agent_id, "Agent destroyed");
        Ok(())
    }

    /// Fetch the current agent status from the database.
    pub async fn get_agent_status(&self, agent_id: Uuid) -> AppResult<AgentStatus> {
        let agent = self.get_agent(agent_id).await?;
        Ok(agent.status)
    }

    /// Ensure the agent is ready to serve requests.
    ///
    /// - `Running` -> return immediately.
    /// - `Stopped` -> wake the agent and return.
    /// - `Creating` -> return an unavailable error (caller should retry).
    /// - `Error` -> return an error.
    pub async fn ensure_ready(&self, agent_id: Uuid) -> AppResult<Agent> {
        let agent = self.get_agent(agent_id).await?;

        match agent.status {
            AgentStatus::Running => Ok(agent),
            AgentStatus::Stopped => self.wake_agent(agent_id).await,
            AgentStatus::Creating => Err(AppError::AgentUnavailable(
                "Agent is still being created".into(),
            )),
            AgentStatus::Error => Err(AppError::AgentUnavailable(
                "Agent is in an error state".into(),
            )),
        }
    }

    // ---------------------------------------------------------------
    // Internal helpers
    // ---------------------------------------------------------------

    /// Fetch an agent by ID or return `NotFound`.
    async fn get_agent(&self, agent_id: Uuid) -> AppResult<Agent> {
        sqlx::query_as::<_, Agent>("SELECT * FROM agents WHERE id = $1")
            .bind(agent_id)
            .fetch_optional(&self.db)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("Agent {agent_id} not found")))
    }

    /// Update an agent's status and return the updated row.
    async fn update_status(
        &self,
        agent_id: Uuid,
        status: AgentStatus,
    ) -> AppResult<Agent> {
        let agent: Agent = sqlx::query_as::<_, Agent>(
            "UPDATE agents SET status = $1, updated_at = NOW() WHERE id = $2 RETURNING *",
        )
        .bind(status)
        .bind(agent_id)
        .fetch_one(&self.db)
        .await?;

        Ok(agent)
    }

    /// Poll the runtime health check with retries.
    async fn poll_health(&self, container_id: &str) -> bool {
        for attempt in 1..=HEALTH_CHECK_RETRIES {
            match self.runtime.health_check(container_id).await {
                Ok(true) => {
                    info!(container_id, attempt, "Health check passed");
                    return true;
                }
                Ok(false) => {
                    warn!(container_id, attempt, "Health check returned unhealthy");
                }
                Err(e) => {
                    warn!(container_id, attempt, error = %e, "Health check error");
                }
            }
            if attempt < HEALTH_CHECK_RETRIES {
                sleep(HEALTH_CHECK_INTERVAL).await;
            }
        }
        false
    }
}
