//! Minimal client for the `codex app-server` JSON-RPC interface.
//!
//! Protocol summary (see `docs/app-server-protocol.md` for the full notes and
//! source links):
//!
//! - Transport: `codex app-server` over stdio (default), newline-delimited
//!   JSON, one JSON-RPC 2.0 message per line (the `"jsonrpc":"2.0"` field is
//!   omitted on the wire).
//! - Every connection must send `initialize` first, then an `initialized`
//!   notification, before any other method is accepted.
//! - Requests carry `method` + `id` (+ optional `params`) and get back either
//!   `result` or `error` keyed by the same `id`. Notifications have no `id`.
//!
//! This client only implements what Codex Pacer needs: the handshake plus a
//! generic blocking `call()`. It spawns a fresh `codex app-server` process per
//! poll rather than keeping one alive across the whole app lifetime — simple
//! and correct for a refresh-every-few-minutes tray app; revisit if polling
//! frequency ever goes up.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

pub struct AppServerClient {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: u64,
}

impl AppServerClient {
    /// Spawns `<binary> app-server` and performs the initialize/initialized
    /// handshake required before any other call.
    pub fn spawn(binary: &str) -> Result<Self, String> {
        let mut child = Command::new(binary)
            .arg("app-server")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to spawn `{binary} app-server`: {e}"))?;

        let stdin = child.stdin.take().ok_or("missing stdin pipe on app-server process")?;
        let stdout = child.stdout.take().ok_or("missing stdout pipe on app-server process")?;

        let mut client = AppServerClient {
            child,
            stdin,
            reader: BufReader::new(stdout),
            next_id: 0,
        };
        client.handshake()?;
        Ok(client)
    }

    fn handshake(&mut self) -> Result<(), String> {
        self.call(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "codex-pacer",
                    "title": "Codex Pacer",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }),
        )?;
        self.notify("initialized", json!({}))
    }

    fn write_line(&mut self, value: &Value) -> Result<(), String> {
        let line = serde_json::to_string(value).map_err(|e| e.to_string())?;
        writeln!(self.stdin, "{line}").map_err(|e| e.to_string())?;
        self.stdin.flush().map_err(|e| e.to_string())
    }

    /// Sends a notification (no response expected).
    pub fn notify(&mut self, method: &str, params: Value) -> Result<(), String> {
        self.write_line(&json!({ "method": method, "params": params }))
    }

    /// Sends a request and blocks until the matching response arrives,
    /// skipping over any notifications or responses to other calls that
    /// show up first on the stream.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        self.write_line(&json!({ "method": method, "id": id, "params": params }))?;

        loop {
            let mut line = String::new();
            let bytes_read = self
                .reader
                .read_line(&mut line)
                .map_err(|e| format!("failed reading from app-server: {e}"))?;

            if bytes_read == 0 {
                return Err(format!(
                    "app-server closed the connection before responding to `{method}`"
                ));
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let msg: Value = serde_json::from_str(trimmed)
                .map_err(|e| format!("invalid JSON from app-server: {e}"))?;

            let Some(msg_id) = msg.get("id").and_then(|v| v.as_u64()) else {
                continue; // notification, not the response we're waiting for
            };
            if msg_id != id {
                continue; // response to a different in-flight call
            }

            if let Some(error) = msg.get("error") {
                return Err(format!("app-server error for `{method}`: {error}"));
            }
            return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
        }
    }
}

impl Drop for AppServerClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}
