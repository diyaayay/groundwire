//! The LSP client side of the bridge.
//!
//! We hand-roll an async LSP *client* (rather than use a framework) because the
//! whole differentiation of groundwire lives here: correct lifecycle and
//! readiness handling. `tower-lsp` and friends are server-side only anyway.
//!
//! Layers, bottom to top (added phase by phase):
//! - [`transport`] — frame JSON-RPC messages over a child process's stdio.  (Phase 1)
//! - `client`    — correlate requests with responses; route notifications.  (Phase 2)
//! - `lifecycle` — the initialize → ready state machine.                    (Phase 2)

pub mod transport;
