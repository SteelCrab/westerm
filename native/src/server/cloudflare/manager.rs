//! One tunnel at a time: starting cloudflared, waiting for its address,
//! serving the tunnel listener, watching the edge connection, and ending it
//! all in order. The panel on 4317 follows the `Status` this keeps.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use super::auth::{new_code, DeviceView};
use super::awake::{self, KeepAwake};
use super::cloudflared;
use super::router::{router, Admission, AdmissionEvent};
use crate::server::host_client::HostClient;
use crate::server::plugins::PluginState;
use crate::server::remote::RemoteManager;
use crate::server::{updates, AppState};

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Off,
    /// cloudflared runs; its address has not come yet.
    Starting,
    On,
    /// cloudflared runs but has no connection to Cloudflare: the address
    /// does not work for now. It reconnects by itself.
    Reconnecting,
    /// No cloudflared on this computer.
    Missing,
    Failed,
}

/// Why the last tunnel ended.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    Stopped,
    /// No device for the idle limit.
    Idle,
    TooManyCodes,
    /// No address within the start timeout.
    Timeout,
    /// cloudflared ended by itself.
    Exited,
    /// The tunnel listener could not open.
    Listen,
}

impl Reason {
    pub fn phase(self) -> Phase {
        match self {
            Reason::Stopped | Reason::Idle => Phase::Off,
            _ => Phase::Failed,
        }
    }
}

/// What the panel shows. Times are milliseconds since the epoch.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub phase: Phase,
    /// For the install command: "macos", "linux", or "windows".
    pub os: &'static str,
    /// `https://<name>.trycloudflare.com` while on or reconnecting.
    pub url: Option<String>,
    /// The access code while on or reconnecting.
    pub code: Option<String>,
    pub started_at: Option<u64>,
    pub devices: Vec<DeviceView>,
    /// When the tunnel stops for want of devices; None while one is connected.
    pub idle_deadline: Option<u64>,
    pub keep_awake: bool,
    pub reason: Option<Reason>,
    /// cloudflared's last lines, when it failed.
    pub log: Vec<String>,
    /// The tunnel listener's port, for tests and diagnosis.
    pub local_port: Option<u16>,
}

impl Status {
    pub fn idle(phase: Phase, reason: Option<Reason>) -> Self {
        Status {
            phase,
            os: os(),
            url: None,
            code: None,
            started_at: None,
            devices: Vec::new(),
            idle_deadline: None,
            keep_awake: false,
            reason,
            log: Vec::new(),
            local_port: None,
        }
    }
}

fn os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        _ => "linux",
    }
}

/// The computer stays awake while the tunnel can carry a device.
pub fn keeps_awake(phase: Phase) -> bool {
    matches!(phase, Phase::Starting | Phase::On | Phase::Reconnecting)
}

/// The tunnel stops at this time unless a device connects first.
pub fn idle_deadline(sockets: u32, last_activity_ms: u64, limit_ms: u64) -> Option<u64> {
    (sockets == 0).then(|| last_activity_ms + limit_ms)
}

pub const START_TIMEOUT: Duration = Duration::from_secs(30);
pub const READY_EVERY: Duration = Duration::from_secs(2);
pub const LOG_LINES: usize = 40;
pub const IDLE_LIMIT: Duration = Duration::from_secs(12 * 60 * 60);
/// How often to ask cloudflared for its address while starting.
const ADDRESS_EVERY: Duration = Duration::from_millis(250);

/// Tests shorten the limits; these are not for users.
fn seconds_from_env(name: &str, default: Duration) -> Duration {
    std::env::var(name).ok().and_then(|value| value.parse().ok()).map_or(default, Duration::from_secs)
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |time| time.as_millis() as u64)
}

fn find_cloudflared() -> Option<PathBuf> {
    cloudflared::find(std::env::var_os(cloudflared::ENV_OVERRIDE), std::env::var_os("PATH"), &cloudflared::common_dirs())
}

/// The sleep hold of the running tunnel.
#[derive(Default)]
struct Awake {
    hold: Option<KeepAwake>,
    /// Asked already in this tunnel. A computer that cannot hold (Linux
    /// without a working systemd-inhibit) is asked once, not on every update:
    /// each try there spawns a process and waits 300 ms.
    tried: bool,
}

