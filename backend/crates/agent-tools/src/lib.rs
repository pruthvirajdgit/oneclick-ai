//! OneClick.ai - Agent tool definitions and OpenClaw integration.
//!
//! Provides tool definitions for agent capabilities (scheduling, notifications)
//! and generates configuration for mounting into agent containers.

mod tools;

pub use tools::{available_tools, generate_tool_config, ToolDefinition};
