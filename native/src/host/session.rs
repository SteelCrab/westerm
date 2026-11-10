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
use tokio::time::{sleep_until, Instant, MissedTickBehavior};

use super::agent::{self, AgentRecord, Tracker};
use super::store::{SessionMeta, SessionStore};
use crate::cwd::process_cwd;
use crate::git;
use crate::i18n::Lang;
use crate::pixels::{PixelStream, KIND_TEXT};
use crate::protocol::{
    AttachMode, ClientMessage, Launch, PixelCodec, Place, PtySize, ServerMessage, SessionKind, CLOSE_TAKEN_OVER,
    FLOW_HIGH_WATER, FLOW_LOW_WATER,
};
use crate::screen::{agent_notice, restored_notice, Screen};
use crate::spawn::{pty_size, spawn, LaunchSpec};

const SAVE_DELAY: Duration = Duration::from_secs(3);
/// A reattaching browser sends its fitted size first; wait this long for it.
const RESIZE_WAIT: Duration = Duration::from_millis(400);
const READ_CHUNK: usize = 64 * 1024;
const REPLAY_CHUNK: usize = 64 * 1024;
/// How often a shell looks for an AI agent in its foreground.
const AGENT_POLL: Duration = Duration::from_secs(1);
/// Rows above the last text where agents show their status line and questions.
const AGENT_ROWS: usize = 16;
/// A restored shell gets its agent's resume command once it printed and then
/// stayed quiet this long (its prompt is up)…
const RESUME_QUIET: Duration = Duration::from_millis(300);
/// …or this long after it started, whichever comes first.
const RESUME_LATEST: Duration = Duration::from_secs(5);