impl Awake {
    /// Holds while `wanted`, asking `hold` once until `wanted` turns false. True while held.
    fn follow(&mut self, wanted: bool, hold: impl FnOnce() -> Option<KeepAwake>) -> bool {
        if !wanted {
            *self = Awake::default();
            return false;
        }
        if !self.tried {
            self.tried = true;
            self.hold = hold();
        }
        self.hold.is_some()
    }
}

/// A tunnel being run by its supervising task.
struct Running {
    stop: mpsc::UnboundedSender<Reason>,
    done: JoinHandle<()>,
}

pub(crate) struct TunnelManager {
    host: Arc<HostClient>,
    remote: Arc<RemoteManager>,
    /// The local dashboard's plugins: a pane a plugin started opens through the tunnel too.
    plugins: Arc<PluginState>,
    state_dir: PathBuf,
    status: watch::Sender<Status>,
    awake: Mutex<Awake>,
    /// One start or stop at a time.
    running: tokio::sync::Mutex<Option<Running>>,
}

impl TunnelManager {
    pub fn new(host: Arc<HostClient>, remote: Arc<RemoteManager>, plugins: Arc<PluginState>, state_dir: PathBuf) -> Arc<Self> {
        let phase = if find_cloudflared().is_some() { Phase::Off } else { Phase::Missing };
        Arc::new(Self {
            host,
            remote,
            plugins,
            state_dir,
            status: watch::channel(Status::idle(phase, None)).0,
            awake: Mutex::new(Awake::default()),
            running: tokio::sync::Mutex::new(None),
        })
    }

    pub fn watch(&self) -> watch::Receiver<Status> {
        self.status.subscribe()
    }

    /// Changes the status. The sleep hold follows the phase here, so every
    /// way a tunnel ends lets the computer sleep again.
    fn update(&self, change: impl FnOnce(&mut Status)) {
        self.status.send_if_modified(|status| {
            let before = status.clone();
            change(status);
            let mut awake = self.awake.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            status.keep_awake = awake.follow(keeps_awake(status.phase), awake::hold);
            *status != before
        });
    }

    fn set(&self, status: Status) {
        self.update(|current| *current = status);
    }

    /// Does nothing while a tunnel runs.
    pub async fn start(self: &Arc<Self>) {
        let mut running = self.running.lock().await;
        if running.as_ref().is_some_and(|run| !run.done.is_finished()) {
            return;
        }
        let Some(binary) = find_cloudflared() else {
            self.set(Status::idle(Phase::Missing, None));
            return;
        };
        self.set(Status::idle(Phase::Starting, None));
        let (stop, stops) = mpsc::unbounded_channel();
        let this = Arc::clone(self);
        let done = tokio::spawn(async move { this.supervise(binary, stops).await });
        *running = Some(Running { stop, done });
    }

    /// Ends the running tunnel, in the order of the spec (7.4), and waits for it.
    pub async fn stop(&self, reason: Reason) {
        let mut running = self.running.lock().await;
        if let Some(run) = running.take() {
            let _ = run.stop.send(reason);
            let _ = run.done.await;
        }
    }

    /// "Check again" after installing cloudflared.
    pub fn check(&self) {
        if matches!(self.status.borrow().phase, Phase::Off | Phase::Missing | Phase::Failed) {
            let phase = if find_cloudflared().is_some() { Phase::Off } else { Phase::Missing };
            self.set(Status::idle(phase, None));
        }
    }

