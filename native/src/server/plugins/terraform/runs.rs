//! Runs and the plan each project keeps. A run is a spec under a token; the
//! runner in the pane says when it started and how it ended, and `settle`
//! moves the run and its plan on from those files. A plan can be applied
//! only while the code is as it was when the plan started.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::plan::{self, Change, Counts, PlanLimits, PlanMark, Reduced};
use super::scan::Project;
use super::tree::{Node, Tree};
use crate::plugins::terraform::{self as files, private_dir, root_dir, Action, RunSpec, Tool};
use crate::state_dir::private_file;

/// A plan's JSON larger than this is not read.
pub const MAX_JSON_BYTES: u64 = 128 * 1024 * 1024;
/// A run whose runner has not started after this long never will.
const EXPIRES_MS: u64 = 30_000;
/// A reduced plan larger than this is not kept in a file (it stays in memory).
const MAX_REDUCED_BYTES: usize = 16 * 1024 * 1024;
/// A plan older than this goes when the server starts.
const KEEP_MS: u64 = 7 * 24 * 60 * 60 * 1000;

pub fn json_fits(bytes: u64) -> bool {
    bytes <= MAX_JSON_BYTES
}

/// Where a plan stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    /// A plan run is making it.
    Planning,
    /// It can be applied while the code is the same.
    Ready,
    /// The plan was made, but its JSON could not be read.
    Unreadable,
    /// An apply run is using it.
    Applying,
    Applied,
    ApplyFailed,
}

/// What is kept beside a plan file (`<plan>.meta.json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanMeta {
    /// Of the code when the plan started; empty when the plan cannot match any code.
    pub fingerprint: String,
    pub started_at: u64,
    /// When the plan, or its apply, ended; 0 before.
    pub ended_at: u64,
    /// How long the plan took.
    pub seconds: u64,
    pub workspace: String,
    pub tool: Tool,
    pub target: Option<String>,
    pub outcome: Outcome,
    /// The folders the fingerprint reads.
    pub dirs: Vec<PathBuf>,
    /// The plan's numbers, once it is read.
    pub counts: Option<Counts>,
}

/// The plan file of a project and workspace. The workspace is in the name as
/// a digest: a workspace name can hold characters a file name cannot.
pub fn plan_path(state_dir: &Path, project: &str, workspace: &str) -> PathBuf {
    let digest: String = Sha256::digest(workspace.as_bytes())[..8].iter().map(|byte| format!("{byte:02x}")).collect();
    root_dir(state_dir).join("plans").join(project).join(format!("{digest}.tfplan"))
}

fn json_path(plan: &Path) -> PathBuf {
    plan.with_extension("json")
}

fn meta_path(plan: &Path) -> PathBuf {
    plan.with_extension("meta.json")
}

/// Where the plan as the page shows it is kept, so it shows again after a
/// restart. It holds no sensitive value (`plan::reduce` drops them).
fn reduced_path(plan: &Path) -> PathBuf {
    plan.with_extension("reduced.json")
}

fn write_reduced(plan: &Path, reduced: &Reduced) {
    let Ok(body) = serde_json::to_vec(reduced) else { return };
    if body.len() > MAX_REDUCED_BYTES {
        return;
    }
    let written = private_file().write(true).create(true).truncate(true).open(reduced_path(plan)).and_then(|mut file| file.write_all(&body));
    if written.is_err() {
        let _ = fs::remove_file(reduced_path(plan));
    }
}

fn read_reduced(plan: &Path) -> Option<Reduced> {
    serde_json::from_slice(&fs::read(reduced_path(plan)).ok()?).ok()
}

fn read_meta(plan: &Path) -> Option<PlanMeta> {
    serde_json::from_slice(&fs::read(meta_path(plan)).ok()?).ok()
}

fn write_meta(plan: &Path, meta: &PlanMeta) -> io::Result<()> {
    let (path, temp) = (meta_path(plan), plan.with_extension("meta.tmp"));
    let body = serde_json::to_vec(meta).map_err(io::Error::other)?;
    let mut file = private_file().write(true).create(true).truncate(true).open(&temp)?;
    file.write_all(&body)?;
    drop(file);
    fs::rename(&temp, path)
}

/// Removes a plan with its JSON, its reduced copy, and its meta.
fn remove_plan(plan: &Path) {
    for path in [plan.to_path_buf(), json_path(plan), reduced_path(plan), meta_path(plan)] {
        let _ = fs::remove_file(path);
    }
}

/// Why a run does not start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The project has a run already.
    Running,
    /// There is no plan to apply, or it would change nothing.
    NoPlan,
    /// The code changed after the plan.
    Stale,
    /// The project's runs happen elsewhere; its plan cannot be saved.
    Remote,
    NoTool,
    /// westerm cannot type into the user's shell.
    NoShell,
    BadTarget,
    /// A file could not be written.
    Internal,
}

impl Refusal {
    /// The word the page reads.
    pub fn code(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::NoPlan => "noPlan",
            Self::Stale => "stale",
            Self::Remote => "remote",
            Self::NoTool => "noTool",
            Self::NoShell => "noShell",
            Self::BadTarget => "badTarget",
            Self::Internal => "internal",
        }
    }

    /// The request was fine, but the project's state does not allow it now.
    pub fn conflict(self) -> bool {
        matches!(self, Self::Running | Self::NoPlan | Self::Stale | Self::Remote)
    }
}

/// How a run stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    Running,
    Ok,
    /// The tool ended with an error, or Ctrl+C stopped it.
    Failed,
    /// The runner is gone without saying how the tool ended.
    Interrupted,
}

/// A project's running or last run, as the page reads it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RunInfo {
    pub action: Action,
    pub state: RunState,
    /// When it started (running) or ended.
    pub at: u64,
}

/// How a plan stands, as the page reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PlanState {
    Current,
    Stale,
    Unreadable,
    Applied,
    ApplyFailed,
}

/// A project's plan in the list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlanInfo {
    pub state: PlanState,
    pub counts: Option<Counts>,
    pub applyable: bool,
    pub at: u64,
    pub target: Option<String>,
}

/// A project's plan in full.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanAnswer {
    pub state: PlanState,
    pub counts: Option<Counts>,
    pub applyable: bool,
    pub moved: usize,
    pub outputs: usize,
    pub target: Option<String>,
    pub workspace: String,
    pub at: u64,
    pub seconds: u64,
    pub changes: Vec<Change>,
    pub more_changes: usize,
    pub destroys: Vec<String>,
}