pub enum Command {
    /// `window`: the page the viewer is in.
    Attach { ws: WebSocket, size: PtySize, mode: AttachMode, window: Option<String> },
    /// Window `owner` took every session: close a viewer in any other window.
    Evict { owner: String },
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

/// A revived shell that ran an AI agent: it resumes the agent's conversation.
pub struct Resuming {
    pub record: AgentRecord,
    /// Language of the notice line.
    pub lang: Lang,
}

/// The program last seen in the foreground, and the agent it is. Its command
/// line is read until it reads the same a few times: right after a fork the
/// child still shows the shell's, until it execs the program.
struct Foreground {
    pid: u32,
    record: Option<AgentRecord>,
    reads: u8,
}

const SETTLED_READS: u8 = 3;

/// A command waiting for the new shell's prompt: the agent's resume command,
/// or the launch of a new session.
struct Resume {
    input: Vec<u8>,
    at: Instant,
    latest: Instant,
}

enum ViewerEvent {
    Text(String),
    Binary(Bytes),
    Closed,
}

struct Viewer {
    generation: u64,
    out: mpsc::UnboundedSender<Message>,
    window: Option<String>,
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
    /// The AI agent in a shell's foreground (macOS and Linux).
    agent: Tracker,
    foreground: Option<Foreground>,
    resume: Option<Resume>,
    /// The folder and branch last sent (or about to be sent) to the viewer.
    place: Option<Place>,
    /// A pixel app whose viewer takes pixel updates.
    pixels: Option<Pixels>,
}

/// Most pixel updates sent and not yet drawn. Later frames wait and merge,
/// so a slow page gets fewer, newer frames instead of a backlog.
const PIXELS_IN_FLIGHT: u8 = 2;

struct Pixels {
    stream: PixelStream,
    codec: PixelCodec,
    in_flight: u8,
}

pub struct Spec {
    pub id: String,
    pub launch: LaunchSpec,
    /// The viewer takes pixel updates in this codec (pixel apps only).
    pub pixels: Option<PixelCodec>,
    /// An agent to start in a new shell once its prompt is up.
    pub agent: Option<Launch>,
    /// A plugin command to type into a new shell once its prompt is up,
    /// already quoted for that shell.
    pub plugin_command: Option<String>,
}

pub fn start(
    spec: Spec,
    size: PtySize,
    store: Arc<SessionStore>,
    restored: Option<Restored>,
    resuming: Option<Resuming>,
    serial: u64,
    ended: mpsc::UnboundedSender<(String, u64)>,
) -> Result<Handle, String> {
    let kind = spec.launch.kind;
    // A pixel app restarts on every attach, so its one viewer is known here.
    // Windows ConPTY drops Kitty graphics, so nothing would arrive.
    let pixels = spec
        .pixels
        .filter(|_| !kind.is_text() && cfg!(unix))
        .map(|codec| Pixels { stream: PixelStream::new(size), codec, in_flight: 0 });
    let screen = kind.is_text().then(|| {
        let mut screen = Screen::new(size);
        // The old screen goes in before the new shell prints its prompt.
        if let Some(restored) = &restored {
            screen.process(restored.screen.as_bytes());
            screen.process(restored_notice(&restored.saved_at, restored.lang).as_bytes());
        }
        if let Some(Resuming { record, lang }) = &resuming {
            screen.process(agent_notice(record.kind.label(), record.session.as_deref(), *lang).as_bytes());
        }
        screen
    });
    // What the shell types once its prompt is up, and the agent it then runs.
    // A launch has no restore notice: the user just asked for it.
    // A plugin command starts no agent, so it has no record. A command that
    // cannot be quoted (a control character) is not typed.
    let shell = agent::resume_shell();
    let (command, record) = match (resuming, &spec.agent, spec.plugin_command) {
        (Some(resuming), _, _) => (resuming.record.resume_command(shell), Some(resuming.record)),
        (None, Some(launch), _) => (agent::launch_command(launch, shell), agent::launch_record(launch)),
        (None, None, Some(command)) => (Some(command), None),
        (None, None, None) => (None, None),
    };
    let latest = Instant::now() + RESUME_LATEST;
    let resume = command.map(|command| Resume { input: format!("{command}\r").into_bytes(), at: latest, latest });
    let agent = record.map(Tracker::resuming).unwrap_or_default();

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
        agent,
        foreground: None,
        resume,
        place: None,
        pixels,
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
        let watches_agents = self.kind == SessionKind::Shell && cfg!(unix);
        let mut agent_poll = tokio::time::interval(AGENT_POLL);
        agent_poll.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            let save_at = self.save_at;
            let replay_at = self.awaiting.map(|(_, at)| at);
            let resume_at = self.resume.as_ref().map(|resume| resume.at);
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
                () = async { sleep_until(resume_at.unwrap()).await }, if resume_at.is_some() => {
                    if let Some(resume) = self.resume.take() {
                        let _ = self.input.send(resume.input);
                    }
                }
                _ = agent_poll.tick(), if watches_agents => self.poll_agent(),
            }
        }
    }

    fn command(&mut self, command: Command) -> Flow {
        match command {
            Command::Attach { ws, size, mode, window } => {
                self.attach(ws, size, mode, window);
                Flow::Continue
            }
            Command::Evict { owner } => {
                if let Some(viewer) = self.viewer.as_ref().filter(|viewer| viewer.window.as_ref() != Some(&owner)) {
                    viewer.close(CLOSE_TAKEN_OVER, "opened in another window");
                    self.detached();
                }
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

    fn attach(&mut self, ws: WebSocket, size: PtySize, mode: AttachMode, window: Option<String>) {
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

        self.viewer = Some(Viewer { generation, out, window });
        self.in_flight = 0;
        self.paused = false;
        self.send_json(&ServerMessage::Ready { pid: self.pid, mode, pixels: self.pixels.is_some() });
        if self.kind == SessionKind::Shell {
            self.send_agent();
            self.refresh_place();
            self.send_place();
        }
        if let Some(pixels) = &mut self.pixels {
            pixels.in_flight = 0;
        }
        // A frame the app drew before this viewer came.
        self.send_pixels();
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
                Some(ClientMessage::Painted) => {
                    if let Some(pixels) = &mut self.pixels {
                        pixels.in_flight = pixels.in_flight.saturating_sub(1);
                    }
                    self.send_pixels();
                }
                Some(ClientMessage::Close) => return self.end(true),
                None => {}
            },
            ViewerEvent::Closed => self.detached(),
        }
        Flow::Continue
    }

    /// The viewer left or was closed: a text session saves soon and keeps
    /// running; a pixel app stops.
    fn detached(&mut self) {
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

    fn output(&mut self, bytes: Bytes) {
        if let Some(screen) = &mut self.screen {
            screen.process(&bytes);
            self.save_at.get_or_insert_with(|| Instant::now() + SAVE_DELAY);
        }
        if let Some(resume) = &mut self.resume {
            resume.at = (Instant::now() + RESUME_QUIET).min(resume.latest);
        }
        // Graphics stay here; the rest goes on behind a kind byte. The app's
        // questions get answered even before a viewer comes.
        let (message, text_len) = match &mut self.pixels {
            Some(pixels) => {
                let fed = pixels.stream.feed(&bytes);
                if !fed.reply.is_empty() {
                    let _ = self.input.send(fed.reply);
                }
                let text_len = fed.text.len();
                let mut message = Vec::with_capacity(1 + text_len);
                message.push(KIND_TEXT);
                message.extend(fed.text);
                (Bytes::from(message), text_len)
            }
            None => {
                let len = bytes.len();
                (bytes, len)
            }
        };
        if let Some(viewer) = &self.viewer
            && text_len > 0
        {
            // The page acknowledges the bytes it parses, without the kind byte.
            self.in_flight += text_len as u64;
            match &mut self.held {
                Some(held) => held.push(message),
                None => viewer.send(Message::Binary(message)),
            }
            if self.in_flight >= FLOW_HIGH_WATER {
                self.paused = true;
            }
        }
        self.send_pixels();
    }

    /// Sends the newest finished frame's changes, unless the page is still
    /// drawing earlier ones. The PTY keeps flowing meanwhile: frames merge.
    fn send_pixels(&mut self) {
        let (Some(viewer), Some(pixels)) = (&self.viewer, &mut self.pixels) else { return };
        if pixels.in_flight >= PIXELS_IN_FLIGHT {
            return;
        }
        if let Some(update) = pixels.stream.take_update(pixels.codec) {
            viewer.send(Message::Binary(update.into()));
            pixels.in_flight += 1;
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
        if let Some(pixels) = &mut self.pixels {
            pixels.stream.resize(size);
        }
        let _ = self.master.resize(pty_size(size));
        self.send_pixels();
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

    fn poll_agent(&mut self) {
        let seen = self.foreground_agent();
        let changes = self.agent.observe(seen, crate::time::now_millis());
        if changes.record {
            self.write_meta();
        }
        if changes.state {
            self.send_agent();
        }
        if self.refresh_place() {
            self.send_place();
        }
    }

    /// Reads the shell's folder and branch. True when they differ from the
    /// last read; a folder that cannot be read keeps the old place.
    fn refresh_place(&mut self) -> bool {
        let Some(cwd) = process_cwd(self.pid) else { return false };
        let place = Place { branch: git::branch(&cwd), cwd: cwd.to_string_lossy().into_owned() };
        if self.place.as_ref() == Some(&place) {
            return false;
        }
        self.place = Some(place);
        true
    }

    /// The program in the foreground of the shell's terminal, when it is an
    /// AI agent.
    fn foreground_agent(&mut self) -> Option<agent::Seen> {
        let Some(leader) = self.foreground_program() else {
            self.foreground = None;
            return None;
        };
        let settled = self.foreground.as_ref().is_some_and(|seen| seen.pid == leader && seen.reads >= SETTLED_READS);
        if !settled {
            let record = crate::process::process_args(leader).and_then(|argv| agent::identify(&argv));
            let reads = match &self.foreground {
                Some(seen) if seen.pid == leader && seen.record == record => seen.reads + 1,
                _ => 1,
            };
            self.foreground = Some(Foreground { pid: leader, record, reads });
        }
        let record = self.foreground.as_ref()?.record.clone()?;
        let report = (record.kind == crate::protocol::AgentKind::Claude)
            .then(|| agent::read_claude_report(agent::claude_home(), leader))
            .flatten();
        let last = self.screen.as_ref().map(|screen| screen.last_text(AGENT_ROWS)).unwrap_or_default();
        let cwd = process_cwd(leader).map(|cwd| cwd.to_string_lossy().into_owned());
        Some(agent::look(record, leader, cwd, report, &last))
    }

    /// The leader of the terminal's foreground process group, unless that is
    /// the shell itself.
    #[cfg(unix)]
    fn foreground_program(&self) -> Option<u32> {
        let leader = u32::try_from(self.master.process_group_leader()?).ok()?;
        (leader != self.pid).then_some(leader)
    }

    /// ConPTY has no foreground process group.
    #[cfg(not(unix))]
    fn foreground_program(&self) -> Option<u32> {
        None
    }

    fn send_agent(&self) {
        self.send_json(&ServerMessage::Agent { agent: self.agent.state().cloned() });
    }

    fn send_place(&self) {
        self.send_json(&ServerMessage::Place { place: self.place.clone() });
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
            agent: self.agent.record().cloned(),
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
