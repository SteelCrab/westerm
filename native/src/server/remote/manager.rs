//! One connection per server, shared by all its panes. The first pane to
//! need it prepares the server over SSH (check, install, start, maybe ask
//! to replace an older host) while the others wait; every pane shows the
//! same stages.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{oneshot, watch, OnceCell};
use tokio_tungstenite::tungstenite::Message;

use super::binary::{self, InstallPlan};
use super::quic::{self, QuicLink};
use super::ssh::{self, Probe, Resolved};
#[cfg(unix)]
use super::tunnel::{self, Tunnel};
use super::{records, Failure, HostStream};
use crate::host::HOST_BUILD;
use crate::protocol::{is_id, RemoteFailure, RemoteStage, RemoteVia, PROTOCOL};
use crate::remote::identity::{self, Identity};
use crate::remote::report::{parse_report, HostInfo, SetupReport};
use crate::remote::{remote_dir, HANDSHAKE_TIMEOUT};
use crate::update;

/// A saved gate gets this long before the full SSH setup runs instead.
const FAST_PATH_TIMEOUT: Duration = Duration::from_secs(2);

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

struct Target {
    slot: tokio::sync::Mutex<Option<Slot>>,
    stages: watch::Sender<Option<RemoteStage>>,
    decision: Mutex<Option<oneshot::Sender<bool>>>,
    /// The saved gate failed: skip the fast path once.
    stale: AtomicBool,
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

fn publish(target: &Target, stage: RemoteStage) {
    target.stages.send_replace(Some(stage));
}

fn setup_failure(detail: impl Into<String>) -> Failure {
    Failure::new(RemoteFailure::Setup, detail)
}

impl RemoteManager {
    pub fn new(state_dir: PathBuf) -> Self {
        Self { state_dir, targets: Mutex::new(HashMap::new()), identity: OnceCell::new(), links: AtomicU64::new(0) }
    }

    fn target(&self, name: &str) -> Arc<Target> {
        let mut targets = self.targets.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        targets
            .entry(name.to_owned())
            .or_insert_with(|| {
                Arc::new(Target {
                    slot: tokio::sync::Mutex::new(None),
                    stages: watch::channel(None).0,
                    decision: Mutex::new(None),
                    stale: AtomicBool::new(false),
                })
            })
            .clone()
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
    /// already published counts as new, so a pane that joins a setup under
    /// way shows it (the replacement question, for one) at once.
    pub fn stages(&self, target: &str) -> watch::Receiver<Option<RemoteStage>> {
        let mut stages = self.target(target).stages.subscribe();
        if stages.borrow().is_some() {
            stages.mark_changed();
        }
        stages
    }

    /// A stream to `target`'s host. The first caller prepares the server;
    /// callers for the same server wait for it and share the connection.
    pub async fn open_stream(&self, name: &str) -> Result<Opened, Failure> {
        let target = self.target(name);
        let mut slot = target.slot.lock().await;
        if let Some(current) = slot.as_mut() {
            if current.link.is_alive() {
                if let Ok(stream) = current.link.opener().open().await {
                    return Ok(Opened { stream, link: current.id });
                }
            }
            *slot = None;
        }
        let link = self.connect(name, &target).await?;
        let opener = link.opener();
        let stream = opener.open().await.map_err(|e| Failure::new(RemoteFailure::Transport, e.to_string()))?;
        let id = self.links.fetch_add(1, Ordering::Relaxed) + 1;
        *slot = Some(Slot { link, id });
        drop(slot);
        if let Ok(dir) = self.dir() {
            tokio::spawn(deliver_closes(dir, name.to_owned(), opener));
        }
        Ok(Opened { stream, link: id })
    }

    /// The link `link` did not carry a session: drop it, and prepare the
    /// server over SSH next time. A newer link stays.
    pub async fn invalidate(&self, name: &str, link: u64) {
        let target = self.target(name);
        let mut slot = target.slot.lock().await;
        if slot.as_ref().is_some_and(|current| current.id == link) {
            *slot = None;
            target.stale.store(true, Ordering::Relaxed);
        }
    }

    /// The answer to a host replacement question. False when none waits.
    pub fn decide(&self, name: &str, replace: bool) -> bool {
        let sender = self.target(name).decision.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
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
        // A connection being made delivers the pending closes itself.
        let Ok(mut slot) = target.slot.try_lock() else { return };
        let Some(current) = slot.as_mut() else { return };
        if !current.link.is_alive() {
            return;
        }
        let opener = current.link.opener();
        drop(slot);
        tokio::spawn(deliver_closes(dir, name.to_owned(), opener));
    }

    /// Replaces the server's host with this build now. Its shells come back
    /// as "restored" when their panes reconnect.
    pub async fn update_host(&self, name: &str) -> Result<(), Failure> {
        let target = self.target(name);
        let mut slot = target.slot.lock().await;
        let identity = self.identity().await?;
        self.setup(name, &identity::fingerprint(&identity.cert), true).await?;
        let dir = self.dir()?;
        if let Some(mut record) = records::load(&dir, name) {
            record.declined = None;
            let _ = records::save(&dir, name, &record);
        }
        *slot = None;
        Ok(())
    }

    async fn probe(&self, name: &str) -> Result<Probe, Failure> {
        let stdout = ssh::run(name, ssh::PROBE, None).await?;
        ssh::parse_probe(&stdout).ok_or_else(|| setup_failure(format!("unexpected answer from the server: {}", stdout.trim())))
    }

    async fn setup(&self, name: &str, fingerprint: &str, replace_host: bool) -> Result<SetupReport, Failure> {
        let command = if replace_host { ssh::SETUP_REPLACE } else { ssh::SETUP };
        let stdout = ssh::run(name, command, Some(format!("{fingerprint}\n").into_bytes())).await?;
        let report = parse_report(&stdout).ok_or_else(|| setup_failure(format!("unexpected answer from remote-setup: {}", stdout.trim())))?;
        // The tunnel passes this path to `ssh -L`, which reads ':' as a separator.
        if !report.socket.starts_with('/') || report.socket.contains([':', ' ', '\n']) {
            return Err(setup_failure(format!("unexpected host socket {}", report.socket)));
        }
        Ok(report)
    }

    /// Makes a new link: the saved gate if it answers, else the full setup.
    async fn connect(&self, name: &str, target: &Target) -> Result<Link, Failure> {
        let dir = self.dir()?;
        let identity = self.identity().await?;
        let local = crate::version_line();
        let saved = records::load(&dir, name);

        if !target.stale.load(Ordering::Relaxed) {
            if let Some(record) = saved.as_ref().filter(|record| record.version == local) {
                publish(target, RemoteStage::Connecting);
                if let Ok(link) =
                    QuicLink::connect(&identity, &record.hostname, record.port, &record.fingerprint, FAST_PATH_TIMEOUT).await
                {
                    publish(target, RemoteStage::Connected { via: RemoteVia::Quic, kill_on_logout: false });
                    return Ok(Link::Quic(link));
                }
            }
        }

        publish(target, RemoteStage::Checking);
        let probe = self.probe(name).await?;
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
            }
        }

