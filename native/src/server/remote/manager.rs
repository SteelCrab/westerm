//! One link per server, shared by all its windows. While at least one remote
//! window watches a server's link (`watch`), a driver task for that server
//! keeps the link up: it prepares the server over SSH when needed (check,
//! install, start, maybe ask to replace an older host), publishes each
//! stage to every watching window, and reconnects by the retry rules below.
//! When the last window leaves, the link goes after `linger()`. Requests
//! only use a live link; they never connect themselves.

use std::collections::HashMap;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{oneshot, watch, Notify, OnceCell};
use tokio_tungstenite::tungstenite::Message;

use super::binary::{self, InstallPlan};
use super::punch::{self, Candidate, Ways, Won, DIRECT_GRACE, PUNCH_HANDSHAKE_TIMEOUT};
use super::quic::{self, QuicLink};
use super::ssh::{self, Probe, Resolved};
use super::stage::{LinkStage, QuicPath};
#[cfg(unix)]
use super::tunnel::{self, Tunnel};
use super::{records, Failure, HostStream};
use crate::host::HOST_BUILD;
use crate::protocol::{is_id, RemoteFailure, RemoteStage, RemoteVia, PROTOCOL};
use crate::remote::identity::{self, Identity};
use crate::remote::punch::{PunchAnswer, PunchRequest};
use crate::remote::report::{parse_report, HostInfo, SetupReport};
use crate::remote::stun::{self, StunConfig};
use crate::remote::{remote_dir, HANDSHAKE_TIMEOUT};
use crate::update;

/// A saved gate gets this long before the full SSH setup runs instead.
const FAST_PATH_TIMEOUT: Duration = Duration::from_secs(2);
/// After a failed SSH setup, the next one waits this long. A server that
/// bans repeated logins (fail2ban and the like) must not see one every few
/// seconds; the saved gate is tried again much more often.
const SSH_RETRY_INTERVAL: Duration = Duration::from_secs(30);
/// How long a link stays after the last window leaves, so a reloaded window
/// finds it still up.
const LINGER: Duration = Duration::from_secs(30);
/// How often the driver checks that a live link is still alive.
const LINK_CHECK: Duration = Duration::from_secs(1);

/// The wait after `failures + 1` transient failures in a row: 1, 2, 4, 8 s,
/// then 8 s.
pub fn retry_delay(failures: u32) -> Duration {
    Duration::from_secs(1 << failures.min(3))
}

/// The SSH setup may run again: it never failed, or failed
/// `SSH_RETRY_INTERVAL` ago or more.
pub fn ssh_due(last_failed_ssh: Option<Instant>, now: Instant) -> bool {
    last_failed_ssh.is_none_or(|failed| now.saturating_duration_since(failed) >= SSH_RETRY_INTERVAL)
}

/// What a window sees while SSH is not due: the last SSH failure, so the
/// cause and its remedy stay on screen, with the wait added. A permanent one
/// stays as it was: the driver halts on it, and Retry runs SSH at once.
pub fn waiting_failure(last: &Failure, wait_secs: u64) -> Failure {
    if last.reason.is_permanent() {
        return last.clone();
    }
    let wait = format!("the next SSH check is in {wait_secs} s");
    let detail = if last.detail.is_empty() { wait } else { format!("{}; {wait}", last.detail) };
    Failure::new(last.reason, detail)
}

/// `LINGER`, or WESTERM_REMOTE_LINGER_MS (for tests).
fn linger() -> Duration {
    std::env::var("WESTERM_REMOTE_LINGER_MS").ok().and_then(|ms| ms.parse().ok()).map_or(LINGER, Duration::from_millis)
}

#[derive(Debug, PartialEq, Eq)]
pub enum HostPlan {
    /// The running host is this build, or one the user chose to keep.
    Use,
    /// Another build with no shells: replacing it loses nothing.
    Replace,
    /// Another build with shells: replacing it ends their programs.
    Ask { sessions: usize },
    /// The user kept an older host that speaks another protocol.
    Incompatible,
}

pub fn host_plan(host: &HostInfo, local_build: &str, declined: bool) -> HostPlan {
    if host.build == local_build {
        HostPlan::Use
    } else if host.sessions == 0 {
        HostPlan::Replace
    } else if declined {
        if host.protocol == PROTOCOL { HostPlan::Use } else { HostPlan::Incompatible }
    } else {
        HostPlan::Ask { sessions: host.sessions }
    }
}

/// Opens streams on a link; cheap to clone into a task.
#[derive(Clone)]
enum Opener {
    Quic(quinn::Connection),
    #[cfg(unix)]
    Tunnel(PathBuf),
}

impl Opener {
    async fn open(&self) -> io::Result<HostStream> {
        match self {
            Self::Quic(connection) => quic::open(connection).await,
            #[cfg(unix)]
            Self::Tunnel(socket) => tunnel::open(socket).await,
        }
    }
}

enum Link {
    Quic(QuicLink),
    #[cfg(unix)]
    Tunnel(Tunnel),
}

