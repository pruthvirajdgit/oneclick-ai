use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use oneclick_orchestrator::Orchestrator;
use oneclick_shared::models::agent::Agent;
use sqlx::PgPool;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// How far ahead to look for upcoming scheduled jobs before deciding an agent
/// can safely be put to sleep.
const UPCOMING_JOB_WINDOW_MINUTES: i64 = 20;

/// Background service that detects idle agents and puts them to sleep.
///
/// The monitor runs on a fixed interval (`scan_interval`, default 5 min),
/// queries for all running agents whose `last_active` exceeds the configured
/// timeout, and then — if no upcoming work is scheduled — delegates to the
/// [`Orchestrator`] to stop each agent's container.
pub struct IdleMonitor {
    db: PgPool,
    orchestrator: Arc<Orchestrator>,
    /// How often the scan loop runs (default: 5 minutes).
    scan_interval: Duration,
    /// How long an agent may be idle before it becomes eligible for sleep.
    idle_timeout: Duration,
}

impl IdleMonitor {
    /// Create a new `IdleMonitor`.
    ///
    /// `idle_timeout_minutes` is typically sourced from
    /// [`Config::idle_timeout_minutes`](oneclick_shared::config::Config::idle_timeout_minutes).
    pub fn new(
        db: PgPool,
        orchestrator: Arc<Orchestrator>,
        idle_timeout_minutes: u32,
    ) -> Self {
        Self {
            db,
            orchestrator,
            scan_interval: Duration::from_secs(5 * 60),
            idle_timeout: Duration::from_secs(u64::from(idle_timeout_minutes) * 60),
        }
    }

    /// Start the monitor loop. Runs until the task is cancelled.
    pub async fn run(&self) -> anyhow::Result<()> {
        info!(
            scan_interval_secs = self.scan_interval.as_secs(),
            idle_timeout_secs = self.idle_timeout.as_secs(),
            "Idle monitor started",
        );

        loop {
            if let Err(e) = self.scan().await {
                error!(error = %e, "Idle monitor scan failed");
            }
            tokio::time::sleep(self.scan_interval).await;
        }
    }

    // -----------------------------------------------------------------------
    // Scan
    // -----------------------------------------------------------------------

    /// Run a single scan: find idle agents, filter by upcoming work, and sleep
    /// those that qualify.
    async fn scan(&self) -> anyhow::Result<()> {
        let idle_agents = self.find_idle_agents().await?;

        // Publish the current running-agent gauge regardless of idle count.
        let running_count = self.count_running_agents().await?;
        metrics::gauge!("agents_running").set(running_count as f64);

        if idle_agents.is_empty() {
            debug!(running = running_count, "No idle agents found");
            return Ok(());
        }

        debug!(
            idle_candidates = idle_agents.len(),
            running = running_count,
            "Idle agent scan complete",
        );

        let mut stopped_count: u64 = 0;

        for agent in &idle_agents {
            match self.should_sleep(agent.id).await {
                Ok(true) => {
                    if let Err(e) = self.orchestrator.sleep_agent(agent.id).await {
                        warn!(
                            agent_id = %agent.id,
                            error = %e,
                            "Failed to sleep idle agent",
                        );
                    } else {
                        info!(
                            agent_id = %agent.id,
                            user_id = %agent.user_id,
                            "Agent put to sleep due to inactivity",
                        );
                        stopped_count += 1;
                    }
                }
                Ok(false) => {
                    debug!(
                        agent_id = %agent.id,
                        "Skipping idle agent — upcoming work detected",
                    );
                }
                Err(e) => {
                    warn!(
                        agent_id = %agent.id,
                        error = %e,
                        "Error checking agent eligibility; skipping",
                    );
                }
            }
        }

        if stopped_count > 0 {
            metrics::counter!("agents_stopped_idle").increment(stopped_count);
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Return all running agents that have been idle longer than
    /// `idle_timeout`.
    async fn find_idle_agents(&self) -> anyhow::Result<Vec<Agent>> {
        let timeout_minutes = (self.idle_timeout.as_secs() / 60) as i64;

        let agents: Vec<Agent> = sqlx::query_as(
            "SELECT * FROM agents \
             WHERE status = 'running' \
             AND (last_active IS NULL \
                  OR last_active < NOW() - make_interval(mins => $1))",
        )
        .bind(timeout_minutes)
        .fetch_all(&self.db)
        .await
        .context("Failed to query idle agents")?;

        Ok(agents)
    }

    /// Count all agents currently in `running` status (used for the gauge
    /// metric).
    async fn count_running_agents(&self) -> anyhow::Result<i64> {
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM agents WHERE status = 'running'")
                .fetch_one(&self.db)
                .await
                .context("Failed to count running agents")?;

        Ok(count)
    }

    /// Determine whether the given agent is safe to sleep.
    ///
    /// Returns `false` if:
    /// - A scheduled job is due within the next 20 minutes, **or**
    /// - There are pending messages in the queue.
    async fn should_sleep(&self, agent_id: Uuid) -> anyhow::Result<bool> {
        if self.has_upcoming_jobs(agent_id).await? {
            return Ok(false);
        }
        if self.has_pending_messages(agent_id).await? {
            return Ok(false);
        }
        Ok(true)
    }

    /// Check if the agent has any active scheduled jobs whose `next_run_at`
    /// falls within the upcoming window.
    async fn has_upcoming_jobs(&self, agent_id: Uuid) -> anyhow::Result<bool> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM scheduled_jobs \
             WHERE agent_id = $1 \
             AND status = 'active' \
             AND next_run_at < NOW() + make_interval(mins => $2)",
        )
        .bind(agent_id)
        .bind(UPCOMING_JOB_WINDOW_MINUTES)
        .fetch_one(&self.db)
        .await
        .context("Failed to check upcoming scheduled jobs")?;

        Ok(count > 0)
    }

    /// Check if the agent has any pending (undelivered) messages in the queue.
    async fn has_pending_messages(&self, agent_id: Uuid) -> anyhow::Result<bool> {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM message_queue \
             WHERE agent_id = $1 \
             AND status = 'pending'",
        )
        .bind(agent_id)
        .fetch_one(&self.db)
        .await
        .context("Failed to check pending messages")?;

        Ok(count > 0)
    }
}