    /// Runs one tunnel from start to end. Everything it starts, it ends.
    async fn supervise(self: Arc<Self>, binary: PathBuf, mut stops: mpsc::UnboundedReceiver<Reason>) {
        let listener = match tokio::net::TcpListener::bind(("127.0.0.1", 0)).await {
            Ok(listener) => listener,
            Err(_) => return self.set(Status::idle(Phase::Failed, Some(Reason::Listen))),
        };
        let Some(local_port) = listener.local_addr().ok().map(|address| address.port()) else {
            return self.set(Status::idle(Phase::Failed, Some(Reason::Listen)));
        };
        let launched = cloudflared::free_port().and_then(|metrics| {
            let config = cloudflared::write_empty_config(&self.state_dir)?;
            let guard = tokio::process::Command::new(std::env::current_exe()?)
                .arg("tunnel-guard")
                .arg("--")
                .arg(&binary)
                .args(cloudflared::args(metrics, local_port, &config))
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()?;
            Ok((metrics, guard))
        });
        let Ok((metrics, mut guard)) = launched else {
            return self.set(Status::idle(Phase::Failed, Some(Reason::Exited)));
        };
        let log = Arc::new(Mutex::new(VecDeque::with_capacity(LOG_LINES)));
        if let Some(stderr) = guard.stderr.take() {
            let log = Arc::clone(&log);
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let mut log = log.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    if log.len() == LOG_LINES {
                        log.pop_front();
                    }
                    log.push_back(line);
                }
            });
        }
        // tokio's Child::wait closes the child's stdin, which would end the
        // guard at once; the pipe is kept here instead.
        let keep_open = guard.stdin.take();
        let mut live = Live { guard, keep_open, log, server: None, admission: None, ended: None };

        // The address. The listener takes no request before it, so no Host is allowed yet.
        let deadline = tokio::time::Instant::now() + seconds_from_env("WESTERM_TUNNEL_START_SECS", START_TIMEOUT);
        let hostname = loop {
            tokio::select! {
                reason = stops.recv() => return self.finish(live, reason.unwrap_or(Reason::Stopped)).await,
                _ = live.guard.wait() => return self.finish(live, Reason::Exited).await,
                () = tokio::time::sleep(ADDRESS_EVERY) => {
                    if let Some(hostname) = cloudflared::hostname(metrics).await {
                        break hostname;
                    }
                    if tokio::time::Instant::now() > deadline {
                        return self.finish(live, Reason::Timeout).await;
                    }
                }
            }
        };

        let code = new_code();
        let (ended, ended_rx) = watch::channel(false);
        let (events_tx, mut events) = mpsc::unbounded_channel();
        let admission = Arc::new(Admission::new(hostname.clone(), code.clone(), ended_rx.clone(), events_tx));
        let url = format!("https://{hostname}");
        let state = Arc::new(AppState {
            host: Arc::clone(&self.host),
            origins: vec![url.clone()],
            state_dir: self.state_dir.clone(),
            updates: updates::UpdateCheck::default(),
            remote: Arc::clone(&self.remote),
            plugins: Arc::clone(&self.plugins),
            trusted: false,
            tunnel: None,
        });
        let app = router(state, Arc::clone(&admission));
        let mut shutdown = ended_rx;
        live.server = Some(tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async move { drop(shutdown.wait_for(|ended| *ended).await) })
                .await;
        }));
        live.admission = Some(Arc::clone(&admission));
        live.ended = Some(ended);

        let limit = seconds_from_env("WESTERM_TUNNEL_IDLE_SECS", IDLE_LIMIT).as_millis() as u64;
        let started = now_ms();
        let mut last_activity = started;
        self.set(Status {
            phase: Phase::On,
            url: Some(url.clone()),
            code: Some(code),
            started_at: Some(started),
            idle_deadline: idle_deadline(0, last_activity, limit),
            local_port: Some(local_port),
            ..Status::idle(Phase::On, None)
        });
        println!("> westerm: tunnel {url}");

        let mut checks = tokio::time::interval(READY_EVERY);
        let reason = loop {
            let wait = idle_deadline(admission.sockets(), last_activity, limit)
                .map_or(Duration::from_secs(24 * 60 * 60), |deadline| Duration::from_millis(deadline.saturating_sub(now_ms())));
            tokio::select! {
                reason = stops.recv() => break reason.unwrap_or(Reason::Stopped),
                _ = live.guard.wait() => break Reason::Exited,
                event = events.recv() => match event {
                    Some(AdmissionEvent::TooManyCodes) => break Reason::TooManyCodes,
                    Some(AdmissionEvent::Changed) | None => {
                        last_activity = now_ms();
                        let (devices, sockets) = (admission.devices(), admission.sockets());
                        self.update(|status| {
                            status.devices = devices;
                            status.idle_deadline = idle_deadline(sockets, last_activity, limit);
                        });
                    }
                },
                _ = checks.tick() => {
                    let phase = if cloudflared::ready(metrics).await == Some(true) { Phase::On } else { Phase::Reconnecting };
                    self.update(|status| status.phase = phase);
                }
                () = tokio::time::sleep(wait) => break Reason::Idle,
            }
        };
        self.finish(live, reason).await;
    }

    /// The end of a tunnel, in order: cookies, terminal sockets, the
    /// listener, cloudflared, then the status (which lets the computer sleep).
    async fn finish(&self, mut live: Live, reason: Reason) {
        if let Some(admission) = &live.admission {
            admission.clear();
        }
        if let Some(ended) = &live.ended {
            let _ = ended.send(true);
        }
        if let Some(mut server) = live.server.take()
            && tokio::time::timeout(Duration::from_secs(2), &mut server).await.is_err()
        {
            server.abort();
        }
        drop(live.keep_open.take());
        if tokio::time::timeout(Duration::from_secs(3), live.guard.wait()).await.is_err() {
            let _ = live.guard.kill().await;
        }
        let log = if reason.phase() == Phase::Failed {
            live.log.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).iter().cloned().collect()
        } else {
            Vec::new()
        };
        self.set(Status { log, ..Status::idle(reason.phase(), Some(reason)) });
    }
}

