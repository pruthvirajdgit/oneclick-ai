//! OneClick.ai — Agent orchestration and lifecycle management.
//!
//! This crate manages the lifecycle of per-user AI agent containers:
//! creating, waking, sleeping, and destroying them. It provides:
//!
//! - [`AgentRuntime`] — a trait abstracting the container runtime
//!   (Phase 1: Docker, future: CRIU, Firecracker).
//! - [`DockerRuntime`] — bollard-based implementation of `AgentRuntime`.
//! - [`Orchestrator`] — the service layer that ties the runtime to
//!   the database with per-agent locking.

pub mod runtime;
pub mod service;

pub use runtime::{AgentRuntime, DockerRuntime};
pub use service::Orchestrator;