impl Link {
    fn opener(&self) -> Opener {
        match self {
            Self::Quic(link) => Opener::Quic(link.opener()),
            #[cfg(unix)]
            Self::Tunnel(tunnel) => Opener::Tunnel(tunnel.socket().to_path_buf()),
        }
    }

    fn is_alive(&mut self) -> bool {
        match self {
            Self::Quic(link) => link.is_alive(),
            #[cfg(unix)]
            Self::Tunnel(tunnel) => tunnel.is_alive(),
        }
    }
}

struct Slot {
    link: Link,
    /// Tells this link from a later one, so a pane cannot drop a newer link.
    id: u64,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct Target {
    slot: tokio::sync::Mutex<Option<Slot>>,
    stages: watch::Sender<Option<LinkStage>>,
    decision: Mutex<Option<oneshot::Sender<bool>>>,
    /// The saved gate failed: skip the fast path once.
    stale: AtomicBool,
    /// The windows watching the link: live `Watch` values.
    watchers: AtomicUsize,
    /// Wakes the driver: the watchers changed, or the link was dropped.
    wake: Notify,
    /// A driver task runs for this target. Only one ever does.
    driving: AtomicBool,
    /// Set while the driver waits on a permanent failure; `retry` sends on it.
    retry: Mutex<Option<oneshot::Sender<()>>>,
    /// When the SSH setup last failed, and how. A success or a retry clears it.
    last_failed_ssh: Mutex<Option<(Instant, Failure)>>,
}

impl Target {
    fn new() -> Self {
        Self {
            slot: tokio::sync::Mutex::new(None),
            stages: watch::channel(None).0,
            decision: Mutex::new(None),
            stale: AtomicBool::new(false),
            watchers: AtomicUsize::new(0),
            wake: Notify::new(),
            driving: AtomicBool::new(false),
            retry: Mutex::new(None),
            last_failed_ssh: Mutex::new(None),
        }
    }

    fn watched(&self) -> bool {
        self.watchers.load(Ordering::SeqCst) > 0
    }

    /// Returns once no window watches. (`notify_one` keeps a wake-up that
    /// comes before the wait, so none is lost between check and wait.)
    async fn until_unwatched(&self) {
        while self.watched() {
            self.wake.notified().await;
        }
    }

    /// Waits up to `wait` for a window to watch; true when one does.
    async fn watched_within(&self, wait: Duration) -> bool {
        let deadline = tokio::time::Instant::now() + wait;
        while !self.watched() {
            if tokio::time::timeout_at(deadline, self.wake.notified()).await.is_err() {
                return self.watched();
            }
        }
        true
    }

    /// Drops `link` if it is still the current one, and wakes the driver to
    /// make a new one. `stale`: the next one comes from the full SSH setup.
    async fn drop_link(&self, link: u64, stale: bool) {
        let mut slot = self.slot.lock().await;
        if slot.as_ref().is_some_and(|current| current.id == link) {
            *slot = None;
            if stale {
                self.stale.store(true, Ordering::Relaxed);
            }
            self.wake.notify_one();
        }
    }

    /// No window watches any more: the link goes, and the driver stops.
    async fn let_go(&self) {
        *self.slot.lock().await = None;
        self.stages.send_replace(None);
        self.driving.store(false, Ordering::SeqCst);
    }

    /// After `let_go`: a window came meanwhile, and no new driver has
    /// started for it, so this one carries on.
    fn resume(&self) -> bool {
        self.watched() && !self.driving.swap(true, Ordering::SeqCst)
    }
}

/// A window watching a server's link. While one exists, the link is kept
/// up; `stages` shows where it is, the current stage first.
pub struct Watch {
    pub stages: watch::Receiver<Option<LinkStage>>,
    target: Arc<Target>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.target.watchers.fetch_sub(1, Ordering::SeqCst);
        self.target.wake.notify_one();
    }
}

/// What a target's driver does next.
enum Next {
    /// Make a link: the saved gate, then the SSH setup when it is due.
    Connect { failures: u32 },
    /// After a transient failure: wait `retry_delay(failures)`, then connect.
    Backoff { failures: u32 },
    /// After a permanent failure: wait for `retry`.
    Halted,
    /// The link is up: check it every second.
    Up,
}

/// A stream to a remote host, and the link it came from.
pub struct Opened {
    pub stream: HostStream,
    pub link: u64,
}

pub struct RemoteManager {
    state_dir: PathBuf,
    targets: Mutex<HashMap<String, Arc<Target>>>,
    identity: OnceCell<Arc<Identity>>,
    links: AtomicU64,
}

fn publish(target: &Target, stage: impl Into<LinkStage>) {
    target.stages.send_replace(Some(stage.into()));
}

fn setup_failure(detail: impl Into<String>) -> Failure {
    Failure::new(RemoteFailure::Setup, detail)
}

/// A new link, and the "connected" stage to show once it is in the slot
/// (a window that sees it may use the link at once).
type Made = (Link, LinkStage);

/// Socket A, or why there is none. None: hole punching is not allowed
/// (WESTERM_REMOTE_TRANSPORT), so nothing about it is said either.
type SocketA = Option<Result<Candidate, String>>;

