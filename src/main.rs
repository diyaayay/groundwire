//! Binary entry point for the `groundwire` MCP server.
//!
//! Intentionally thin: all real logic lives in the `groundwire` library crate so
//! it can be integration-tested against a real language server. Two rules shape
//! this file:
//!   1. **stdout is sacred** — it carries the MCP JSON-RPC frames. Every
//!      human-facing log line goes to **stderr** via `tracing`.
//!   2. The process is a long-lived server: start, serve tools over stdio, and
//!      run until the client (Claude Code) disconnects.

use anyhow::Result;
use tracing_subscriber::EnvFilter;

// `#[tokio::main]` turns this `async fn` into a normal `fn main` that spins up the
// Tokio runtime and blocks on our future. It only compiles because we enabled the
// `macros` + `rt-multi-thread` features earlier — the Cargo features lesson, made
// concrete.
#[tokio::main]
async fn main() -> Result<()> {
    // Logs → STDERR, so they can never corrupt the MCP JSON-RPC stream on STDOUT.
    // `RUST_LOG=groundwire=debug` (etc.) tunes verbosity at runtime; if it's unset
    // we default to `info`.
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    tracing::info!(version = env!("CARGO_PKG_VERSION"), "groundwire starting");

    // The MCP server will start here once it's built (Phase 3):
    //     groundwire::mcp::serve_stdio().await?;
    // For now this just proves the async runtime + logging pipeline work.
    tracing::warn!("no MCP server wired yet — scaffold only");

    Ok(())
}
