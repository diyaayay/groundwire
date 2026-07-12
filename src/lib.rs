//! # groundwire
//!
//! A correctness-first, token-efficient **LSP → MCP bridge**.
//!
//! Coding agents (Claude Code and friends) are strong at *reading* code but weak
//! at *navigating* it — "who calls this?", "where is this defined?", "what's the
//! type here?" They fall back to `grep`, which returns comments and unrelated
//! same-named symbols. Real editors solve this with **Language Servers**
//! (rust-analyzer, tsserver, gopls, …) that understand code semantically.
//!
//! `groundwire` is the adapter in the middle. It speaks **MCP** on one side,
//! drives a real **Language Server** over **LSP** on the other, and returns
//! compact, LLM-shaped answers.
//!
//! ```text
//! Claude Code ──MCP (JSON-RPC/stdio)──► groundwire ──LSP (JSON-RPC/stdio)──► rust-analyzer
//!             ◄──── compact snippets ───            ◄──── Location/Hover ────
//! ```
//!
//! ## Target module map (built incrementally, phase by phase)
//!
//! We add each `pub mod` below only when we actually build it — no empty stubs.
//!
//! - `lsp`       — the hand-rolled async LSP *client*:
//!     - `transport` — Content-Length framing over a child's stdio   (Phase 1)
//!     - `client`    — JSON-RPC request/response correlation          (Phase 2)
//!     - `lifecycle` — the initialize → ready state machine           (Phase 2)
//! - `mcp`       — the `rmcp` server + the five tools                 (Phase 3+)
//! - `format`    — LSP results → compact, token-efficient text        (Phase 4)
//! - `workspace` — project-root & language detection                  (Phase 2+)

// Modules are declared here as we build them.
pub mod lsp;
