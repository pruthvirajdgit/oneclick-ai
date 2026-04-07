//! OneClick.ai — Agent orchestration and lifecycle management.
//!
//! This crate manages the lifecycle of per-user AI agent containers/VMs:
//! creating, waking, sleeping, and destroying them. It provides:
//!
//! - [`AgentRuntime`] — a trait abstracting the container/VM runtime.
//! - [`DockerRuntime`] — bollard-based implementation of `AgentRuntime`.
//! - [`FirecrackerRuntime`] — fctools-based Firecracker microVM implementation.
//! - [`TapManager`] — TAP network device pool for Firecracker VMs.
//! - [`Orchestrator`] — the service layer that ties the runtime to
//!   the database with per-agent locking.

pub mod firecracker_runtime;
pub mod runtime;
pub mod service;
pub mod tap_manager;

pub use firecracker_runtime::FirecrackerRuntime;
pub use runtime::{AgentRuntime, DockerRuntime};
pub use service::Orchestrator;
pub use tap_manager::TapManager;