/// What a running tunnel holds, so `finish` can end each part.
struct Live {
    guard: tokio::process::Child,
    /// The guard's stdin, never written; closing it ends cloudflared.
    keep_open: Option<tokio::process::ChildStdin>,
    log: Arc<Mutex<VecDeque<String>>>,
    server: Option<JoinHandle<()>>,
    admission: Option<Arc<Admission>>,
    ended: Option<watch::Sender<bool>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn awake_only_while_the_tunnel_can_run() {
        for phase in [Phase::Starting, Phase::On, Phase::Reconnecting] {
            assert!(keeps_awake(phase), "{phase:?}");
        }
        for phase in [Phase::Off, Phase::Missing, Phase::Failed] {
            assert!(!keeps_awake(phase), "{phase:?}");
        }
    }

    #[test]
    fn awake_is_tried_once_per_tunnel() {
        // A computer that cannot hold (Linux without a working systemd-inhibit)
        // must not be asked again on every status update.
        let mut tries = 0;
        let mut awake = Awake::default();
        for _ in 0..3 {
            assert!(!awake.follow(true, || {
                tries += 1;
                None
            }));
        }
        assert_eq!(tries, 1);
        assert!(!awake.follow(false, || unreachable!("not wanted")));
        awake.follow(true, || {
            tries += 1;
            None
        });
        assert_eq!(tries, 2, "the next tunnel asks again");
    }

    #[test]
    fn idle_deadline_pauses_while_connected() {
        assert_eq!(idle_deadline(1, 1000, 500), None);
        assert_eq!(idle_deadline(0, 1000, 500), Some(1500));
    }

    #[test]
    fn reasons_end_in_off_or_failed() {
        assert_eq!(Reason::Stopped.phase(), Phase::Off);
        assert_eq!(Reason::Idle.phase(), Phase::Off);
        for reason in [Reason::TooManyCodes, Reason::Timeout, Reason::Exited, Reason::Listen] {
            assert_eq!(reason.phase(), Phase::Failed, "{reason:?}");
        }
    }

    #[test]
    fn status_json_shape() {
        let status = Status {
            phase: Phase::On,
            reason: Some(Reason::TooManyCodes),
            local_port: Some(5),
            ..Status::idle(Phase::Off, None)
        };
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json["phase"], "on");
        assert_eq!(json["reason"], "tooManyCodes");
        assert_eq!(json["localPort"], 5);
        for key in ["os", "url", "code", "startedAt", "devices", "idleDeadline", "keepAwake", "log"] {
            assert!(json.get(key).is_some(), "{key} in {json}");
        }
        assert!(["macos", "linux", "windows"].contains(&json["os"].as_str().unwrap()));
    }
}
