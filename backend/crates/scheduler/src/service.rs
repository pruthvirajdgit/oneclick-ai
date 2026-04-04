//! Core scheduler service — polls for due jobs and executes them.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use sqlx::PgPool;

use oneclick_orchestrator::Orchestrator;
use oneclick_shared::models::schedule::ScheduledJob;

use crate::cron_utils;

// ── Request payload sent to the agent container ─────────────────────────

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    message: &'a str,
}

// ── Scheduler ───────────────────────────────────────────────────────────

/// Background cron runner that polls for due [`ScheduledJob`]s and
/// dispatches them to agent containers via the orchestrator.
pub struct Scheduler {
    db: PgPool,
    orchestrator: Arc<Orchestrator>,
    http_client: reqwest::Client,
    poll_interval: Duration,
}

impl Scheduler {
    /// Create a new `Scheduler`.
    ///
    /// * `db`             – Postgres connection pool.
    /// * `orchestrator`   – Shared orchestrator for waking agents.
    /// * `poll_interval`  – How often to poll for due jobs (e.g. 60 s).
    pub fn new(
        db: PgPool,
        orchestrator: Arc<Orchestrator>,
        poll_interval: Duration,
    ) -> Self {
        Self {
            db,
            orchestrator,
            http_client: reqwest::Client::new(),
            poll_interval,
        }
    }

    /// Start the scheduler loop. Runs until the future is cancelled.
    pub async fn run(&self) -> anyhow::Result<()> {
        tracing::info!(
            poll_interval_secs = self.poll_interval.as_secs(),
            "Scheduler started"
        );

        loop {
            if let Err(e) = self.tick().await {
                tracing::error!(error = %e, "Scheduler tick failed");
            }
            tokio::time::sleep(self.poll_interval).await;
        }
    }

    // ── Tick ────────────────────────────────────────────────────────────

    /// Execute one tick: find due jobs and process each one.
    async fn tick(&self) -> anyhow::Result<()> {
        let due_jobs = self.find_due_jobs().await?;

        if !due_jobs.is_empty() {
            tracing::info!(count = due_jobs.len(), "Found due scheduled jobs");
        }

        for job in &due_jobs {
            if let Err(e) = self.execute_job(job).await {
                tracing::error!(
                    job_id   = %job.id,
                    agent_id = %job.agent_id,
                    error    = %e,
                    "Failed to execute scheduled job"
                );
            }
        }

        Ok(())
    }

    // ── Query ───────────────────────────────────────────────────────────

    /// Fetch up to 50 active jobs whose `next_run_at` is in the past.
    async fn find_due_jobs(&self) -> anyhow::Result<Vec<ScheduledJob>> {
        let jobs = sqlx::query_as::<_, ScheduledJob>(
            r#"
            SELECT id, user_id, agent_id, cron_expr, task_message,
                   next_run_at, last_run_at, status, created_at
            FROM scheduled_jobs
            WHERE status = 'active' AND next_run_at <= NOW()
            ORDER BY next_run_at ASC
            LIMIT 50
            "#,
        )
        .fetch_all(&self.db)
        .await?;

        Ok(jobs)
    }

    // ── Execute a single job ────────────────────────────────────────────

    /// Wake the agent, deliver the task message, and advance `next_run_at`.
    async fn execute_job(&self, job: &ScheduledJob) -> anyhow::Result<()> {
        // 1. Ensure the agent container is running.
        let agent = self
            .orchestrator
            .ensure_ready(job.agent_id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to wake agent {}: {e}", job.agent_id))?;

        let container_name = agent.container_name.as_deref().ok_or_else(|| {
            anyhow::anyhow!("Agent {} has no container_name", job.agent_id)
        })?;

        // 2. POST the task message to the agent's chat endpoint.
        let url = format!("http://{container_name}:3000/api/chat");

        let response = self
            .http_client
            .post(&url)
            .json(&ChatRequest {
                message: &job.task_message,
            })
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("HTTP request to agent failed: {e}"))?;

        if !response.status().is_success() {
            anyhow::bail!(
                "Agent responded with status {} for job {}",
                response.status(),
                job.id,
            );
        }

        // 3. Calculate the next run time and update the database.
        let next_run = cron_utils::next_run_at(&job.cron_expr)?;

        sqlx::query(
            "UPDATE scheduled_jobs SET last_run_at = NOW(), next_run_at = $1 WHERE id = $2",
        )
        .bind(next_run)
        .bind(job.id)
        .execute(&self.db)
        .await?;

        tracing::info!(
            job_id      = %job.id,
            agent_id    = %job.agent_id,
            next_run_at = %next_run,
            "Scheduled job executed successfully"
        );

        Ok(())
    }
}