/// A new socket A, when hole punching is allowed. Nothing is sent otherwise.
async fn prepare(ways: Ways, stun: &StunConfig) -> SocketA {
    if ways.punched { Some(punch::prepare(stun).await) } else { None }
}

/// What `remote-setup` sends to the gate: the address of socket A.
fn punch_request(socket_a: &SocketA) -> Option<PunchRequest> {
    match socket_a {
        Some(Ok(candidate)) => Some(candidate.request()),
        _ => None,
    }
}

/// The saved gate may be tried without SSH: the record is from this version,
/// and its last link was not punched (that gate does not answer directly).
/// Not with `punch`: that setting is for testing the punched path only.
fn fast_path_fits(record: &records::Record, local: &str, ways: Ways) -> bool {
    let punch_only = ways.punched && !ways.direct;
    record.version == local && !record.punched && !punch_only
}

/// The record's `punched` after a connection: which way won, or the old value when the tunnel carried it.
fn punched_after(won: Option<Won>, before: bool) -> bool {
    match won {
        Some(won) => won == Won::Punched,
        None => before,
    }
}

/// The "connected" stage of a tunnel link, with why QUIC was not used.
// Windows has no tunnel, as for `mod remote` in main.rs.
#[cfg_attr(not(unix), allow(dead_code))]
fn tunnel_stage(kill_on_logout: bool, direct: Option<&str>, punch: Option<&str>) -> LinkStage {
    LinkStage {
        stage: RemoteStage::Connected { via: RemoteVia::Tunnel, kill_on_logout },
        path: None,
        note: Some(punch::tunnel_note(direct, punch)),
    }
}

/// The errors of what was tried, as one detail.
fn joined<'a>(parts: impl IntoIterator<Item = Option<&'a str>>) -> String {
    parts.into_iter().flatten().collect::<Vec<_>>().join("; ")
}

const PROXIED: &str = "the destination goes through a proxy, which UDP cannot follow";

impl RemoteManager {
    pub fn new(state_dir: PathBuf) -> Self {
        Self { state_dir, targets: Mutex::new(HashMap::new()), identity: OnceCell::new(), links: AtomicU64::new(0) }
    }

    fn target(&self, name: &str) -> Arc<Target> {
        lock(&self.targets).entry(name.to_owned()).or_insert_with(|| Arc::new(Target::new())).clone()
    }

    fn dir(&self) -> Result<PathBuf, Failure> {
        remote_dir(&self.state_dir).map_err(|e| setup_failure(e.to_string()))
    }

    /// This dashboard's client certificate, made on first use.
    async fn identity(&self) -> Result<Arc<Identity>, Failure> {
        self.identity
            .get_or_try_init(|| async {
                let dir = self.dir()?;
                identity::load_or_create(&dir, "client").map(Arc::new).map_err(|e| setup_failure(e.to_string()))
            })
            .await
            .cloned()
    }

    /// The stages of `target`'s current or last connection attempt. A stage
    /// already published counts as new, so a window that joins a setup under
    /// way shows it (the replacement question, for one) at once.
    pub fn stages(&self, target: &str) -> watch::Receiver<Option<LinkStage>> {
        let mut stages = self.target(target).stages.subscribe();
        if stages.borrow().is_some() {
            stages.mark_changed();
        }
        stages
    }

    /// A window watches `name`'s link: keep it up until the last `Watch`
    /// is dropped. The first watcher starts the target's driver.
    pub fn watch(self: &Arc<Self>, name: &str) -> Watch {
        let target = self.target(name);
        let stages = self.stages(name);
        target.watchers.fetch_add(1, Ordering::SeqCst);
        target.wake.notify_one();
        if !target.driving.swap(true, Ordering::SeqCst) {
            tokio::spawn(Arc::clone(self).drive(name.to_owned(), Arc::clone(&target)));
        }
        Watch { stages, target }
    }

    /// Wakes a driver that stopped on a permanent failure. False when none
    /// waits. The user asked, so the SSH setup may run at once: the 30 s
    /// spacing is for the driver's own retries.
    pub fn retry(&self, name: &str) -> bool {
        let target = self.target(name);
        let Some(sender) = lock(&target.retry).take() else { return false };
        *lock(&target.last_failed_ssh) = None;
        sender.send(()).is_ok()
    }

    /// A stream on `name`'s live link. None when there is no live link:
    /// this never connects; the driver of a watched link does.
    pub async fn open_stream(&self, name: &str) -> Option<Opened> {
        let target = self.target(name);
        let (opener, link) = {
            let mut slot = target.slot.lock().await;
            let current = slot.as_mut()?;
            if !current.link.is_alive() {
                return None;
            }
            (current.link.opener(), current.id)
        };
        match opener.open().await {
            Ok(stream) => Some(Opened { stream, link }),
            Err(_) => {
                target.drop_link(link, false).await;
                None
            }
        }
    }

    /// The link `link` did not carry a session: drop it, and prepare the
    /// server over SSH for the next one. A newer link stays.
    pub async fn invalidate(&self, name: &str, link: u64) {
        self.target(name).drop_link(link, true).await;
    }

    /// The answer to a host replacement question. False when none waits.
    pub fn decide(&self, name: &str, replace: bool) -> bool {
        let sender = lock(&self.target(name).decision).take();
        sender.is_some_and(|sender| sender.send(replace).is_ok())
    }

