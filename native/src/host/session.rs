//! One session: a PTY process and at most one viewer (a browser pane through
//! the dashboard). Runs as a task that owns all of its state.
//!
//! Text sessions (shell, ssh) outlive their viewer: output keeps feeding the
//! host's screen copy, and a new viewer gets that screen replayed. Pixel apps
//! (terminal-browser, terminal-code) draw with Kitty images the copy cannot
//! keep, so they stop when their viewer leaves and restart on the next attach;
//! both restore their own tabs and folders.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use portable_pty::{ChildKiller, ExitStatus, MasterPty};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{sleep_until, Instant};

use super::store::{SessionMeta, SessionStore};
use crate::cwd::process_cwd;
use crate::i18n::Lang;
use crate::protocol::{
    AttachMode, ClientMessage, PtySize, ServerMessage, SessionKind, CLOSE_TAKEN_OVER, FLOW_HIGH_WATER, FLOW_LOW_WATER,
};
use crate::screen::{restored_notice, Screen};
use crate::spawn::{pty_size, spawn, LaunchSpec};

const SAVE_DELAY: Duration = Duration::from_secs(3);
/// A reattaching browser sends its fitted size first; wait this long for it.
const RESIZE_WAIT: Duration = Duration::from_millis(400);
const READ_CHUNK: usize = 64 * 1024;
const REPLAY_CHUNK: usize = 64 * 1024;

pub enum Command {
    Attach { ws: WebSocket, size: PtySize, mode: AttachMode },
    /// `forget`: the user closed the pane; also delete the saved screen.
    End { forget: bool },
    SaveNow { reply: oneshot::Sender<()> },
    Cwd { reply: oneshot::Sender<PathBuf> },
}

#[derive(Clone)]
pub struct Handle {
    pub kind: SessionKind,
    /// Distinguishes this run from a later session with the same id.
    pub serial: u64,
    pub tx: mpsc::UnboundedSender<Command>,
    alive: Arc<AtomicBool>,
}

impl Handle {
    pub fn alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }
}

pub struct Restored {
    pub screen: String,
    pub saved_at: String,
    /// Language of the restore notice.
    pub lang: Lang,
}

enum ViewerEvent {
    Text(String),
    Binary(Bytes),
    Closed,
}

struct Viewer {
    generation: u64,
    out: mpsc::UnboundedSender<Message>,
}

impl Viewer {
    fn send(&self, message: Message) {
        let _ = self.out.send(message);
    }

    fn close(&self, code: u16, reason: &'static str) {
        self.send(Message::Close(Some(CloseFrame { code, reason: reason.into() })));
    }
}

#[derive(PartialEq)]
enum Flow {
    Continue,
    Stop,
}

struct Session {
    id: String,
    kind: SessionKind,
    url: Option<String>,
    target: Option<String>,
    cwd: PathBuf,
    pid: u32,
    store: Arc<SessionStore>,
    screen: Option<Screen>,
    master: Box<dyn MasterPty + Send>,
    input: std::sync::mpsc::Sender<Vec<u8>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    viewer: Option<Viewer>,
    generation: u64,
    events: mpsc::UnboundedSender<(u64, ViewerEvent)>,
    /// Live output held while a reattach replay is being prepared.
    held: Option<Vec<Bytes>>,
    /// Waiting for the viewer's fitted size before replaying.
    awaiting: Option<(PtySize, Instant)>,
    in_flight: u64,
    paused: bool,
    save_at: Option<Instant>,
    forgotten: bool,
    alive: Arc<AtomicBool>,
    serial: u64,
    ended: mpsc::UnboundedSender<(String, u64)>,
}

pub struct Spec {
    pub id: String,
    pub launch: LaunchSpec,
}

