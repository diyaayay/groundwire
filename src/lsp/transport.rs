//! JSON-RPC message framing over an async byte stream.
//!
//! LSP frames each message like a tiny HTTP message: a `Content-Length: N\r\n`
//! header, a blank line, then exactly `N` bytes of JSON. This module reads and
//! writes those frames over a language server's stdin/stdout.
//!
//! We work at the `serde_json::Value` level here (raw JSON in, raw JSON out).
//! Typed messages and request/response correlation come one layer up in the
//! `client` module (Phase 2); this layer only cares about *bytes → one JSON
//! value* and *one JSON value → bytes*.

use std::io;

use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

/// Write one JSON-RPC message as an LSP frame: the `Content-Length` header, a
/// blank line, then the JSON body.
///
/// The `W: AsyncWriteExt + Unpin` bound, read slowly:
/// - `AsyncWriteExt` = "anything I can asynchronously write bytes to" — here it's
///   a child process's **stdin**, but keeping it generic means the same function
///   works over a socket or an in-memory buffer (which is how the tests reuse it).
/// - `Unpin` is a requirement for `.await`-ing the write helpers; the stream types
///   we use satisfy it automatically, so treat it as boilerplate for now.
pub async fn write_message<W>(writer: &mut W, message: &Value) -> io::Result<()>
where
    W: AsyncWriteExt + Unpin,
{
    // Serialize first, because the header needs the exact *byte* length of the
    // body. `to_vec` gives bytes (not a String) — Content-Length counts bytes.
    let body = serde_json::to_vec(message)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // `\r\n\r\n`: one CRLF ends the header line, the second is the blank line that
    // separates headers from body. LSP inherits this framing from HTTP.
    let header = format!("Content-Length: {}\r\n\r\n", body.len());

    // Two writes + a flush. Each `.await` is a suspend point: if the pipe buffer is
    // full we pause here and let the runtime do other work until it drains —
    // instead of blocking the thread.
    writer.write_all(header.as_bytes()).await?;
    writer.write_all(&body).await?;

    // Pipes are buffered; without `flush` the bytes can sit in our buffer and the
    // server waits forever for a message we think we "sent". Flush = push it out now.
    writer.flush().await?;
    Ok(())
}

/// Read exactly one JSON-RPC message from an LSP frame.
///
/// Returns:
/// - `Ok(Some(value))` — a complete message was read;
/// - `Ok(None)` — clean end-of-stream *at a frame boundary* (the server closed its
///   stdout with nothing half-sent). Callers treat this as "the server is gone";
/// - `Err(..)` — a malformed frame or an I/O failure mid-message.
///
/// The `R: AsyncBufReadExt` bound (note: *Buf*) matters: reading headers means
/// reading *line by line*, which needs a buffered reader that can scan ahead for
/// the `\n`. Callers wrap the child's raw stdout in a `tokio::io::BufReader` first.
pub async fn read_message<R>(reader: &mut R) -> io::Result<Option<Value>>
where
    R: AsyncBufReadExt + Unpin,
{
    // --- 1. Parse the header block, one line at a time, until the blank line. ---
    let mut content_length: Option<usize> = None;
    let mut line = String::new();

    loop {
        line.clear();
        // `read_line` reads up to and *including* the `\n`, appending to `line`.
        // It returns the number of bytes read; 0 means EOF (write end closed).
        let bytes_read = reader.read_line(&mut line).await?;

        if bytes_read == 0 {
            if content_length.is_some() {
                // We saw a Content-Length but the stream died before the blank
                // line — a truncated frame. That is an error.
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "stream closed in the middle of a message header",
                ));
            }
            // Clean EOF at a frame boundary: the server closed its output.
            return Ok(None);
        }

        // Strip the trailing `\r\n` (or lone `\n`). What's left is the header
        // content, e.g. `Content-Length: 128`.
        let header = line.trim_end();

        if header.is_empty() {
            // Blank line: headers are done, the body starts next.
            break;
        }

        // We only care about Content-Length. Other headers (e.g. an optional
        // `Content-Type`) are allowed by the spec but safe to ignore.
        if let Some(value) = header.strip_prefix("Content-Length:") {
            let len: usize = value.trim().parse().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid Content-Length value: {:?}", value.trim()),
                )
            })?;
            content_length = Some(len);
        }
    }

    // A frame with no Content-Length is malformed — we can't know where the body
    // ends, so we can't safely read on.
    let len = content_length.ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "frame had no Content-Length header")
    })?;

    // --- 2. Read exactly `len` bytes of body. ---
    // `read_exact` loops internally until the buffer is full (or errors on a short
    // read). This is the "length header IS the boundary" guarantee in action: we
    // take precisely as many bytes as the header promised — no more, no less.
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;

    // --- 3. Parse those bytes as one JSON value. ---
    let value = serde_json::from_slice(&body)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(Some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Round-trip: what we write must read back byte-for-byte identical. We use an
    /// in-memory `Vec<u8>` as the "stream" — the same generic functions that will
    /// drive a real child process work here unchanged, which is the payoff of the
    /// generic `W`/`R` bounds.
    #[tokio::test]
    async fn round_trip_single_message() {
        let msg = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "hello": "world" }
        });

        // Write into a byte buffer (Vec<u8> implements the async-write trait).
        let mut buffer: Vec<u8> = Vec::new();
        write_message(&mut buffer, &msg).await.unwrap();

        // The framing must be exactly: header, blank line, then the JSON body.
        let body = serde_json::to_vec(&msg).unwrap();
        let expected_header = format!("Content-Length: {}\r\n\r\n", body.len());
        assert!(buffer.starts_with(expected_header.as_bytes()));

        // Read it back. A `&[u8]` slice implements the buffered async-read traits,
        // so it stands in for the server's stdout, advancing as we read.
        let mut cursor = &buffer[..];
        let read_back = read_message(&mut cursor).await.unwrap();
        assert_eq!(read_back, Some(msg));
    }

    /// Two messages back-to-back must split at the right boundary — reading the
    /// first must not consume any of the second.
    #[tokio::test]
    async fn reads_two_framed_messages_in_sequence() {
        let first = json!({ "n": 1 });
        let second = json!({ "n": 2 });

        let mut buffer: Vec<u8> = Vec::new();
        write_message(&mut buffer, &first).await.unwrap();
        write_message(&mut buffer, &second).await.unwrap();

        let mut cursor = &buffer[..];
        assert_eq!(read_message(&mut cursor).await.unwrap(), Some(first));
        assert_eq!(read_message(&mut cursor).await.unwrap(), Some(second));
        // Nothing left → clean EOF.
        assert_eq!(read_message(&mut cursor).await.unwrap(), None);
    }

    /// A clean end-of-stream at a frame boundary is `Ok(None)`, not an error.
    #[tokio::test]
    async fn empty_stream_is_clean_eof() {
        let mut cursor = &b""[..];
        assert_eq!(read_message(&mut cursor).await.unwrap(), None);
    }
}