    /// A close that the server may not have received: end `session` there
    /// at the next connection.
    pub fn remember_close(&self, name: &str, session: &str) {
        if let Ok(dir) = self.dir() {
            let _ = records::add_pending_close(&dir, name, session);
        }
    }

    /// The pane of `session` closed: end it on the server now, or at the
    /// next connection when the server is out of reach.
    pub async fn forget_session(&self, name: &str, session: &str) {
        let Ok(dir) = self.dir() else { return };
        if records::add_pending_close(&dir, name, session).is_err() {
            return;
        }
        let target = self.target(name);
        // Without a live link, the next link delivers the pending closes itself.
        let mut slot = target.slot.lock().await;
        let Some(current) = slot.as_mut() else { return };
        if !current.link.is_alive() {
            return;
        }
        let opener = current.link.opener();
        drop(slot);
        tokio::spawn(deliver_closes(dir, name.to_owned(), opener));
    }

    /// Replaces the server's host with this build now. Its shells come back
    /// as "restored" when their panes reconnect over the driver's new link.
    pub async fn update_host(&self, name: &str) -> Result<(), Failure> {
        let target = self.target(name);
        let mut slot = target.slot.lock().await;
        let identity = self.identity().await?;
        self.setup(name, &identity::fingerprint(&identity.cert), true, None).await?;
        let dir = self.dir()?;
        if let Some(mut record) = records::load(&dir, name) {
            record.declined = None;
            let _ = records::save(&dir, name, &record);
        }
        *slot = None;
        target.wake.notify_one();
        Ok(())
    }

    /// Keeps `target`'s link up while windows watch it: connects, reconnects
    /// by the retry rules, and ends once no window has watched for `linger()`.
    async fn drive(self: Arc<Self>, name: String, target: Arc<Target>) {
        let mut next = Next::Connect { failures: 0 };
        loop {
            if !target.watched() && !target.watched_within(linger()).await {
                target.let_go().await;
                if !target.resume() {
                    return;
                }
                next = Next::Connect { failures: 0 };
            }
            next = match next {
                Next::Connect { failures } => self.attempt(&name, &target, failures).await,
                Next::Backoff { failures } => backoff(&target, failures).await,
                Next::Halted => halted(&target).await,
                Next::Up => check_link(&target).await,
            };
        }
    }

    /// One try at a link. Shows how it went and says what comes next.
    async fn attempt(&self, name: &str, target: &Target, failures: u32) -> Next {
        match self.connect(name, target).await {
            Ok((link, connected)) => {
                self.install(name, target, link).await;
                publish(target, connected);
                Next::Up
            }
            Err(failure) => {
                let permanent = failure.reason.is_permanent();
                publish(target, RemoteStage::Failed { reason: failure.reason, detail: failure.detail });
                if permanent { Next::Halted } else { Next::Backoff { failures } }
            }
        }
    }

    /// The saved gate if it answers, else the full setup over SSH when it
    /// is due (`ssh_due`).
    async fn connect(&self, name: &str, target: &Target) -> Result<Made, Failure> {
        if let Some(made) = self.connect_fast(name, target).await {
            return Ok(made);
        }
        let last_failed = lock(&target.last_failed_ssh).clone();
        let now = Instant::now();
        if let Some((failed, failure)) = last_failed.filter(|(failed, _)| !ssh_due(Some(*failed), now)) {
            let wait = SSH_RETRY_INTERVAL.saturating_sub(now - failed).as_secs().max(1);
            return Err(waiting_failure(&failure, wait));
        }
        let made = self.connect_full(name, target).await;
        if let Err(failure) = &made {
            eprintln!("> remote {name}: {}", failure.detail);
        }
        *lock(&target.last_failed_ssh) = made.as_ref().err().map(|failure| (Instant::now(), failure.clone()));
        made
    }

    /// Puts a new link in the slot, and ends the sessions whose panes
    /// closed while the server was out of reach.
    async fn install(&self, name: &str, target: &Target, link: Link) {
        let opener = link.opener();
        let id = self.links.fetch_add(1, Ordering::Relaxed) + 1;
        *target.slot.lock().await = Some(Slot { link, id });
        if let Ok(dir) = self.dir() {
            tokio::spawn(deliver_closes(dir, name.to_owned(), opener));
        }
    }

    async fn probe(&self, name: &str) -> Result<Probe, Failure> {
        let stdout = ssh::run(name, ssh::PROBE, None).await?;
        ssh::parse_probe(&stdout).ok_or_else(|| setup_failure(format!("unexpected answer from the server: {}", stdout.trim())))
    }

