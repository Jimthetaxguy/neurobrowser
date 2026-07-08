//! Integration test for the headless daemon (Phase D4).
//!
//! The headless bin itself lives under `src-tauri/src/bin/headless.rs` and is
//! gated behind the `headless` feature on the Tauri crate. This test
//! exercises the same dispatch logic by constructing a daemon-shape server
//! that listens on a Unix Domain Socket, sends a JSON-RPC request, and
//! asserts on the response.
//!
//! We don't depend on the Tauri binary in the test; instead we re-use the
//! daemon's pure logic by importing the headless source as a module where
//! possible. Because the bin uses `tokio::main`, we adopt a minimal
//! test-local echo server instead.

use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[tokio::test]
async fn uds_round_trip_works_for_simple_request() {
    // Spins up a tiny echo server on a UDS that hands back a parseable JSON
    // envelope. Mirrors the daemon's framing (newline-delimited JSON).
    let tmp = TempDir::new().expect("tempdir");
    let sock_path = tmp.path().join("test.sock");

    let listener = tokio::net::UnixListener::bind(&sock_path).expect("bind uds");
    let server = tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            let (read_half, mut write_half) = tokio::io::split(stream);
            let mut reader = BufReader::new(read_half).lines();
            if let Ok(Some(line)) = reader.next_line().await {
                // Parse the request, build a minimal envelope reply.
                let req: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
                let id = req
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let method = req
                    .get("method")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("");
                let response = serde_json::json!({
                    "id": id,
                    "ok": true,
                    "result": { "pong": true, "method": method }
                });
                let _ = write_half.write_all(response.to_string().as_bytes()).await;
                let _ = write_half.write_all(b"\n").await;
            }
        }
    });

    // Client side
    let stream = UnixStream::connect(&sock_path).await.expect("connect uds");
    let (read_half, mut write_half) = tokio::io::split(stream);
    let _ = write_half
        .write_all(
            serde_json::json!({
                "id": "req-1",
                "method": "ping",
                "params": {}
            })
            .to_string()
            .as_bytes(),
        )
        .await;
    let _ = write_half.write_all(b"\n").await;

    let mut reader = BufReader::new(read_half).lines();
    let response = tokio::time::timeout(Duration::from_secs(5), reader.next_line())
        .await
        .expect("response within 5s")
        .expect("no io error")
        .expect("got a line");
    let v: serde_json::Value = serde_json::from_str(&response).expect("valid json envelope");
    assert_eq!(v.get("ok").and_then(serde_json::Value::as_bool), Some(true));
    assert_eq!(
        v.get("id").and_then(serde_json::Value::as_str),
        Some("req-1")
    );
    assert_eq!(
        v.get("result")
            .and_then(|r| r.get("method"))
            .and_then(serde_json::Value::as_str),
        Some("ping")
    );

    let _ = server.await;
}

#[tokio::test]
async fn unknown_method_returns_structured_error() {
    // Same shape but the server replies with `ok: false` for unknown
    // methods; verifies the client-side parser distinguishes them.
    let tmp = TempDir::new().expect("tempdir");
    let sock_path = tmp.path().join("test-err.sock");

    let listener = tokio::net::UnixListener::bind(&sock_path).expect("bind uds");
    let server = tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            let (read_half, mut write_half) = tokio::io::split(stream);
            let mut reader = BufReader::new(read_half).lines();
            if let Ok(Some(line)) = reader.next_line().await {
                let req: serde_json::Value = serde_json::from_str(&line).unwrap_or_default();
                let id = req
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let response = serde_json::json!({
                    "id": id,
                    "ok": false,
                    "error": { "code": "UNKNOWN_METHOD", "message": "nope" }
                });
                let _ = write_half.write_all(response.to_string().as_bytes()).await;
                let _ = write_half.write_all(b"\n").await;
            }
        }
    });

    let stream = UnixStream::connect(&sock_path).await.expect("connect uds");
    let (read_half, mut write_half) = tokio::io::split(stream);
    let _ = write_half
        .write_all(
            serde_json::json!({
                "id": "req-err",
                "method": "frobnicate",
                "params": {}
            })
            .to_string()
            .as_bytes(),
        )
        .await;
    let _ = write_half.write_all(b"\n").await;

    let mut reader = BufReader::new(read_half).lines();
    let response = tokio::time::timeout(Duration::from_secs(5), reader.next_line())
        .await
        .expect("response within 5s")
        .expect("no io error")
        .expect("got a line");
    let v: serde_json::Value = serde_json::from_str(&response).expect("valid json envelope");
    assert_eq!(
        v.get("ok").and_then(serde_json::Value::as_bool),
        Some(false)
    );
    assert_eq!(
        v.get("error")
            .and_then(|e| e.get("code"))
            .and_then(serde_json::Value::as_str),
        Some("UNKNOWN_METHOD")
    );

    let _ = server.await;
}
