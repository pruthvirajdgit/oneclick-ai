//! OneClick.ai — Cron-based task scheduling
//!
//! External cron runner that polls the database every N seconds for due
//! scheduled jobs, wakes the target agent via the orchestrator, delivers
//! the task message over HTTP, and advances `next_run_at`.

pub mod cron_utils;
pub mod service;

pub use service::Scheduler;
