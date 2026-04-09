# Local POC (Pre-Phase 1)

This directory contains the **original proof-of-concept** code from early project exploration. It was used to validate that OpenClaw containers could be managed via Docker Compose and shell scripts.

## What's Here

| Directory/File | Purpose |
|---------------|---------|
| `agent-runtime/` | Custom OpenClaw Docker image + compose files for local testing |
| `scripts/` | Shell scripts for agent lifecycle (start/stop/restart) |
| `start.sh` | One-click setup script |

## Status: Archived

This code is **not part of the current architecture**. The Phase 1 backend (`backend/` crate workspace) replaces all of this with a proper Rust API server, orchestrator, and Docker runtime.

Kept for reference only. Do not extend or depend on these files.