/// A run that has not ended.
struct Active {
    token: String,
    action: Action,
    created_at: u64,
    /// The plan this run makes or applies.
    plan: Option<PathBuf>,
}

#[derive(Default)]
struct Inner {
    /// By project id: at most one run each.
    active: HashMap<String, Active>,
    last: HashMap<String, RunInfo>,
    /// The plans read in this run of the server, by plan file.
    reduced: HashMap<PathBuf, Reduced>,
}

/// The runs and plans of every project.
#[derive(Default)]
pub struct Runs {
    state_dir: PathBuf,
    inner: Mutex<Inner>,
}

/// Whether a process with this id is alive.
#[cfg(unix)]
fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 sends nothing; it only asks whether the process is there.
    let sent = unsafe { libc::kill(pid as libc::pid_t, 0) };
    sent == 0 || io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(unix))]
fn alive(_pid: u32) -> bool {
    true
}

/// The addresses of the modules in a tree.
fn module_addresses(nodes: &[Node], out: &mut Vec<String>) {
    for node in nodes {
        if let Node::Module { address, children, .. } = node {
            out.push(address.clone());
            module_addresses(children, out);
        }
    }
}

impl Runs {
    pub fn new(state_dir: &Path) -> Self {
        Self { state_dir: state_dir.to_path_buf(), inner: Mutex::default() }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn is_current(meta: &PlanMeta, project: &Project) -> bool {
        !meta.fingerprint.is_empty() && plan::fingerprint(&meta.dirs, &project.workspace, project.tool) == meta.fingerprint
    }

    /// Starts a run: checks it, writes its spec, and gives the token and the
    /// pane's name. `tree` is the project's tree now (its modules are the
    /// targets a plan may name, its folders are what the plan is made from).
    ///
    /// `plan_at`: the time of the plan the user confirmed. An apply of any
    /// other plan (one made since, or one of another workspace) is refused.
    #[allow(clippy::too_many_arguments)]
    pub fn start(&self, project: &Project, action: Action, target: Option<&str>, tree: &Tree, shell_panes: bool, now: u64, plan_at: Option<u64>) -> Result<(String, String), Refusal> {
        if !shell_panes {
            return Err(Refusal::NoShell);
        }
        if !project.tool_found {
            return Err(Refusal::NoTool);
        }
        // One lock for the checks and the write: two requests cannot both start.
        let mut inner = self.lock();
        if inner.active.contains_key(&project.id) {
            return Err(Refusal::Running);
        }
        if let Some(target) = target {
            let mut modules = Vec::new();
            module_addresses(&tree.nodes, &mut modules);
            if action != Action::Plan || !modules.iter().any(|address| address == target) {
                return Err(Refusal::BadTarget);
            }
        }
        let plan = plan_path(&self.state_dir, &project.id, &project.workspace);
        let mut spec = RunSpec { project: project.id.clone(), action, tool: project.tool, cwd: project.path.clone(), args: vec![action.as_str().to_owned()], plan: None, json: None };
        let mut uses = None;
        match action {
            Action::Init | Action::Validate => {}
            // A plan that runs elsewhere cannot be saved; a target still narrows it.
            Action::Plan if project.remote => spec.args.extend(target.map(|target| format!("-target={target}"))),
            Action::Plan => {
                remove_plan(&plan);
                inner.reduced.remove(&plan);
                let folder = plan.parent().ok_or(Refusal::Internal)?;
                [root_dir(&self.state_dir), root_dir(&self.state_dir).join("plans"), folder.to_path_buf()].iter().try_for_each(|dir| private_dir(dir)).map_err(|_| Refusal::Internal)?;
                let meta = PlanMeta {
                    fingerprint: plan::fingerprint(&tree.local_dirs, &project.workspace, project.tool),
                    started_at: now,
                    ended_at: 0,
                    seconds: 0,
                    workspace: project.workspace.clone(),
                    tool: project.tool,
                    target: target.map(str::to_owned),
                    outcome: Outcome::Planning,
                    dirs: tree.local_dirs.clone(),
                    counts: None,
                };
                write_meta(&plan, &meta).map_err(|_| Refusal::Internal)?;
                spec.args.push(format!("-out={}", plan.display()));
                spec.args.extend(target.map(|target| format!("-target={target}")));
                (spec.plan, spec.json) = (Some(plan.clone()), Some(json_path(&plan)));
                uses = Some(plan);
            }
            Action::Apply => {
                if project.remote {
                    return Err(Refusal::Remote);
                }
                let ready = read_meta(&plan).filter(|meta| meta.outcome == Outcome::Ready && plan.is_file());
                let Some(mut meta) = ready else { return Err(Refusal::NoPlan) };
                let Some(reduced) = Self::reduced_of(&mut inner, &plan) else { return Err(Refusal::NoPlan) };
                if !reduced.applyable {
                    return Err(Refusal::NoPlan);
                }
                // The plan on the user's screen, and no other: another window may have planned since.
                if plan_at != Some(Self::at(&meta)) || !Self::is_current(&meta, project) {
                    return Err(Refusal::Stale);
                }
                meta.outcome = Outcome::Applying;
                write_meta(&plan, &meta).map_err(|_| Refusal::Internal)?;
                spec.args.push(plan.display().to_string());
                spec.plan = Some(plan.clone());
                uses = Some(plan);
            }
        }
        let token = super::super::random_hex(16);
        files::write_spec(&self.state_dir, &token, &spec).map_err(|_| Refusal::Internal)?;
        inner.active.insert(project.id.clone(), Active { token: token.clone(), action, created_at: now, plan: uses });
        Ok((token, format!("{} {}", project.name, action.as_str())))
    }

    /// Reads what the runners wrote and moves each run and its plan on.
    pub fn settle(&self, now: u64) {
        let mut inner = self.lock();
        let ids: Vec<String> = inner.active.keys().cloned().collect();
        for id in ids {
            let Some(run) = inner.active.get(&id) else { continue };
            let done = files::read_done(&self.state_dir, &run.token);
            let started = files::read_started(&self.state_dir, &run.token);
            let (state, at, done) = match (done, started) {
                (Some(done), _) => (if done.exit == 0 { RunState::Ok } else { RunState::Failed }, done.ended_at, Some(done)),
                (None, Some(started)) if !alive(started.pid) => match files::read_done(&self.state_dir, &run.token) {
                    // It ended between the two reads.
                    Some(done) => (if done.exit == 0 { RunState::Ok } else { RunState::Failed }, done.ended_at, Some(done)),
                    None => (RunState::Interrupted, now, None),
                },
                (None, Some(_)) => continue,
                (None, None) if now.saturating_sub(run.created_at) <= EXPIRES_MS => continue,
                (None, None) => {
                    // The pane never opened: as if the run had not been asked for.
                    let Some(run) = inner.active.remove(&id) else { continue };
                    let _ = fs::remove_file(files::spec_path(&self.state_dir, &run.token));
                    match (run.action, &run.plan) {
                        (Action::Apply, Some(plan)) => {
                            if let Some(mut meta) = read_meta(plan) {
                                meta.outcome = Outcome::Ready;
                                let _ = write_meta(plan, &meta);
                            }
                        }
                        (_, Some(plan)) => remove_plan(plan),
                        _ => {}
                    }
                    continue;
                }
            };
            let Some(run) = inner.active.remove(&id) else { continue };
            for path in [files::started_path(&self.state_dir, &run.token), files::done_path(&self.state_dir, &run.token), files::spec_path(&self.state_dir, &run.token)] {
                let _ = fs::remove_file(path);
            }
            if let Some(plan) = &run.plan {
                self.close_plan(&mut inner, plan, run.action, state, at, done.as_ref());
            }
            inner.last.insert(id, RunInfo { action: run.action, state, at });
        }
    }

    /// The plan as the page shows it: from memory, else from its file (a plan of before a restart).
    fn reduced_of(inner: &mut Inner, plan: &Path) -> Option<Reduced> {
        if !inner.reduced.contains_key(plan) {
            inner.reduced.insert(plan.to_path_buf(), read_reduced(plan)?);
        }
        inner.reduced.get(plan).cloned()
    }

    /// What the end of a plan or apply run does to the plan.
    fn close_plan(&self, inner: &mut Inner, plan: &Path, action: Action, state: RunState, at: u64, done: Option<&files::RunDone>) {
        let Some(mut meta) = read_meta(plan) else {
            // Nothing to close, but the JSON of a plan holds every value in plain text.
            let _ = fs::remove_file(json_path(plan));
            return;
        };
        meta.ended_at = at;
        match (action, state) {
            (Action::Plan, RunState::Ok) => {
                let json = json_path(plan);
                let reduced = done
                    .filter(|done| done.json_ok)
                    .and_then(|_| fs::metadata(&json).ok())
                    .filter(|file| json_fits(file.len()))
                    .and_then(|_| fs::read(&json).ok())
                    .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                    .and_then(|value| plan::reduce(&value, &PlanLimits::DEFAULT));
                // The JSON holds every value in plain text: it goes once it is reduced.
                let _ = fs::remove_file(&json);
                meta.seconds = at.saturating_sub(meta.started_at) / 1000;
                // The plan is for the workspace the runner saw. When that is not the
                // server's, or the tool did not say, the plan matches no code here
                // and cannot be applied from the panel.
                match done.and_then(|done| done.workspace.as_ref()) {
                    Some(actual) if *actual == meta.workspace => {}
                    Some(actual) => (meta.workspace, meta.fingerprint) = (actual.clone(), String::new()),
                    None => meta.fingerprint = String::new(),
                }
                match reduced {
                    Some(reduced) => {
                        (meta.outcome, meta.counts) = (Outcome::Ready, Some(reduced.counts));
                        write_reduced(plan, &reduced);
                        inner.reduced.insert(plan.to_path_buf(), reduced);
                    }
                    None => meta.outcome = Outcome::Unreadable,
                }
            }
            (Action::Plan, _) => {
                remove_plan(plan);
                inner.reduced.remove(plan);
                return;
            }
            (Action::Apply, state) => {
                let _ = fs::remove_file(plan);
                let _ = fs::remove_file(reduced_path(plan));
                inner.reduced.remove(plan);
                meta.outcome = if state == RunState::Ok { Outcome::Applied } else { Outcome::ApplyFailed };
            }
            _ => return,
        }
        let _ = write_meta(plan, &meta);
    }

    /// The project's running run, else its last one.
    pub fn run_info(&self, project: &str) -> Option<RunInfo> {
        let inner = self.lock();
        match inner.active.get(project) {
            Some(run) => Some(RunInfo { action: run.action, state: RunState::Running, at: run.created_at }),
            None => inner.last.get(project).cloned(),
        }
    }

    /// The project's plan in its workspace, with the reduced plan when this server read it.
    fn plan_of(&self, project: &Project) -> Option<(PlanMeta, PlanState, Option<Reduced>)> {
        let plan = plan_path(&self.state_dir, &project.id, &project.workspace);
        let meta = read_meta(&plan)?;
        let reduced = if meta.outcome == Outcome::Ready { Self::reduced_of(&mut self.lock(), &plan) } else { None };
        let state = match (meta.outcome, &reduced) {
            (Outcome::Planning | Outcome::Applying, _) => return None,
            (Outcome::Ready, Some(_)) if Self::is_current(&meta, project) => PlanState::Current,
            (Outcome::Ready, Some(_)) => PlanState::Stale,
            // Its reduced copy is gone or was too large to keep: only the tool that wrote the plan could read it again.
            (Outcome::Ready | Outcome::Unreadable, _) => PlanState::Unreadable,
            (Outcome::Applied, _) => PlanState::Applied,
            (Outcome::ApplyFailed, _) => PlanState::ApplyFailed,
        };
        Some((meta, state, reduced))
    }

    fn at(meta: &PlanMeta) -> u64 {
        if meta.ended_at > 0 { meta.ended_at } else { meta.started_at }
    }

    pub fn plan_info(&self, project: &Project) -> Option<PlanInfo> {
        let (meta, state, reduced) = self.plan_of(project)?;
        let counts = reduced.as_ref().map(|reduced| reduced.counts).or(meta.counts).filter(|_| state != PlanState::Unreadable);
        Some(PlanInfo { state, counts, applyable: reduced.is_some_and(|reduced| reduced.applyable), at: Self::at(&meta), target: meta.target })
    }

    pub fn plan_answer(&self, project: &Project) -> Option<PlanAnswer> {
        let (meta, state, reduced) = self.plan_of(project)?;
        let counts = reduced.as_ref().map(|reduced| reduced.counts).or(meta.counts).filter(|_| state != PlanState::Unreadable);
        let (at, seconds) = (Self::at(&meta), meta.seconds);
        let reduced = reduced.filter(|_| matches!(state, PlanState::Current | PlanState::Stale));
        Some(PlanAnswer {
            state,
            counts,
            applyable: reduced.as_ref().is_some_and(|reduced| reduced.applyable),
            moved: reduced.as_ref().map_or(0, |reduced| reduced.moved),
            outputs: reduced.as_ref().map_or(0, |reduced| reduced.outputs),
            target: meta.target,
            workspace: meta.workspace,
            at,
            seconds,
            more_changes: reduced.as_ref().map_or(0, |reduced| reduced.more_changes),
            destroys: reduced.as_ref().map(|reduced| reduced.destroys.clone()).unwrap_or_default(),
            changes: reduced.map(|reduced| reduced.changes).unwrap_or_default(),
        })
    }

    /// What the current plan does to the resources of the tree; nothing for any other plan.
    pub fn marks(&self, project: &Project) -> Vec<PlanMark> {
        match self.plan_of(project) {
            Some((_, PlanState::Current, Some(reduced))) => reduced.marks,
            _ => Vec::new(),
        }
    }

    /// Removes every saved plan, except one a run is using now.
    pub fn delete_all(&self) {
        let mut inner = self.lock();
        let used: Vec<PathBuf> = inner.active.values().filter_map(|run| run.plan.clone()).collect();
        for plan in saved_plans(&self.state_dir) {
            if !used.contains(&plan) {
                remove_plan(&plan);
                inner.reduced.remove(&plan);
            }
        }
    }

    /// The server starts: no run of an earlier server is waited for, a plan
    /// that was half way is closed, and old plans go.
    pub fn clean_at_start(state_dir: &Path, now: u64) {
        let _ = fs::remove_dir_all(root_dir(state_dir).join("runs"));
        for plan in saved_plans(state_dir) {
            let _ = fs::remove_file(json_path(&plan));
            match read_meta(&plan) {
                Some(meta) if meta.outcome == Outcome::Planning || now.saturating_sub(meta.started_at) > KEEP_MS => remove_plan(&plan),
                Some(mut meta) if meta.outcome == Outcome::Applying => {
                    let _ = fs::remove_file(&plan);
                    let _ = fs::remove_file(reduced_path(&plan));
                    meta.outcome = Outcome::ApplyFailed;
                    let _ = write_meta(&plan, &meta);
                }
                Some(_) => {}
                None => remove_plan(&plan),
            }
        }
    }
}

/// The plan files the plans folder speaks of (a plan, its JSON, or its meta), once each.
fn saved_plans(state_dir: &Path) -> Vec<PathBuf> {
    let folders = fs::read_dir(root_dir(state_dir).join("plans")).into_iter().flatten().filter_map(Result::ok);
    let mut plans: Vec<PathBuf> = folders
        .flat_map(|folder| fs::read_dir(folder.path()).into_iter().flatten().filter_map(Result::ok))
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            let stem = name.split('.').next()?;
            Some(path.with_file_name(format!("{stem}.tfplan")))
        })
        .collect();
    plans.sort();
    plans.dedup();
    plans
}

