//! Live sessions by id, and revival of ended ones from the store. One task
//! handles every attach in order, so two windows cannot spawn a session twice.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use tokio::sync::{mpsc, oneshot};

use super::session::{self, Command, Handle, Restored, Spec};
use super::store::SessionStore;
use crate::protocol::{AttachMode, AttachRequest, ServerMessage};
use crate::spawn::LaunchSpec;
use crate::state_dir::home_dir;

const LIVE_LIMIT: usize = 48;

pub enum RegistryCommand {
    Attach { ws: WebSocket, request: AttachRequest },
    /// Saves every session; used on shutdown, logout, and reboot.
    SaveAll { reply: oneshot::Sender<()> },
    Count { reply: oneshot::Sender<usize> },
}

pub fn start(store: Arc<SessionStore>) -> mpsc::UnboundedSender<RegistryCommand> {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(Registry { store, live: HashMap::new(), serial: 0 }.run(rx));
    tx
}

struct Registry {
    store: Arc<SessionStore>,
    live: HashMap<String, Handle>,
    serial: u64,
}

fn refuse(mut ws: WebSocket, message: String) {
    tokio::spawn(async move {
        let _ = ws.send(Message::Text(ServerMessage::Error { message }.to_json().into())).await;
        let _ = ws.send(Message::Close(Some(CloseFrame { code: 1011, reason: "refused".into() }))).await;
    });
}

impl Registry {
    async fn run(mut self, mut commands: mpsc::UnboundedReceiver<RegistryCommand>) {
        let (ended_tx, mut ended_rx) = mpsc::unbounded_channel::<(String, u64)>();
        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    None => break,
                    Some(RegistryCommand::Attach { ws, request }) => self.attach(ws, request, &ended_tx).await,
                    Some(RegistryCommand::SaveAll { reply }) => {
                        self.save_all().await;
                        let _ = reply.send(());
                    }
                    Some(RegistryCommand::Count { reply }) => {
                        let _ = reply.send(self.live.len());
                    }
                },
                Some((id, serial)) = ended_rx.recv() => {
                    if self.live.get(&id).is_some_and(|handle| handle.serial == serial) {
                        self.live.remove(&id);
                    }
                }
            }
        }
    }

    async fn attach(&mut self, ws: WebSocket, request: AttachRequest, ended: &mpsc::UnboundedSender<(String, u64)>) {
        if let Some(running) = self.live.get(&request.id) {
            if running.alive() && running.kind == request.kind && running.kind.is_text() {
                let _ = running.tx.send(Command::Attach { ws, size: request.size, mode: AttachMode::Reattached });
                return;
            }
            // A pixel app restarts on every attach; see session.rs.
            let _ = running.tx.send(Command::End { forget: false });
            self.live.remove(&request.id);
        }
        if self.live.len() >= LIVE_LIMIT {
            refuse(ws, request.lang.too_many_sessions(LIVE_LIMIT));
            return;
        }

        let meta = self.store.read_meta(&request.id).filter(|meta| meta.kind == request.kind);
        let cwd = match &meta {
            Some(meta) if meta.cwd.is_dir() => meta.cwd.clone(),
            _ => self.cwd_for(request.cwd_from.as_deref()).await,
        };
        let restored = meta.as_ref().filter(|_| request.kind.is_text()).and_then(|meta| {
            Some(Restored {
                screen: self.store.read_screen(&request.id)?,
                saved_at: meta.saved_at.clone(),
                lang: request.lang,
            })
        });
        let launch = LaunchSpec {
            kind: request.kind,
            url: request.url.clone().or_else(|| meta.as_ref().and_then(|m| m.url.clone())),
            target: request.target.clone().or_else(|| meta.as_ref().and_then(|m| m.target.clone())),
            cwd,
        };
        let mode = if meta.is_some() { AttachMode::Revived } else { AttachMode::New };
        self.serial += 1;
        let spec = Spec { id: request.id.clone(), launch };
        match session::start(spec, request.size, self.store.clone(), restored, self.serial, ended.clone()) {
            Ok(handle) => {
                let _ = handle.tx.send(Command::Attach { ws, size: request.size, mode });
                self.live.insert(request.id, handle);
            }
            Err(error) => refuse(ws, request.lang.start_failed(&error)),
        }
    }

    /// Directory of another session's process (the focused shell), else home.
    async fn cwd_for(&self, from: Option<&str>) -> PathBuf {
        let Some(id) = from else { return home_dir() };
        let cwd = match self.live.get(id) {
            Some(handle) => {
                let (reply, answer) = oneshot::channel();
                let _ = handle.tx.send(Command::Cwd { reply });
                tokio::time::timeout(Duration::from_secs(1), answer).await.ok().and_then(Result::ok)
            }
            None => self.store.read_meta(id).map(|meta| meta.cwd),
        };
        cwd.filter(|cwd| cwd.is_dir()).unwrap_or_else(home_dir)
    }

    async fn save_all(&self) {
        let mut waits = Vec::new();
        for handle in self.live.values() {
            let (reply, answer) = oneshot::channel();
            if handle.tx.send(Command::SaveNow { reply }).is_ok() {
                waits.push(answer);
            }
        }
        let all = futures_util::future::join_all(waits);
        let _ = tokio::time::timeout(Duration::from_secs(3), all).await;
    }
}