pub fn start(
    spec: Spec,
    size: PtySize,
    store: Arc<SessionStore>,
    restored: Option<Restored>,
    serial: u64,
    ended: mpsc::UnboundedSender<(String, u64)>,
) -> Result<Handle, String> {
    let kind = spec.launch.kind;
    let screen = kind.is_text().then(|| {
        let mut screen = Screen::new(size);
        // The old screen goes in before the new shell prints its prompt.
        if let Some(restored) = &restored {
            screen.process(restored.screen.as_bytes());
            screen.process(restored_notice(&restored.saved_at, restored.lang).as_bytes());
        }
        screen
    });

    let mut pty = spawn(&spec.launch, size)?;
    let mut reader = pty.master.try_clone_reader().map_err(|e| e.to_string())?;
    let mut writer = pty.master.take_writer().map_err(|e| e.to_string())?;
    let killer = pty.child.clone_killer();

    // Blocking PTY reads on a thread. The bounded channel is the flow
    // control: when the session stops taking chunks, the reader blocks and
    // the process blocks on write.
    let (out_tx, out_rx) = mpsc::channel::<Bytes>(32);
    std::thread::spawn(move || {
        let mut buf = vec![0u8; READ_CHUNK];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || out_tx.blocking_send(Bytes::copy_from_slice(&buf[..n])).is_err() {
                break;
            }
        }
    });
    let (input_tx, input_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        for data in input_rx {
            if writer.write_all(&data).and_then(|()| writer.flush()).is_err() {
                break;
            }
        }
    });
    let (exit_tx, exit_rx) = oneshot::channel::<ExitStatus>();
    std::thread::spawn(move || {
        if let Ok(status) = pty.child.wait() {
            let _ = exit_tx.send(status);
        }
    });

    let alive = Arc::new(AtomicBool::new(true));
    let (tx, rx) = mpsc::unbounded_channel();
    let (events, events_rx) = mpsc::unbounded_channel();
    let session = Session {
        id: spec.id,
        kind,
        url: spec.launch.url,
        target: spec.launch.target,
        cwd: spec.launch.cwd,
        pid: pty.pid,
        store,
        screen,
        master: pty.master,
        input: input_tx,
        killer,
        viewer: None,
        generation: 0,
        events,
        held: None,
        awaiting: None,
        in_flight: 0,
        paused: false,
        save_at: None,
        forgotten: false,
        alive: alive.clone(),
        serial,
        ended,
    };
    session.write_meta();
    tokio::spawn(session.run(rx, events_rx, out_rx, exit_rx));
    Ok(Handle { kind, serial, tx, alive })
}