        publish(target, RemoteStage::Starting);
        let fingerprint = identity::fingerprint(&identity.cert);
        let mut report = self.setup(name, &fingerprint, false).await?;
        let mut declined = saved.as_ref().and_then(|record| record.declined.as_deref()) == Some(local.as_str());
        match host_plan(&report.host, HOST_BUILD, declined) {
            HostPlan::Use | HostPlan::Incompatible => {}
            HostPlan::Replace => report = self.setup(name, &fingerprint, true).await?,
            HostPlan::Ask { sessions } => {
                let (answer, question) = oneshot::channel();
                *target.decision.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(answer);
                publish(target, RemoteStage::Confirm { sessions });
                if question.await.map_err(|_| setup_failure("the host question went unanswered"))? {
                    report = self.setup(name, &fingerprint, true).await?;
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
        };
        records::save(&dir, name, &record).map_err(|e| setup_failure(e.to_string()))?;
        if report.host.build != HOST_BUILD && report.host.protocol != PROTOCOL {
            return Err(Failure::new(
                RemoteFailure::Incompatible,
                format!("the server's host {} speaks protocol {}", report.host.build, report.host.protocol),
            ));
        }

        publish(target, RemoteStage::Connecting);
        let link = self.transport(name, &identity, &resolved, &report, &dir).await?;
        let via = match link {
            Link::Quic(_) => RemoteVia::Quic,
            #[cfg(unix)]
            Link::Tunnel(_) => RemoteVia::Tunnel,
        };
        target.stale.store(false, Ordering::Relaxed);
        publish(target, RemoteStage::Connected { via, kill_on_logout: report.kill_on_logout });
        Ok(link)
    }

    /// QUIC, or the SSH tunnel when UDP does not get through.
    /// WESTERM_REMOTE_TRANSPORT=quic or =tunnel allows only that one.
    async fn transport(
        &self,
        name: &str,
        identity: &Identity,
        resolved: &Resolved,
        report: &SetupReport,
        dir: &Path,
    ) -> Result<Link, Failure> {
        let only = std::env::var("WESTERM_REMOTE_TRANSPORT").unwrap_or_default();
        let mut quic_error = None;
        if only != "tunnel" && !resolved.proxied {
            match QuicLink::connect(identity, &resolved.hostname, report.port, &report.fingerprint, HANDSHAKE_TIMEOUT).await {
                Ok(link) => return Ok(Link::Quic(link)),
                Err(failure) => quic_error = Some(failure.detail),
            }
        }
        if only == "quic" {
            let detail = quic_error.unwrap_or_else(|| "the destination goes through a proxy, which UDP cannot follow".to_owned());
            return Err(Failure::new(RemoteFailure::Transport, detail));
        }
        #[cfg(unix)]
        {
            let socket = dir.join(format!("t-{}.sock", &identity::fingerprint(name.as_bytes())[..8]));
            Tunnel::start(name, socket, &report.socket).await.map(Link::Tunnel).map_err(|tunnel| {
                let detail = match quic_error {
                    Some(quic) => format!("{quic}; {}", tunnel.detail),
                    None => tunnel.detail,
                };
                Failure::new(RemoteFailure::Transport, detail)
            })
        }
        #[cfg(not(unix))]
        {
            let _ = (name, dir);
            let quic = quic_error.unwrap_or_default();
            Err(Failure::new(RemoteFailure::Transport, format!("{quic}; the SSH tunnel fallback needs macOS or Linux")))
        }
    }
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
    let url = format!("ws://host/session?session={id}&kind=shell&cols=80&rows=24&width=640&height=408&lang=en");
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

    /// A pane opened while another pane's setup is under way must show the
    /// current stage (such as the replacement question) at once.
    #[tokio::test]
    async fn a_late_subscriber_sees_the_current_stage() {
        let dir = tempfile::tempdir().unwrap();
        let manager = RemoteManager::new(dir.path().to_path_buf());
        publish(&manager.target("prod"), RemoteStage::Confirm { sessions: 2 });
        let mut stages = manager.stages("prod");
        assert!(stages.has_changed().unwrap(), "the current stage counts as new");
        assert_eq!(*stages.borrow_and_update(), Some(RemoteStage::Confirm { sessions: 2 }));
        let fresh = manager.stages("other");
        assert!(!fresh.has_changed().unwrap(), "nothing to show yet");
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
}