    /// `punch`: stdin line 2, so the gate punches towards socket A.
    async fn setup(&self, name: &str, fingerprint: &str, replace_host: bool, punch: Option<&PunchRequest>) -> Result<SetupReport, Failure> {
        let command = if replace_host { ssh::SETUP_REPLACE } else { ssh::SETUP };
        let mut stdin = format!("{fingerprint}\n");
        if let Some(line) = punch.and_then(|punch| serde_json::to_string(punch).ok()) {
            stdin.push_str(&line);
            stdin.push('\n');
        }
        let stdout = ssh::run(name, command, Some(stdin.into_bytes())).await?;
        let report = parse_report(&stdout).ok_or_else(|| setup_failure(format!("unexpected answer from remote-setup: {}", stdout.trim())))?;
        // The tunnel passes this path to `ssh -L`, which reads ':' as a separator.
        if !report.dashboard.starts_with('/') || report.dashboard.contains([':', ' ', '\n']) {
            return Err(setup_failure(format!("unexpected dashboard socket {}", report.dashboard)));
        }
        Ok(report)
    }

    /// The saved gate, without SSH: when the record is from this version
    /// and the gate did not fail since. None when it does not answer.
    /// A punched link is not tried: its saved gate does not answer directly,
    /// and its punched socket is gone.
    async fn connect_fast(&self, name: &str, target: &Target) -> Option<Made> {
        if target.stale.load(Ordering::Relaxed) {
            return None;
        }
        let ways = punch::ways_from(&std::env::var("WESTERM_REMOTE_TRANSPORT").unwrap_or_default());
        let record = records::load(&self.dir().ok()?, name).filter(|record| fast_path_fits(record, &crate::version_line(), ways))?;
        let identity = self.identity().await.ok()?;
        publish(target, RemoteStage::Connecting);
        let link = QuicLink::connect(&identity, &record.hostname, record.port, &record.fingerprint, FAST_PATH_TIMEOUT).await.ok()?;
        Some((Link::Quic(link), RemoteStage::Connected { via: RemoteVia::Quic, kill_on_logout: false }.into()))
    }

    /// The full setup over SSH: check the server, install this build if
    /// needed, run `remote-setup` (maybe asking to replace an older host),
    /// then connect over QUIC or the SSH tunnel.
    async fn connect_full(&self, name: &str, target: &Target) -> Result<Made, Failure> {
        let dir = self.dir()?;
        let identity = self.identity().await?;
        let local = crate::version_line();
        let saved = records::load(&dir, name);
        let ways = punch::ways_from(&std::env::var("WESTERM_REMOTE_TRANSPORT").unwrap_or_default());
        let stun = stun::config_from(std::env::var("WESTERM_REMOTE_STUN").ok().as_deref());

        publish(target, RemoteStage::Checking);
        // STUN runs alongside the probe, so it adds no wait before the setup;
        // a failed probe shows at once.
        let (probe, mut socket_a) = tokio::try_join!(self.probe(name), async { Ok(prepare(ways, &stun).await) })?;
        let unsupported = || Failure::new(RemoteFailure::Unsupported, format!("{} {}", probe.os, probe.arch));
        if probe.os != "Linux" {
            return Err(unsupported());
        }
        let triple = binary::linux_triple(&probe.arch).ok_or_else(unsupported)?;
        match binary::install_plan(probe.version.as_deref(), &local) {
            InstallPlan::UpToDate => {}
            InstallPlan::RemoteNewer => {
                return Err(Failure::new(RemoteFailure::NewerRemote, probe.version.clone().unwrap_or_default()));
            }
            InstallPlan::Install => {
                publish(target, RemoteStage::Installing { version: update::CURRENT.to_owned() });
                let path = binary::obtain(&self.state_dir, triple).await?;
                let bytes = tokio::fs::read(&path).await.map_err(|e| setup_failure(format!("cannot read {}: {e}", path.display())))?;
                ssh::run(name, ssh::UPLOAD, Some(bytes)).await?;
                let installed = self.probe(name).await?.version;
                if installed.as_deref() != Some(local.as_str()) {
                    let reported = installed.unwrap_or_else(|| "nothing".to_owned());
                    return Err(setup_failure(format!("the uploaded westerm reports {reported}")));
                }
                // The install can outlast a NAT's idle mapping (30 s on many
                // routers): the gate must punch towards a fresh socket A.
                if let Some(Ok(_)) = socket_a {
                    socket_a = prepare(ways, &stun).await;
                }
            }
        }

        publish(target, RemoteStage::Starting);
        let fingerprint = identity::fingerprint(&identity.cert);
        let mut report = self.setup(name, &fingerprint, false, punch_request(&socket_a).as_ref()).await?;
        let mut received = Instant::now();
        let mut declined = saved.as_ref().and_then(|record| record.declined.as_deref()) == Some(local.as_str());
        match host_plan(&report.host, HOST_BUILD, declined) {
            HostPlan::Use | HostPlan::Incompatible => {}
            HostPlan::Replace => {
                report = self.setup_again(name, &fingerprint, true, &mut socket_a, &stun).await?;
                received = Instant::now();
            }
            HostPlan::Ask { sessions } => {
                // Waits for an answer even when no window watches: the next
                // window to open shows the question.
                let (answer, question) = oneshot::channel();
                *lock(&target.decision) = Some(answer);
                publish(target, RemoteStage::Confirm { sessions });
                if question.await.map_err(|_| setup_failure("the host question went unanswered"))? {
                    report = self.setup_again(name, &fingerprint, true, &mut socket_a, &stun).await?;
                    received = Instant::now();
                } else {
                    declined = true;
                }
            }
        }

        let resolved = ssh::resolve(name).await?;
        let record = records::Record {
            hostname: resolved.hostname.clone(),
            port: report.port,
            fingerprint: report.fingerprint.clone(),
            version: local.clone(),
            declined: declined.then(|| local.clone()),
            // Known after the transport below.
            punched: saved.as_ref().is_some_and(|record| record.punched),
        };
        records::save(&dir, name, &record).map_err(|e| setup_failure(e.to_string()))?;
        if report.host.build != HOST_BUILD && report.host.protocol != PROTOCOL {
            return Err(Failure::new(
                RemoteFailure::Incompatible,
                format!("the server's host {} speaks protocol {}", report.host.build, report.host.protocol),
            ));
        }

        publish(target, RemoteStage::Connecting);
        // Such as after the host question: the gate may have closed socket B
        // by now. Asked again once; the second offer is used however old.
        if let (Some(Ok(_)), Some(PunchAnswer::Ready { wait_ms, .. })) = (&socket_a, &report.punch)
            && !punch::offer_fresh(received, *wait_ms, Instant::now())
        {
            socket_a = prepare(ways, &stun).await;
            if let Some(request) = punch_request(&socket_a) {
                match self.setup(name, &fingerprint, false, Some(&request)).await {
                    Ok(again) => report.punch = again.punch,
                    // Hole punching never fails a connection: the other ways stay.
                    Err(failure) => socket_a = Some(Err(format!("hole punching: {}", failure.detail))),
                }
            }
        }
        let (link, connected, won) = transport(name, &identity, &resolved, &report, &dir, ways, socket_a).await?;
        let punched = punched_after(won, record.punched);
        if punched != record.punched {
            // The link is up: a record that cannot be saved only costs the fast path a try.
            let _ = records::save(&dir, name, &records::Record { punched, ..record });
        }
        target.stale.store(false, Ordering::Relaxed);
        Ok((link, connected))
    }