impl Session {
    async fn run(
        mut self,
        mut commands: mpsc::UnboundedReceiver<Command>,
        mut events: mpsc::UnboundedReceiver<(u64, ViewerEvent)>,
        mut output: mpsc::Receiver<Bytes>,
        mut exit: oneshot::Receiver<ExitStatus>,
    ) {
        let mut output_open = true;
        loop {
            let save_at = self.save_at;
            let replay_at = self.awaiting.map(|(_, at)| at);
            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else {
                        self.kill();
                        break;
                    };
                    if self.command(command) == Flow::Stop {
                        break;
                    }
                }
                Some((generation, event)) = events.recv() => {
                    if self.viewer_event(generation, event) == Flow::Stop {
                        break;
                    }
                }
                chunk = output.recv(), if output_open && !self.paused => match chunk {
                    Some(bytes) => self.output(bytes),
                    None => output_open = false,
                },
                status = &mut exit => {
                    // Output still queued belongs before the exit message.
                    while let Ok(bytes) = output.try_recv() {
                        self.output(bytes);
                    }
                    self.exited(status.ok());
                    break;
                }
                () = async { sleep_until(save_at.unwrap()).await }, if save_at.is_some() => {
                    self.save_at = None;
                    self.save();
                }
                () = async { sleep_until(replay_at.unwrap()).await }, if replay_at.is_some() => {
                    let size = self.awaiting.map(|(size, _)| size).unwrap();
                    self.replay(size);
                }
            }
        }
    }

    fn command(&mut self, command: Command) -> Flow {
        match command {
            Command::Attach { ws, size, mode } => {
                self.attach(ws, size, mode);
                Flow::Continue
            }
            Command::End { forget } => self.end(forget),
            Command::SaveNow { reply } => {
                self.save();
                let _ = reply.send(());
                Flow::Continue
            }
            Command::Cwd { reply } => {
                self.refresh_cwd();
                let _ = reply.send(self.cwd.clone());
                Flow::Continue
            }
        }
    }

    fn attach(&mut self, ws: WebSocket, size: PtySize, mode: AttachMode) {
        if let Some(old) = self.viewer.take() {
            old.close(CLOSE_TAKEN_OVER, "opened in another window");
        }
        self.generation += 1;
        let generation = self.generation;
        let (mut sink, mut stream) = ws.split();
        let (out, mut out_rx) = mpsc::unbounded_channel::<Message>();
        tokio::spawn(async move {
            while let Some(message) = out_rx.recv().await {
                let closing = matches!(message, Message::Close(_));
                if sink.send(message).await.is_err() || closing {
                    break;
                }
            }
            let _ = sink.close().await;
        });
        let events = self.events.clone();
        tokio::spawn(async move {
            while let Some(Ok(message)) = stream.next().await {
                let event = match message {
                    Message::Text(text) => ViewerEvent::Text(text.to_string()),
                    Message::Binary(bytes) => ViewerEvent::Binary(bytes),
                    Message::Close(_) => break,
                    _ => continue,
                };
                if events.send((generation, event)).is_err() {
                    return;
                }
            }
            let _ = events.send((generation, ViewerEvent::Closed));
        });

        self.viewer = Some(Viewer { generation, out });
        self.in_flight = 0;
        self.paused = false;
        self.send_json(&ServerMessage::Ready { pid: self.pid, mode });
        if mode != AttachMode::New && self.screen.is_some() {
            self.held = Some(Vec::new());
            self.awaiting = Some((size, Instant::now() + RESIZE_WAIT));
        }
    }

    fn viewer_event(&mut self, generation: u64, event: ViewerEvent) -> Flow {
        if self.viewer.as_ref().is_none_or(|viewer| viewer.generation != generation) {
            return Flow::Continue; // From a replaced viewer.
        }
        match event {
            ViewerEvent::Binary(bytes) => {
                // Raw X10 mouse reports.
                let _ = self.input.send(bytes.to_vec());
            }
            ViewerEvent::Text(text) => match ClientMessage::parse(&text) {
                Some(ClientMessage::Input { data }) => {
                    let _ = self.input.send(data.into_bytes());
                }
                Some(ClientMessage::Resize { cols, rows, width, height }) => {
                    let size = PtySize { cols, rows, width, height };
                    if self.awaiting.is_some() {
                        self.replay(size);
                    } else {
                        self.resize(size);
                    }
                }
                Some(ClientMessage::Ack { bytes }) => {
                    self.in_flight = self.in_flight.saturating_sub(bytes);
                    if self.in_flight <= FLOW_LOW_WATER {
                        self.paused = false;
                    }
                }
                Some(ClientMessage::Close) => return self.end(true),
                None => {}
            },
            ViewerEvent::Closed => {
                self.viewer = None;
                self.held = None;
                self.awaiting = None;
                self.paused = false;
                if self.kind.is_text() {
                    self.save_at = Some(Instant::now());
                } else {
                    self.kill();
                }
            }
        }
        Flow::Continue
    }

    fn output(&mut self, bytes: Bytes) {
        if let Some(screen) = &mut self.screen {
            screen.process(&bytes);
            self.save_at.get_or_insert_with(|| Instant::now() + SAVE_DELAY);
        }
        let Some(viewer) = &self.viewer else { return };
        self.in_flight += bytes.len() as u64;
        match &mut self.held {
            Some(held) => held.push(bytes),
            None => viewer.send(Message::Binary(bytes)),
        }
        if self.in_flight >= FLOW_HIGH_WATER {
            self.paused = true;
        }
    }

    fn replay(&mut self, size: PtySize) {
        self.awaiting = None;
        self.resize(size);
        let Some(viewer) = &self.viewer else { return };
        if let Some(screen) = &self.screen {
            let bytes = screen.replay();
            for chunk in bytes.chunks(REPLAY_CHUNK) {
                viewer.send(Message::Binary(Bytes::copy_from_slice(chunk)));
            }
            self.in_flight += bytes.len() as u64;
        }
        for bytes in self.held.take().unwrap_or_default() {
            viewer.send(Message::Binary(bytes));
        }
    }

    fn resize(&mut self, size: PtySize) {
        if let Some(screen) = &mut self.screen {
            screen.resize(size);
        }
        let _ = self.master.resize(pty_size(size));
    }

    fn end(&mut self, forget: bool) -> Flow {
        if !forget {
            self.kill(); // The exit that follows saves and reports.
            return Flow::Continue;
        }
        self.forgotten = true;
        self.save_at = None;
        self.store.remove(&self.id);
        self.kill();
        if let Some(viewer) = self.viewer.take() {
            viewer.close(1000, "closed");
        }
        self.finish();
        Flow::Stop
    }

    fn exited(&mut self, status: Option<ExitStatus>) {
        if !self.forgotten {
            self.save();
        }
        let code = status.map(|status| i64::from(status.exit_code())).unwrap_or(-1);
        self.send_json(&ServerMessage::Exit { code, signal: 0 });
        if let Some(viewer) = self.viewer.take() {
            viewer.close(1000, "process exited");
        }
        self.finish();
    }

    fn finish(&mut self) {
        self.alive.store(false, Ordering::Relaxed);
        let _ = self.ended.send((self.id.clone(), self.serial));
    }

    fn kill(&mut self) {
        let _ = self.killer.kill();
    }

    fn refresh_cwd(&mut self) {
        if let Some(cwd) = process_cwd(self.pid) {
            self.cwd = cwd;
        }
    }

    fn save(&mut self) {
        self.refresh_cwd();
        self.write_meta();
        if let Some(snapshot) = self.screen.as_ref().and_then(Screen::snapshot) {
            if let Err(error) = self.store.write_screen(&self.id, &snapshot) {
                eprintln!("session {}: screen save failed: {error}", self.id);
            }
        }
    }

    fn write_meta(&self) {
        let meta = SessionMeta {
            version: 1,
            id: self.id.clone(),
            kind: self.kind,
            url: self.url.clone(),
            target: self.target.clone(),
            cwd: self.cwd.clone(),
            saved_at: crate::time::now_iso(),
        };
        if let Err(error) = self.store.write_meta(&meta) {
            eprintln!("session {}: save failed: {error}", self.id);
        }
    }

    fn send_json(&self, message: &ServerMessage) {
        if let Some(viewer) = &self.viewer {
            viewer.send(Message::Text(message.to_json().into()));
        }
    }
}
