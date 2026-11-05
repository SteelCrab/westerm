//! Helpers shared by the integration test files. Each file uses a part.
#![allow(dead_code)]

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

pub const BIN: &str = env!("CARGO_BIN_EXE_westerm");

/// A child process that is killed when it goes out of scope, so a test that
/// fails half way leaves nothing running in the background.
pub struct Spawned(pub Child);

impl Drop for Spawned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Runs `westerm stop` for a state folder when it goes out of scope. It stops
/// a host the test did not start itself: the dashboard starts its host in a
/// separate session, so killing the dashboard leaves the host running. On a
/// server, `stop` also ends the remote gate.
pub struct StopHostOnDrop<'a>(pub &'a Path);

impl Drop for StopHostOnDrop<'_> {
    fn drop(&mut self) {
        let _ = Command::new(BIN)
            .arg("stop")
            .env("WESTERM_STATE_DIR", self.0)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// A pane's view of one session over any byte stream to a host.
pub struct Viewer<S> {
    pub ws: WebSocketStream<S>,
    pub output: String,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Viewer<S> {
    /// Attaches over `stream`, sends the fitted size first as the browser
    /// does, and waits for "ready".
    pub async fn attach_over(stream: S, query: &str) -> (Self, serde_json::Value) {
        let (mut ws, _) = tokio_tungstenite::client_async(format!("ws://host/session?{query}"), stream).await.unwrap();
        ws.send(Message::text(r#"{"type":"resize","cols":80,"rows":24,"width":640,"height":408}"#)).await.unwrap();
        let mut viewer = Self { ws, output: String::new() };
        let ready = viewer.next_json().await;
        assert_eq!(ready["type"], "ready");
        (viewer, ready)
    }

    pub async fn next_json(&mut self) -> serde_json::Value {
        loop {
            match self.ws.next().await.unwrap().unwrap() {
                Message::Text(text) => return serde_json::from_str(&text).unwrap(),
                Message::Binary(bytes) => self.take(&bytes).await,
                _ => {}
            }
        }
    }

    async fn take(&mut self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        // Answer "where is the cursor?" (DSR 6) as a terminal does. Windows
        // ConPTY asks once at start and holds the shell until it gets a reply;
        // in the dashboard, wterm answers it.
        let questions = text.matches("\x1b[6n").count();
        self.output.push_str(&text);
        let ack = format!(r#"{{"type":"ack","bytes":{}}}"#, bytes.len());
        let _ = self.ws.send(Message::text(ack)).await;
        for _ in 0..questions {
            let _ = self.ws.send(Message::text(r#"{"type":"input","data":"\u001b[1;1R"}"#)).await;
        }
    }

    pub async fn wait_for(&mut self, needle: &str) {
        // PowerShell on a fresh CI machine can take several seconds to start.
        let deadline = Instant::now() + Duration::from_secs(30);
        while !self.output.contains(needle) {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "output contains {needle:?}; got {:?}", self.output);
            if let Ok(Some(Ok(Message::Binary(bytes)))) = tokio::time::timeout(left, self.ws.next()).await {
                self.take(&bytes).await;
            }
        }
    }

    pub async fn send(&mut self, json: &str) {
        self.ws.send(Message::text(json)).await.unwrap();
    }

    /// The close code the host sends, skipping output before it.
    pub async fn close_code(&mut self) -> u16 {
        loop {
            match tokio::time::timeout(Duration::from_secs(5), self.ws.next()).await.unwrap() {
                Some(Ok(Message::Close(Some(frame)))) => return u16::from(frame.code),
                Some(Ok(_)) => continue,
                other => panic!("expected a close frame, got {other:?}"),
            }
        }
    }

    pub async fn detach(mut self) {
        let _ = self.ws.close(None).await;
        // Give the host a moment to see the close before the next attach.
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
