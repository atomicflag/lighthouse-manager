# AGENTS.md

Rules for AI agents working on this project.

## Logging — no raw println!/eprintln!

**Raw `println!` and `eprintln!` calls are forbidden.** Every log message must go through the `tracing` ecosystem (`tracing::debug!`, `tracing::info!`, `tracing::warn!`, `tracing::error!`, `tracing::trace!`).

## Success criteria

A change is only considered successful when **all three** of the following pass:

1. `cargo check`
2. `cargo test`
3. `cargo clippy`