#[cfg(test)]
mod tests {
    use super::super::{scan, tree};
    use super::*;
    use crate::plugins::terraform::{read_started, spec_path, take_spec, write_done, write_started, RunDone, RunStarted};
    use serde_json::json;
    use std::fs;

    const NOW: u64 = 1_800_000_000_000;

    struct Env {
        _keep: tempfile::TempDir,
        root: PathBuf,
        runs: Runs,
        project: Project,
        tree: Tree,
    }

    fn write(dir: &Path, name: &str, text: &str) {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn env() -> Env {
        let keep = tempfile::tempdir().unwrap();
        let root = tree::canonical(keep.path()).unwrap();
        let path = root.join("w/envs/dev");
        write(&path, "main.tf", "provider \"aws\" {}\nmodule \"eks\" {\n  source = \"./eks\"\n}\n");
        write(&path, "eks/main.tf", "resource \"x\" \"y\" {}\n");
        fs::create_dir_all(root.join("state")).unwrap();
        let data_dir = path.join(".terraform");
        let tree = tree::build(&path, &data_dir, Tool::Terraform, &tree::Limits::DEFAULT);
        let project = Project {
            id: scan::project_id(&path),
            path,
            name: "envs/dev".into(),
            sub: "~/w".into(),
            tool: Tool::Terraform,
            tool_found: true,
            data_dir,
            workspace: "default".into(),
            workspace_locked: false,
            remote: false,
            needs_init: true,
            unreadable: Vec::new(),
            local_dirs: tree.local_dirs.clone(),
        };
        Env { runs: Runs::new(&root.join("state")), _keep: keep, root, project, tree }
    }

    impl Env {
        fn state(&self) -> PathBuf {
            self.root.join("state")
        }

        /// Starts a run; an apply names the plan the project shows now, as the page does.
        fn start(&self, action: Action, target: Option<&str>) -> Result<(String, String), Refusal> {
            let shown = self.runs.plan_info(&self.project).map(|plan| plan.at);
            self.runs.start(&self.project, action, target, &self.tree, true, NOW, shown.filter(|_| action == Action::Apply))
        }

        fn plan_file(&self) -> PathBuf {
            plan_path(&self.state(), &self.project.id, &self.project.workspace)
        }

        /// The runner's part: it started, wrote the plan and its JSON, and ended.
        fn finish(&self, token: &str, exit: i32, json: Option<serde_json::Value>) {
            assert!(take_spec(&self.state(), token).is_some(), "the runner reads the spec");
            write_started(&self.state(), token, &RunStarted { pid: std::process::id(), started_at: NOW + 100 }).unwrap();
            if let Some(json) = &json {
                fs::write(self.plan_file(), "plan bytes").unwrap();
                fs::write(self.plan_file().with_extension("json"), json.to_string()).unwrap();
            }
            write_done(&self.state(), token, &RunDone { exit, ended_at: NOW + 12_400, workspace: Some("default".into()), version: Some("1.9.8".into()), json_ok: json.is_some() }).unwrap();
            self.runs.settle(NOW + 13_000);
        }

        /// A finished plan with one resource to create.
        fn planned(&self) {
            let (token, _) = self.start(Action::Plan, None).unwrap();
            self.finish(&token, 0, Some(one_create()));
        }

        fn plan_state(&self) -> Option<PlanState> {
            self.runs.plan_info(&self.project).map(|plan| plan.state)
        }

        fn run_state(&self) -> Option<RunState> {
            self.runs.run_info(&self.project.id).map(|run| run.state)
        }
    }

    fn one_create() -> serde_json::Value {
        json!({ "format_version": "1.2", "resource_changes": [
            { "address": "module.eks.x.y", "module_address": "module.eks", "mode": "managed", "type": "x", "name": "y", "change": { "actions": ["create"], "before": null, "after": { "n": 1 } } },
        ] })
    }

    /// A pid that is not alive: a child that has ended and was waited for.
    #[cfg(unix)]
    fn dead_pid() -> u32 {
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        pid
    }

    #[test]
    fn a_plan_run_is_specified() {
        let env = env();
        let (token, name) = env.start(Action::Plan, None).unwrap();
        assert!(crate::plugins::terraform::is_token(&token));
        assert_eq!(name, "envs/dev plan");
        let spec = take_spec(&env.state(), &token).unwrap();
        assert_eq!(spec.args, ["plan".to_owned(), format!("-out={}", env.plan_file().display())]);
        assert_eq!((spec.cwd.clone(), spec.tool, spec.action), (env.project.path.clone(), Tool::Terraform, Action::Plan));
        assert_eq!(spec.project, env.project.id);
        assert_eq!(spec.plan, Some(env.plan_file()));
        assert_eq!(spec.json, Some(env.plan_file().with_extension("json")));
        assert!(env.plan_file().with_extension("meta.json").is_file());
        assert_eq!(env.run_state(), Some(RunState::Running));
        assert_eq!(env.plan_state(), None, "a plan that is being made is not a plan yet");
        let run = env.runs.run_info(&env.project.id).unwrap();
        assert_eq!(serde_json::to_value(&run).unwrap(), json!({ "action": "plan", "state": "running", "at": NOW }));
    }

    #[test]
    fn a_target_is_added_to_a_plan() {
        let env = env();
        let (token, _) = env.start(Action::Plan, Some("module.eks")).unwrap();
        assert_eq!(take_spec(&env.state(), &token).unwrap().args.last().map(String::as_str), Some("-target=module.eks"));
    }

    #[test]
    fn a_target_must_be_a_module_of_the_tree() {
        let env = env();
        for target in ["module.nope", "x.y", "module.eks.x.y", "module.eks; rm -rf ~", ""] {
            assert_eq!(env.start(Action::Plan, Some(target)), Err(Refusal::BadTarget), "{target}");
        }
        assert_eq!(env.start(Action::Init, Some("module.eks")), Err(Refusal::BadTarget));
        assert!(!env.state().join("terraform/runs").exists() || fs::read_dir(env.state().join("terraform/runs")).unwrap().count() == 0);
    }

    #[test]
    fn the_other_actions_are_plain_words() {
        for (action, word) in [(Action::Init, "init"), (Action::Validate, "validate")] {
            let env = env();
            let (token, name) = env.start(action, None).unwrap();
            let spec = take_spec(&env.state(), &token).unwrap();
            assert_eq!((spec.args, spec.plan, spec.json), (vec![word.to_owned()], None, None));
            assert_eq!(name, format!("envs/dev {word}"));
        }
    }

    #[test]
    fn a_run_needs_a_shell_and_the_tool() {
        let mut env = env();
        assert_eq!(env.runs.start(&env.project, Action::Plan, None, &env.tree, false, NOW, None), Err(Refusal::NoShell));
        env.project.tool_found = false;
        assert_eq!(env.start(Action::Plan, None), Err(Refusal::NoTool));
    }

    #[test]
    fn a_second_run_of_a_project_is_refused() {
        let env = env();
        env.start(Action::Plan, None).unwrap();
        assert_eq!(env.start(Action::Plan, None), Err(Refusal::Running));
        assert_eq!(env.start(Action::Init, None), Err(Refusal::Running));
        let specs = fs::read_dir(env.state().join("terraform/runs")).unwrap().count();
        assert_eq!(specs, 1);
        // Another project still starts.
        let mut other = env.project.clone();
        other.id = "f".repeat(16);
        assert!(env.runs.start(&other, Action::Validate, None, &env.tree, true, NOW, None).is_ok());
    }

    #[test]
    fn an_unstarted_run_expires_after_30_seconds() {
        let env = env();
        let (token, _) = env.start(Action::Plan, None).unwrap();
        env.runs.settle(NOW + 29_000);
        assert_eq!(env.run_state(), Some(RunState::Running));
        env.runs.settle(NOW + 31_000);
        assert_eq!(env.run_state(), None);
        assert!(!spec_path(&env.state(), &token).exists());
        assert!(!env.plan_file().with_extension("meta.json").exists());
        assert!(env.start(Action::Plan, None).is_ok());
    }

    #[test]
    fn a_started_run_does_not_expire() {
        let env = env();
        let (token, _) = env.start(Action::Plan, None).unwrap();
        take_spec(&env.state(), &token).unwrap();
        write_started(&env.state(), &token, &RunStarted { pid: std::process::id(), started_at: NOW }).unwrap();
        env.runs.settle(NOW + 600_000);
        assert_eq!(env.run_state(), Some(RunState::Running));
    }

    #[test]
    fn a_finished_plan_is_reduced_and_its_json_removed() {
        let env = env();
        let (token, _) = env.start(Action::Plan, None).unwrap();
        env.finish(&token, 0, Some(one_create()));
        let plan = env.runs.plan_info(&env.project).unwrap();
        assert_eq!(serde_json::to_value(&plan).unwrap(), json!({
            "state": "current",
            "counts": { "add": 1, "change": 0, "destroy": 0, "import": 0, "forget": 0 },
            "applyable": true,
            "at": NOW + 12_400,
            "target": null,
        }));
        assert!(!env.plan_file().with_extension("json").exists(), "the JSON holds secrets and goes once it is reduced");
        assert!(env.plan_file().is_file());
        let marks = env.runs.marks(&env.project);
        assert_eq!(serde_json::to_value(&marks).unwrap(), json!([{ "module": "module.eks", "type": "x", "name": "y", "mark": "create", "count": 1 }]));
        assert_eq!(serde_json::to_value(env.runs.run_info(&env.project.id).unwrap()).unwrap(), json!({ "action": "plan", "state": "ok", "at": NOW + 12_400 }));
        assert!(read_started(&env.state(), &token).is_none(), "the run's files are removed");
        let answer = serde_json::to_value(env.runs.plan_answer(&env.project).unwrap()).unwrap();
        assert_eq!(answer["state"], "current");
        assert_eq!((answer["workspace"].as_str(), answer["seconds"].as_u64(), answer["at"].as_u64()), (Some("default"), Some(12), Some(NOW + 12_400)));
        assert_eq!(answer["changes"][0]["address"], "module.eks.x.y");
        assert_eq!((answer["moved"].as_u64(), answer["outputs"].as_u64(), answer["moreChanges"].as_u64()), (Some(0), Some(0), Some(0)));
        assert_eq!((&answer["destroys"], &answer["target"]), (&json!([]), &json!(null)));
    }

    #[test]
    fn a_changed_file_makes_the_plan_stale() {
        let env = env();
        env.planned();
        assert_eq!(env.plan_state(), Some(PlanState::Current));
        write(&env.project.path, "main.tf", "provider \"aws\" {}\nmodule \"eks\" {\n  source = \"./eks\"\n}\n# edited\n");
        assert_eq!(env.plan_state(), Some(PlanState::Stale));
        assert!(env.runs.marks(&env.project).is_empty());
        assert_eq!(env.start(Action::Apply, None), Err(Refusal::Stale));
        // The numbers stay, to show them struck through.
        assert_eq!(env.runs.plan_info(&env.project).unwrap().counts.map(|counts| counts.add), Some(1));
    }

    #[test]
    fn a_changed_module_file_or_workspace_makes_the_plan_stale() {
        let env = env();
        env.planned();
        write(&env.project.path, "eks/main.tf", "resource \"x\" \"y\" {}\nresource \"x\" \"z\" {}\n");
        assert_eq!(env.plan_state(), Some(PlanState::Stale));
        let mut env = self::env();
        env.planned();
        env.project.workspace = "prod".into();
        assert_eq!(env.plan_state(), None, "each workspace has its own plan");
    }

    #[test]
    fn an_edit_during_the_plan_makes_it_stale() {
        let env = env();
        let (token, _) = env.start(Action::Plan, None).unwrap();
        write(&env.project.path, "main.tf", "provider \"aws\" {}\n# edited while the plan ran\n");
        env.finish(&token, 0, Some(one_create()));
        assert_eq!(env.plan_state(), Some(PlanState::Stale));
    }

    #[test]
    fn a_plan_made_in_another_workspace_is_stale() {
        let env = env();
        let (token, _) = env.start(Action::Plan, None).unwrap();
        take_spec(&env.state(), &token).unwrap();
        write_started(&env.state(), &token, &RunStarted { pid: std::process::id(), started_at: NOW }).unwrap();
        fs::write(env.plan_file(), "plan bytes").unwrap();
        fs::write(env.plan_file().with_extension("json"), one_create().to_string()).unwrap();
        write_done(&env.state(), &token, &RunDone { exit: 0, ended_at: NOW + 1000, workspace: Some("prod".into()), version: None, json_ok: true }).unwrap();
        env.runs.settle(NOW + 2000);
        assert_eq!(env.plan_state(), Some(PlanState::Stale));
        assert_eq!(env.runs.plan_answer(&env.project).unwrap().workspace, "prod");
        assert_eq!(env.start(Action::Apply, None), Err(Refusal::Stale));
    }

    #[test]
    fn apply_needs_a_current_plan_with_something_to_apply() {
        let env = env();
        assert_eq!(env.start(Action::Apply, None), Err(Refusal::NoPlan));
        let (token, _) = env.start(Action::Plan, None).unwrap();
        env.finish(&token, 0, Some(json!({ "format_version": "1.2", "resource_changes": [] })));
        assert_eq!(env.plan_state(), Some(PlanState::Current));
        assert_eq!(env.start(Action::Apply, None), Err(Refusal::NoPlan), "nothing to apply");
        env.planned();
        let (token, name) = env.start(Action::Apply, None).unwrap();
        assert_eq!(name, "envs/dev apply");
        let spec = take_spec(&env.state(), &token).unwrap();
        assert_eq!(spec.args, ["apply".to_owned(), env.plan_file().display().to_string()]);
        assert_eq!((spec.plan, spec.json), (Some(env.plan_file()), None));
        assert_eq!(env.plan_state(), None, "while it applies, the run shows");
        assert_eq!(env.run_state(), Some(RunState::Running));
    }

    #[test]
    fn an_apply_names_the_plan_the_user_confirmed() {
        let env = env();
        env.planned();
        let shown = env.runs.plan_info(&env.project).unwrap().at;
        let apply = |at: Option<u64>| env.runs.start(&env.project, Action::Apply, None, &env.tree, true, NOW, at);
        assert_eq!(apply(None), Err(Refusal::Stale), "an apply that names no plan is refused");
        assert_eq!(apply(Some(shown - 1)), Err(Refusal::Stale), "another plan than the one on the screen");
        assert!(env.plan_file().is_file() && env.plan_state() == Some(PlanState::Current), "a refusal leaves the plan as it was");
        assert!(apply(Some(shown)).is_ok());
    }

    #[test]
    fn a_plan_made_again_in_another_window_is_not_applied_by_an_old_dialog() {
        let env = env();
        env.planned();
        let old = env.runs.plan_info(&env.project).unwrap().at;
        // Another window plans again: the plan ends at another time.
        let (token, _) = env.start(Action::Plan, None).unwrap();
        take_spec(&env.state(), &token).unwrap();
        write_started(&env.state(), &token, &RunStarted { pid: std::process::id(), started_at: NOW }).unwrap();
        fs::write(env.plan_file(), "plan bytes").unwrap();
        fs::write(env.plan_file().with_extension("json"), one_create().to_string()).unwrap();
        write_done(&env.state(), &token, &RunDone { exit: 0, ended_at: NOW + 99_000, workspace: Some("default".into()), version: None, json_ok: true }).unwrap();
        env.runs.settle(NOW + 100_000);
        assert_eq!(env.plan_state(), Some(PlanState::Current));
        assert_eq!(env.runs.start(&env.project, Action::Apply, None, &env.tree, true, NOW, Some(old)), Err(Refusal::Stale));
    }

    #[test]
    fn a_plan_whose_workspace_is_not_known_cannot_be_applied() {
        let env = env();
        let (token, _) = env.start(Action::Plan, None).unwrap();
        take_spec(&env.state(), &token).unwrap();
        write_started(&env.state(), &token, &RunStarted { pid: std::process::id(), started_at: NOW }).unwrap();
        fs::write(env.plan_file(), "plan bytes").unwrap();
        fs::write(env.plan_file().with_extension("json"), one_create().to_string()).unwrap();
        // The tool did not say its workspace (it was slow, or Ctrl+C came while it was asked).
        write_done(&env.state(), &token, &RunDone { exit: 0, ended_at: NOW + 1000, workspace: None, version: None, json_ok: true }).unwrap();
        env.runs.settle(NOW + 2000);
        assert_eq!(env.plan_state(), Some(PlanState::Stale));
        assert_eq!(env.start(Action::Apply, None), Err(Refusal::Stale));
    }

    #[test]
    fn a_plan_is_shown_again_after_the_server_restarts() {
        let env = env();
        env.planned();
        let reduced = env.plan_file().with_extension("reduced.json");
        assert!(reduced.is_file());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&reduced).unwrap().permissions().mode() & 0o777, 0o600);
        }
        let fresh = Runs::new(&env.state());
        assert_eq!(fresh.plan_info(&env.project).map(|plan| plan.state), Some(PlanState::Current));
        assert_eq!(fresh.plan_answer(&env.project).unwrap().changes[0].address, "module.eks.x.y");
        assert_eq!(serde_json::to_value(fresh.marks(&env.project)).unwrap()[0]["mark"], "create");
        let shown = fresh.plan_info(&env.project).unwrap().at;
        assert!(fresh.start(&env.project, Action::Apply, None, &env.tree, true, NOW, Some(shown)).is_ok(), "and it can be applied");
        // Every way a plan goes takes the reduced file with it.
        let env = self::env();
        env.planned();
        env.start(Action::Plan, None).unwrap();
        assert!(!env.plan_file().with_extension("reduced.json").exists());
    }

    #[test]
    fn the_json_of_a_plan_goes_even_when_its_meta_is_gone() {
        let env = env();
        let (token, _) = env.start(Action::Plan, None).unwrap();
        take_spec(&env.state(), &token).unwrap();
        write_started(&env.state(), &token, &RunStarted { pid: std::process::id(), started_at: NOW }).unwrap();
        fs::write(env.plan_file().with_extension("json"), one_create().to_string()).unwrap();
        fs::remove_file(env.plan_file().with_extension("meta.json")).unwrap();
        write_done(&env.state(), &token, &RunDone { exit: 0, ended_at: NOW + 1000, workspace: Some("default".into()), version: None, json_ok: true }).unwrap();
        env.runs.settle(NOW + 2000);
        assert!(!env.plan_file().with_extension("json").exists(), "the JSON holds secrets");
    }

    #[test]
    fn a_remote_plan_keeps_its_target() {
        let mut env = env();
        env.project.remote = true;
        let (token, _) = env.start(Action::Plan, Some("module.eks")).unwrap();
        assert_eq!(take_spec(&env.state(), &token).unwrap().args, ["plan", "-target=module.eks"]);
    }

    #[test]
    fn an_apply_that_never_started_gives_the_plan_back() {
        let env = env();
        env.planned();
        env.start(Action::Apply, None).unwrap();
        env.runs.settle(NOW + 60_000);
        assert_eq!(env.plan_state(), Some(PlanState::Current));
        assert!(env.plan_file().is_file());
    }

    #[test]
    fn a_new_plan_removes_the_old_one_first() {
        let env = env();
        env.planned();
        assert!(env.plan_file().is_file());
        env.start(Action::Plan, None).unwrap();
        assert!(!env.plan_file().exists());
        assert_eq!(env.plan_state(), None);
        assert!(env.runs.marks(&env.project).is_empty());
    }

    #[test]
    fn apply_ends_remove_the_plan_file() {
        for (exit, state, run) in [(0, PlanState::Applied, RunState::Ok), (1, PlanState::ApplyFailed, RunState::Failed)] {
            let env = env();
            env.planned();
            let (token, _) = env.start(Action::Apply, None).unwrap();
            env.finish(&token, exit, None);
            assert_eq!(env.plan_state(), Some(state));
            assert_eq!(env.run_state(), Some(run));
            assert!(!env.plan_file().exists());
            assert!(env.runs.marks(&env.project).is_empty());
            let answer = env.runs.plan_answer(&env.project).unwrap();
            assert_eq!(answer.counts.map(|counts| counts.add), Some(1), "the numbers of what was applied stay");
            assert!(answer.changes.is_empty());
            assert_eq!(env.start(Action::Apply, None), Err(Refusal::NoPlan));
        }
    }

    // Whether a runner is alive is asked of the system on Unix only; runs are Unix only.
    #[cfg(unix)]
    #[test]
    fn a_vanished_apply_reads_as_interrupted_and_drops_the_plan() {
        let env = env();
        env.planned();
        let (token, _) = env.start(Action::Apply, None).unwrap();
        take_spec(&env.state(), &token).unwrap();
        write_started(&env.state(), &token, &RunStarted { pid: dead_pid(), started_at: NOW }).unwrap();
        env.runs.settle(NOW + 5000);
        assert_eq!(env.run_state(), Some(RunState::Interrupted));
        assert_eq!(env.plan_state(), Some(PlanState::ApplyFailed));
        assert!(!env.plan_file().exists());
        assert!(env.start(Action::Plan, None).is_ok(), "the project can run again");
    }

    #[test]
    fn a_failed_plan_leaves_no_plan() {
        let env = env();
        env.planned();
        let (token, _) = env.start(Action::Plan, None).unwrap();
        env.finish(&token, 1, None);
        assert_eq!(env.plan_state(), None);
        assert_eq!(env.run_state(), Some(RunState::Failed));
        assert!(!env.plan_file().with_extension("meta.json").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_vanished_plan_leaves_no_plan() {
        let env = env();
        let (token, _) = env.start(Action::Plan, None).unwrap();
        take_spec(&env.state(), &token).unwrap();
        write_started(&env.state(), &token, &RunStarted { pid: dead_pid(), started_at: NOW }).unwrap();
        env.runs.settle(NOW + 5000);
        assert_eq!((env.run_state(), env.plan_state()), (Some(RunState::Interrupted), None));
    }

    #[test]
    fn an_oversized_or_broken_json_reads_as_unreadable() {
        for body in ["{", "[]", r#"{"format_version":"1.2","resource_changes":3}"#] {
            let env = env();
            let (token, _) = env.start(Action::Plan, None).unwrap();
            take_spec(&env.state(), &token).unwrap();
            write_started(&env.state(), &token, &RunStarted { pid: std::process::id(), started_at: NOW }).unwrap();
            fs::write(env.plan_file(), "plan bytes").unwrap();
            fs::write(env.plan_file().with_extension("json"), body).unwrap();
            write_done(&env.state(), &token, &RunDone { exit: 0, ended_at: NOW + 1000, workspace: None, version: None, json_ok: true }).unwrap();
            env.runs.settle(NOW + 2000);
            assert_eq!(env.plan_state(), Some(PlanState::Unreadable), "{body}");
            assert!(!env.plan_file().with_extension("json").exists());
            assert_eq!(env.start(Action::Apply, None), Err(Refusal::NoPlan));
        }
        // The runner could not write the JSON at all.
        let env = env();
        let (token, _) = env.start(Action::Plan, None).unwrap();
        env.finish(&token, 0, None);
        assert_eq!(env.plan_state(), Some(PlanState::Unreadable));
        assert!(json_fits(MAX_JSON_BYTES) && !json_fits(MAX_JSON_BYTES + 1));
        assert_eq!(MAX_JSON_BYTES, 128 * 1024 * 1024);
    }

    #[test]
    fn remote_projects_plan_without_a_file_and_refuse_apply() {
        let mut env = env();
        env.project.remote = true;
        let (token, _) = env.start(Action::Plan, None).unwrap();
        let spec = take_spec(&env.state(), &token).unwrap();
        assert_eq!((spec.args, spec.plan, spec.json), (vec!["plan".to_owned()], None, None));
        assert!(!env.plan_file().with_extension("meta.json").exists());
        write_started(&env.state(), &token, &RunStarted { pid: std::process::id(), started_at: NOW }).unwrap();
        write_done(&env.state(), &token, &RunDone { exit: 0, ended_at: NOW + 1000, workspace: None, version: None, json_ok: false }).unwrap();
        env.runs.settle(NOW + 2000);
        assert_eq!((env.run_state(), env.plan_state()), (Some(RunState::Ok), None));
        assert_eq!(env.start(Action::Apply, None), Err(Refusal::Remote));
    }

    #[test]
    fn deleting_the_plans_keeps_the_one_in_use() {
        let env = env();
        env.planned();
        env.runs.delete_all();
        assert_eq!(env.plan_state(), None);
        assert!(!env.plan_file().exists());
        env.planned();
        env.start(Action::Apply, None).unwrap();
        env.runs.delete_all();
        assert!(env.plan_file().is_file(), "an apply is reading it");
    }

    #[test]
    fn plans_older_than_seven_days_go_at_start() {
        let env = env();
        env.planned();
        let day = 24 * 60 * 60 * 1000;
        Runs::clean_at_start(&env.state(), NOW + 6 * day);
        assert!(env.plan_file().is_file());
        assert!(!env.state().join("terraform/runs").exists(), "no run of an earlier server is waited for");
        Runs::clean_at_start(&env.state(), NOW + 8 * day);
        assert!(!env.plan_file().exists());
        assert!(!env.plan_file().with_extension("meta.json").exists());
    }

    #[test]
    fn a_plan_left_half_way_by_an_earlier_server_is_closed_at_start() {
        let env = env();
        env.start(Action::Plan, None).unwrap();
        Runs::clean_at_start(&env.state(), NOW);
        assert!(!env.plan_file().with_extension("meta.json").exists(), "a plan that was being made");
        let env = self::env();
        env.planned();
        env.start(Action::Apply, None).unwrap();
        Runs::clean_at_start(&env.state(), NOW);
        assert!(!env.plan_file().exists());
        let fresh = Runs::new(&env.state());
        assert_eq!(fresh.plan_info(&env.project).map(|plan| plan.state), Some(PlanState::ApplyFailed));
    }

    #[test]
    fn a_plan_without_its_reduced_file_reads_as_unreadable() {
        let env = env();
        env.planned();
        fs::remove_file(env.plan_file().with_extension("reduced.json")).unwrap();
        let fresh = Runs::new(&env.state());
        assert_eq!(fresh.plan_info(&env.project).map(|plan| plan.state), Some(PlanState::Unreadable));
        assert_eq!(fresh.start(&env.project, Action::Apply, None, &env.tree, true, NOW, None), Err(Refusal::NoPlan));
    }

    #[cfg(unix)]
    #[test]
    fn plan_folders_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let env = env();
        env.start(Action::Plan, None).unwrap();
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(env.plan_file().parent().unwrap()), 0o700);
        assert_eq!(mode(&env.state().join("terraform/plans")), 0o700);
        assert_eq!(mode(&env.plan_file().with_extension("meta.json")), 0o600);
    }

    #[test]
    fn refusals_have_their_codes() {
        let codes: Vec<&str> = [Refusal::Running, Refusal::NoPlan, Refusal::Stale, Refusal::Remote, Refusal::NoTool, Refusal::NoShell, Refusal::BadTarget].iter().map(|refusal| refusal.code()).collect();
        assert_eq!(codes, ["running", "noPlan", "stale", "remote", "noTool", "noShell", "badTarget"]);
        assert!([Refusal::Running, Refusal::NoPlan, Refusal::Stale, Refusal::Remote].iter().all(|refusal| refusal.conflict()));
        assert!(![Refusal::NoTool, Refusal::NoShell, Refusal::BadTarget].iter().any(|refusal| refusal.conflict()));
    }
}
