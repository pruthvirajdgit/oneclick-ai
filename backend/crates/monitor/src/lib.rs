//! OneClick.ai — Health monitoring and idle detection.
//!
//! The [`IdleMonitor`] periodically scans for running agents that have been
//! idle beyond a configurable timeout and puts them to sleep via the
//! orchestrator, freeing up container resources. It is **task-aware**: agents
//! with upcoming scheduled jobs (due within 20 minutes) or pending messages in
//! the queue are left running.

mod service;

pub use service::IdleMonitor;