    /// `remote-setup` once more (to replace the host), with a new socket A:
    /// the old one's offer is not used, and the gate opens a new socket B.
    async fn setup_again(
        &self,
        name: &str,
        fingerprint: &str,
        replace_host: bool,
        socket_a: &mut SocketA,
        stun: &StunConfig,
    ) -> Result<SetupReport, Failure> {
        // Without a socket A the reason stays: asking STUN again would only add a wait.
        if let Some(Ok(_)) = socket_a {
            *socket_a = Some(punch::prepare(stun).await);
        }
        self.setup(name, fingerprint, replace_host, punch_request(socket_a).as_ref()).await
    }
}

/// Direct QUIC and the punched path at once (direct wins when both
/// work), else the SSH tunnel. WESTERM_REMOTE_TRANSPORT limits the ways
/// (`ways_from`). Also says which QUIC way won; None for the tunnel.
async fn transport(
    name: &str,
    identity: &Identity,
    resolved: &Resolved,
    report: &SetupReport,
    dir: &Path,
    ways: Ways,
    socket_a: SocketA,
) -> Result<(Link, LinkStage, Option<Won>), Failure> {
    let direct = (ways.direct && !resolved.proxied)
        .then(|| QuicLink::connect(identity, &resolved.hostname, report.port, &report.fingerprint, HANDSHAKE_TIMEOUT));
    // The punched try, or why there is none. A socket A not used closes here.
    let (punched, not_tried) = match (socket_a, &report.punch) {
        (None, _) => (None, None),
        (Some(Err(reason)), _) => (None, Some(reason)),
        (Some(Ok(candidate)), Some(PunchAnswer::Ready { addr, .. })) => {
            let addr = SocketAddr::V4(*addr);
            let fingerprint = &report.fingerprint;
            let try_punched = async move {
                let socket = candidate.into_socket().map_err(|e| Failure::new(RemoteFailure::Transport, format!("socket A: {e}")))?;
                QuicLink::connect_on(socket, identity, addr, fingerprint, PUNCH_HANDSHAKE_TIMEOUT).await
            };
            (Some(try_punched), None)
        }
        (Some(Ok(_)), answer) => (None, punch::skip_text(answer.as_ref()).map(str::to_owned)),
    };
    let (direct_error, punched_error) = match punch::race(direct, punched, DIRECT_GRACE).await {
        Ok((link, won)) => {
            let stage = RemoteStage::Connected { via: RemoteVia::Quic, kill_on_logout: report.kill_on_logout };
            let path = (won == Won::Punched).then_some(QuicPath::Punched);
            return Ok((Link::Quic(link), LinkStage { stage, path, note: None }, Some(won)));
        }
        Err((direct, punched)) => (direct.map(|failure| failure.detail), punched),
    };
    let punch_reason = not_tried.or_else(|| punched_error.map(|failure| format!("hole punching: {}", failure.detail)));
    // What a user reads about direct QUIC: its error, or why it was allowed but not tried.
    let direct_reason = || direct_error.clone().or_else(|| ways.direct.then(|| PROXIED.to_owned()));
    if !ways.tunnel {
        return Err(Failure::new(RemoteFailure::Transport, joined([direct_reason().as_deref(), punch_reason.as_deref()])));
    }
    #[cfg(unix)]
    {
        let socket = dir.join(format!("t-{}.sock", &identity::fingerprint(name.as_bytes())[..8]));
        match Tunnel::start(name, socket, &report.dashboard).await {
            Ok(tunnel) => {
                let stage = tunnel_stage(report.kill_on_logout, direct_reason().as_deref(), punch_reason.as_deref());
                eprintln!("> remote {name}: {}; using the SSH tunnel", stage.note.as_deref().unwrap_or_default());
                Ok((Link::Tunnel(tunnel), stage, None))
            }
            Err(tunnel) => {
                let detail = joined([direct_error.as_deref(), punch_reason.as_deref(), Some(&tunnel.detail)]);
                Err(Failure::new(RemoteFailure::Transport, detail))
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (name, dir);
        let quic = direct_error.unwrap_or_default();
        let mut detail = format!("{quic}; the SSH tunnel fallback needs macOS or Linux");
        if let Some(punch) = punch_reason {
            detail = format!("{detail}; {punch}");
        }
        Err(Failure::new(RemoteFailure::Transport, detail))
    }
}

/// Waits out a transient failure. A window leaving cuts the wait short, so
/// the driver can let go.
async fn backoff(target: &Target, failures: u32) -> Next {
    tokio::select! {
        () = tokio::time::sleep(retry_delay(failures)) => Next::Connect { failures: failures.saturating_add(1) },
        () = target.until_unwatched() => Next::Backoff { failures },
    }
}

/// Waits for `retry` after a permanent failure, or until no window watches.
async fn halted(target: &Target) -> Next {
    let (retry, retried) = oneshot::channel();
    *lock(&target.retry) = Some(retry);
    tokio::select! {
        // `retry` cleared the last SSH failure.
        Ok(()) = retried => Next::Connect { failures: 0 },
        () = target.until_unwatched() => Next::Halted,
    }
}

/// Checks the live link every second, or at once when woken. A dead or
/// dropped link is made again, the saved gate first.
async fn check_link(target: &Target) -> Next {
    let _ = tokio::time::timeout(LINK_CHECK, target.wake.notified()).await;
    let mut slot = target.slot.lock().await;
    if slot.as_mut().is_some_and(|current| current.link.is_alive()) {
        return Next::Up;
    }
    *slot = None;
    drop(slot);
    publish(target, RemoteStage::Connecting);
    Next::Connect { failures: 0 }
}

/// Ends the sessions whose panes closed while `target` was out of reach.
async fn deliver_closes(dir: PathBuf, target: String, opener: Opener) {
    for id in records::take_pending_closes(&dir, &target) {
        if is_id(&id) && end_session(&opener, &id).await.is_err() {
            let _ = records::add_pending_close(&dir, &target, &id);
        }
    }
}

/// Attaches to `id` on the remote host and closes it, as a pane does.
async fn end_session(opener: &Opener, id: &str) -> Result<(), String> {
    let stream = opener.open().await.map_err(|e| e.to_string())?;
    let url = format!("ws://remote/ws/session?session={id}&kind=shell&cols=80&rows=24&width=640&height=408&lang=en");
    let (mut ws, _) = tokio::time::timeout(Duration::from_secs(5), tokio_tungstenite::client_async(url, stream))
        .await
        .map_err(|_| "timed out".to_owned())?
        .map_err(|e| e.to_string())?;
    ws.send(Message::text(r#"{"type":"close"}"#)).await.map_err(|e| e.to_string())?;
    // The host closes the socket once the session has ended.
    let _ = tokio::time::timeout(Duration::from_secs(5), async { while let Some(Ok(_)) = ws.next().await {} }).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A window opened while another window's setup is under way must show
    /// the current stage (such as the replacement question) at once.
    #[tokio::test]
    async fn a_late_subscriber_sees_the_current_stage() {
        let dir = tempfile::tempdir().unwrap();
        let manager = RemoteManager::new(dir.path().to_path_buf());
        publish(&manager.target("prod"), RemoteStage::Confirm { sessions: 2 });
        let mut stages = manager.stages("prod");
        assert!(stages.has_changed().unwrap(), "the current stage counts as new");
        assert_eq!(*stages.borrow_and_update(), Some(LinkStage::from(RemoteStage::Confirm { sessions: 2 })));
        let fresh = manager.stages("other");
        assert!(!fresh.has_changed().unwrap(), "nothing to show yet");
    }

    #[test]
    fn retry_delays_grow_to_eight_seconds() {
        let secs: Vec<u64> = (0..6).map(|n| retry_delay(n).as_secs()).collect();
        assert_eq!(secs, [1, 2, 4, 8, 8, 8]);
    }

    #[test]
    fn ssh_waits_thirty_seconds_after_a_failed_setup() {
        let now = Instant::now();
        assert!(ssh_due(None, now));
        assert!(!ssh_due(Some(now - Duration::from_secs(29)), now));
        assert!(ssh_due(Some(now - Duration::from_secs(30)), now));
    }

    /// A user who fixed the cause and pressed "Try again" gets SSH at once,
    /// not the remembered failure again.
    #[test]
    fn try_again_forgets_the_last_ssh_failure() {
        let dir = tempfile::tempdir().unwrap();
        let manager = RemoteManager::new(dir.path().to_path_buf());
        let target = manager.target("prod");
        *lock(&target.last_failed_ssh) = Some((Instant::now(), Failure::new(RemoteFailure::Auth, "Permission denied")));
        let (sender, mut waiting) = oneshot::channel();
        *lock(&target.retry) = Some(sender);
        assert!(manager.retry("prod"));
        assert!(lock(&target.last_failed_ssh).is_none(), "cleared before the driver wakes");
        assert!(ssh_due(lock(&target.last_failed_ssh).as_ref().map(|(failed, _)| *failed), Instant::now()));
        assert!(waiting.try_recv().is_ok(), "the driver is woken");
    }

    #[test]
    fn a_failed_ssh_check_keeps_its_reason_while_ssh_waits() {
        let last = Failure::new(RemoteFailure::Unreachable, "ssh: connect to host prod port 22: Connection refused");
        let shown = waiting_failure(&last, 12);
        assert_eq!(shown.reason, RemoteFailure::Unreachable);
        assert!(shown.detail.starts_with(&last.detail), "{}", shown.detail);
        assert!(shown.detail.contains("the next SSH check is in 12 s"), "{}", shown.detail);
        assert_eq!(waiting_failure(&Failure::new(RemoteFailure::Transport, ""), 3).detail, "the next SSH check is in 3 s");
        // The driver halts on it, and Retry runs SSH at once: no wait to mention.
        let permanent = Failure::new(RemoteFailure::Auth, "Permission denied (publickey)");
        assert_eq!(waiting_failure(&permanent, 5), permanent);
    }

    #[test]
    fn host_plans_follow_the_users_choice() {
        let host = |build: &str, protocol, sessions| HostInfo { build: build.into(), protocol, sessions };
        assert_eq!(host_plan(&host("rs-a", 1, 3), "rs-a", false), HostPlan::Use);
        assert_eq!(host_plan(&host("rs-old", 1, 0), "rs-a", false), HostPlan::Replace);
        assert_eq!(host_plan(&host("rs-old", 1, 2), "rs-a", false), HostPlan::Ask { sessions: 2 });
        assert_eq!(host_plan(&host("rs-old", PROTOCOL, 2), "rs-a", true), HostPlan::Use);
        assert_eq!(host_plan(&host("rs-old", PROTOCOL + 1, 2), "rs-a", true), HostPlan::Incompatible);
    }

    /// Review Focus 5: after the user forwards the port, direct QUIC wins and the fast path comes back.
    #[test]
    fn a_direct_win_clears_the_punched_flag() {
        assert!(punched_after(Some(Won::Punched), true));
        assert!(punched_after(Some(Won::Punched), false));
        assert!(!punched_after(Some(Won::Direct), true));
        // The tunnel carried the link: what the record said stays.
        assert!(punched_after(None, true));
        assert!(!punched_after(None, false));
    }

    /// Rule 10: a punched record's gate does not answer directly, so the fast path skips it.
    #[test]
    fn the_fast_path_skips_a_punched_record() {
        let local = "westerm 0.4.3 (host rs-1)";
        let record = records::Record {
            hostname: "h".into(),
            port: 4317,
            fingerprint: "ab".repeat(32),
            version: local.into(),
            declined: None,
            punched: false,
        };
        let all = punch::ways_from("");
        assert!(fast_path_fits(&record, local, all));
        assert!(!fast_path_fits(&records::Record { punched: true, ..record.clone() }, local, all));
        assert!(!fast_path_fits(&record, "westerm 0.4.4 (host rs-2)", all), "a record of another version");
    }

    /// `punch` tests the punched path, so the saved direct gate is not tried;
    /// every other setting keeps the fast path as before.
    #[test]
    fn the_fast_path_follows_the_transport_setting() {
        let local = "westerm 0.4.3 (host rs-1)";
        let record = records::Record {
            hostname: "h".into(),
            port: 4317,
            fingerprint: "ab".repeat(32),
            version: local.into(),
            declined: None,
            punched: false,
        };
        for setting in ["", "quic", "tunnel", "other"] {
            assert!(fast_path_fits(&record, local, punch::ways_from(setting)), "{setting:?}");
        }
        assert!(!fast_path_fits(&record, local, punch::ways_from("punch")));
    }

    /// What a window is told when neither QUIC way worked and the tunnel carries the link.
    #[test]
    fn a_tunnel_link_tells_both_reasons() {
        let stage = tunnel_stage(false, Some("no QUIC answer from 49.171.219.140:4317 within 3 s (is UDP 4317 open?)"), Some("hole punching: no STUN answer on this computer"));
        assert_eq!(stage.stage, RemoteStage::Connected { via: RemoteVia::Tunnel, kill_on_logout: false });
        assert_eq!(stage.path, None);
        assert_eq!(
            stage.note.as_deref(),
            Some("QUIC: no QUIC answer from 49.171.219.140:4317 within 3 s (is UDP 4317 open?); hole punching: no STUN answer on this computer")
        );
    }
}
