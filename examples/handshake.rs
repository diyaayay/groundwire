//! Proof of life: spawn `rust-analyzer`, complete the `initialize` handshake,
//! and print the raw `initialize` result. This is Phase 1b of the roadmap — the
//! first time our framing drives a *real* language server instead of a test
//! buffer.
//!
//! Run it from the repo root (rust-analyzer will index *this* crate):
//!
//! ```sh
//! cargo run --example handshake
//! ```
//!
//! What it does NOT do yet: wait for indexing to finish, or issue a real query
//! (`textDocument/definition`). That readiness logic is Phase 2 — the hard,
//! differentiating part. Here we only prove we can spawn, frame, and handshake.

use std::process::Stdio;

use anyhow::{Context, Result};
use serde_json::{Value, json};
use tokio::io::BufReader;
use tokio::process::Command;

// We reuse the framing from our own library crate — the same functions the unit
// tests exercised against a `Vec<u8>` now drive a live process. That reuse is the
// whole reason we split the code into a library.
use groundwire::lsp::transport::{read_message, write_message};

#[tokio::main]
async fn main() -> Result<()> {
    // --- 1. Spawn rust-analyzer, owning its stdio pipes. ---
    // `Stdio::piped()` means "connect this stream to a pipe I control", so we can
    // write to the child's stdin and read from its stdout. We let its stderr flow
    // to *our* stderr (`inherit`) so we can watch rust-analyzer's own log lines —
    // handy while learning, though it's chatty.
    let mut child = Command::new("rust-analyzer")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .context("failed to spawn `rust-analyzer` — is it installed and on PATH?")?;

    // `.take()` moves the pipe handles out of the child struct so we own them
    // outright. The child's *stdin* is the thing WE write to; its *stdout* is the
    // thing WE read from — mirror image, which always takes a second to internalize.
    let mut server_stdin = child.stdin.take().context("child had no stdin")?;
    let server_stdout = child.stdout.take().context("child had no stdout")?;

    // Wrap stdout in a BufReader so it satisfies the *buffered* read trait that
    // `read_message` needs (line-by-line header reads require look-ahead buffering).
    let mut reader = BufReader::new(server_stdout);

    // --- 2. Build the `initialize` request. ---
    // rust-analyzer needs to know which folder to index. We point it at the current
    // directory (this very crate) as a `file://` URI.
    let root_path = std::env::current_dir()?;
    let root_uri = format!("file://{}", root_path.display());

    let initialize = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            // Our PID lets the server exit if we die unexpectedly.
            "processId": std::process::id(),
            // `.clone()` because we reuse this URI again in workspaceFolders below,
            // and a String can't be moved twice.
            "rootUri": root_uri.clone(),
            // Empty capabilities = "I support the bare minimum." Fine for a probe;
            // Phase 2 will declare the specific client capabilities we actually use.
            "capabilities": {},
            "workspaceFolders": [{
                "uri": root_uri,
                "name": "groundwire"
            }]
        }
    });

    // --- 3. Send it over the wire (Content-Length framed). ---
    eprintln!("→ sending initialize …");
    write_message(&mut server_stdin, &initialize)
        .await
        .context("failed to send initialize")?;

    // --- 4. Read frames until we find the response to our request id (1). ---
    // rust-analyzer may send notifications (logs, progress) *before* the response,
    // so we can't assume the first frame is our answer — we match on `id`. This is
    // request/response *correlation* in its simplest, hand-rolled form; Phase 2
    // generalizes it to many concurrent requests via channels.
    loop {
        match read_message(&mut reader).await? {
            Some(message) => {
                if message.get("id") == Some(&json!(1)) {
                    // Found our initialize result. Pretty-print the raw JSON — the
                    // whole point of this milestone.
                    println!("{}", serde_json::to_string_pretty(&message)?);
                    break;
                }

                // Not our response — a notification or unrelated message. Log what
                // it was (to stderr, so stdout stays clean) and keep reading.
                let kind = message
                    .get("method")
                    .and_then(Value::as_str)
                    .unwrap_or("(a response with a different id)");
                eprintln!("… skipping server message: {kind}");
            }
            None => {
                // Clean EOF before we got our answer: the server closed its stdout.
                anyhow::bail!("rust-analyzer closed its output before replying to initialize");
            }
        }
    }

    // --- 5. Shut the probe down. ---
    // A real client would send `initialized`, then later a `shutdown` request and
    // an `exit` notification (Phase 2's lifecycle). For a one-shot probe we just
    // kill the child so it doesn't linger after we print and exit.
    eprintln!("✓ got initialize result — killing rust-analyzer");
    child.kill().await.context("failed to stop rust-analyzer")?;

    Ok(())
}
