//! Finding projects: the folders, in and below the session folders, where a
//! tool runs (root modules). Only folders and `.tf` files are read.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::tree::{self, Limits};
use crate::plugins::terraform::Tool;

/// Folders the scan does not enter, besides those whose name starts with a dot.
const SKIPPED: [&str; 4] = ["node_modules", "vendor", "target", "dist"];

/// Which tools are on the login shell's PATH.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Tools {
    pub terraform: bool,
    pub tofu: bool,
}

impl Tools {
    pub fn has(&self, tool: Tool) -> bool {
        match tool {
            Tool::Terraform => self.terraform,
            Tool::Tofu => self.tofu,
        }
    }
}

/// What a scan needs to know besides the folders.
#[derive(Clone, Debug, Default)]
pub struct ScanEnv {
    pub home: PathBuf,
    pub tools: Tools,
    /// `TF_WORKSPACE` of the login environment.
    pub tf_workspace: Option<String>,
    /// `TF_DATA_DIR` of the login environment.
    pub tf_data_dir: Option<String>,
    /// The tool the user chose for a project, by its folder.
    pub overrides: HashMap<PathBuf, Tool>,
}

/// How far a scan goes.
#[derive(Clone, Copy, Debug)]
pub struct ScanLimits {
    /// Folder levels below a start folder.
    pub depth: usize,
    /// Folder entries read below one start folder.
    pub entries: usize,
    /// For the whole scan.
    pub time: Duration,
    pub projects: usize,
}

impl ScanLimits {
    pub const DEFAULT: Self = Self { depth: 3, entries: 5000, time: Duration::from_secs(3), projects: 32 };
}

/// A folder where a tool runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub id: String,
    /// Canonical.
    pub path: PathBuf,
    pub name: String,
    pub sub: String,
    pub tool: Tool,
    /// Whether `tool` is installed.
    pub tool_found: bool,
    /// `.terraform`, or what `TF_DATA_DIR` names.
    pub data_dir: PathBuf,
    pub workspace: String,
    /// `TF_WORKSPACE` sets the workspace.
    pub workspace_locked: bool,
    /// Runs happen elsewhere (a `cloud` block, `backend "remote"`).
    pub remote: bool,
    pub needs_init: bool,
    /// Files of the project's own folder that could not be read.
    pub unreadable: Vec<String>,
    /// The project's folder and the folders of its local modules.
    pub local_dirs: Vec<PathBuf>,
}

/// The id of the project in `path`: the first 16 hex digits of the path's SHA-256.
///
/// The path is read part by part, as `Path` compares paths: two spellings of
/// one folder (`\` and `/` on Windows, a doubled or trailing `/`) give one id.
pub fn project_id(path: &Path) -> String {
    let mut hasher = Sha256::new();
    for part in path.components() {
        hasher.update(part.as_os_str().to_string_lossy().as_bytes());
        hasher.update([0]);
    }
    hasher.finalize()[..8].iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The tool for the project in `dir`, and whether it is installed. The user's
/// choice wins while that tool is installed; else `.tofu` files or
/// `.opentofu-version` mean OpenTofu; and a tool that is missing gives way to
/// the other one.
pub fn pick_tool(dir: &Path, tools: &Tools, chosen: Option<Tool>) -> (Tool, bool) {
    if let Some(tool) = chosen.filter(|tool| tools.has(*tool)) {
        return (tool, true);
    }
    let tofu_files = || fs::read_dir(dir).is_ok_and(|entries| entries.filter_map(Result::ok).any(|entry| entry.file_name().to_string_lossy().ends_with(".tofu")));
    let wanted = if dir.join(".opentofu-version").is_file() || tofu_files() { Tool::Tofu } else { Tool::Terraform };
    let other = if wanted == Tool::Tofu { Tool::Terraform } else { Tool::Tofu };
    match (tools.has(wanted), tools.has(other)) {
        (true, _) => (wanted, true),
        (false, true) => (other, true),
        (false, false) => (wanted, false),
    }
}

/// Where the project's tool keeps its data: `.terraform`, or `TF_DATA_DIR` (relative to the project).
pub fn data_dir(project: &Path, tf_data_dir: Option<&str>) -> PathBuf {
    project.join(tf_data_dir.filter(|dir| !dir.is_empty()).unwrap_or(".terraform"))
}

/// The workspace in use and whether `TF_WORKSPACE` sets it. Without the
/// variable it is what `<data_dir>/environment` says, else `default`.
pub fn workspace_of(data_dir: &Path, tf_workspace: Option<&str>) -> (String, bool) {
    if let Some(name) = tf_workspace.filter(|name| !name.is_empty()) {
        return (name.to_owned(), true);
    }
    let saved = fs::read_to_string(data_dir.join("environment")).ok().map(|text| text.trim().to_owned()).filter(|name| !name.is_empty());
    (saved.unwrap_or_else(|| "default".to_owned()), false)
}

/// `path` with the home folder as `~`.
fn tilde(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~/{}", rest.to_string_lossy().replace('\\', "/")),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

/// The nearest folder at or above `project` that holds `.git`, not above the home folder.
fn repository_root(project: &Path, home: &Path) -> Option<PathBuf> {
    for folder in project.ancestors() {
        if folder.join(".git").exists() {
            return Some(folder.to_path_buf());
        }
        if folder == home {
            break;
        }
    }
    None
}

/// A project's name and the dimmer text beside it: its path from the
/// repository root (else from the start folder), and that root. The root
/// itself is named by its folder, beside its parent.
pub fn names(project: &Path, start: &Path, home: &Path) -> (String, String) {
    let base = repository_root(project, home).unwrap_or_else(|| start.to_path_buf());
    match project.strip_prefix(&base) {
        Ok(rest) if !rest.as_os_str().is_empty() => (rest.to_string_lossy().replace('\\', "/"), tilde(&base, home)),
        _ => {
            let name = project.file_name().map_or_else(|| project.to_string_lossy().into_owned(), |name| name.to_string_lossy().into_owned());
            (name, project.parent().map_or_else(String::new, |parent| tilde(parent, home)))
        }
    }
}

/// The projects in and below `folders`, by path, and whether a limit cut the
/// scan short. A folder that another project uses as a local module is not a
/// project.
pub fn scan(folders: &[PathBuf], env: &ScanEnv, limits: &ScanLimits) -> (Vec<Project>, bool) {
    let deadline = Instant::now() + limits.time;
    let mut truncated = false;
    let mut projects: Vec<Project> = Vec::new();
    'starts: for start in folders {
        let Some(start) = tree::canonical(start) else { continue };
        let mut entries = 0;
        let mut queue = VecDeque::from([(start.clone(), 0usize)]);
        while let Some((dir, depth)) = queue.pop_front() {
            if Instant::now() >= deadline {
                truncated = true;
                break 'starts;
            }
            let Ok(listing) = fs::read_dir(&dir) else { continue };
            let mut has_files = false;
            let mut below = Vec::new();
            for entry in listing.filter_map(Result::ok) {
                entries += 1;
                if entries > limits.entries {
                    truncated = true;
                    continue 'starts;
                }
                let name = entry.file_name().to_string_lossy().into_owned();
                // Links to folders are not followed: they can loop.
                if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    if !name.starts_with('.') && !SKIPPED.contains(&name.as_str()) {
                        below.push(entry.path());
                    }
                } else if name.ends_with(".tf") || name.ends_with(".tofu") {
                    has_files = true;
                }
            }
            if has_files
                && !projects.iter().any(|project| project.path == dir)
                && let Some(project) = project_in(&dir, &start, dir == start, env)
            {
                projects.push(project);
            }
            if depth < limits.depth {
                below.sort();
                queue.extend(below.into_iter().map(|folder| (folder, depth + 1)));
            }
        }
    }
    let modules: Vec<PathBuf> = projects.iter().flat_map(|project| project.local_dirs.iter().skip(1).cloned()).collect();
    projects.retain(|project| !modules.contains(&project.path));
    projects.sort_by(|a, b| a.path.cmp(&b.path));
    if projects.len() > limits.projects {
        projects.truncate(limits.projects);
        truncated = true;
    }
    (projects, truncated)
}

/// The project in `dir`, when the folder is one (the conditions of the spec's G2).
fn project_in(dir: &Path, start: &Path, is_start: bool, env: &ScanEnv) -> Option<Project> {
    let (tool, tool_found) = pick_tool(dir, &env.tools, env.overrides.get(dir).copied());
    let content = tree::read_dir(dir, tool);
    let data_dir = data_dir(dir, env.tf_data_dir.as_deref());
    let initialized = data_dir.is_dir();
    let signal = initialized || dir.join(".terraform.lock.hcl").is_file() || content.backend || content.provider || is_start;
    if !content.has_files || !signal {
        return None;
    }
    let (name, sub) = names(dir, start, &env.home);
    let (workspace, workspace_locked) = workspace_of(&data_dir, env.tf_workspace.as_deref());
    let local_dirs = tree::build(dir, &data_dir, tool, &Limits::DEFAULT).local_dirs;
    Some(Project {
        id: project_id(dir),
        path: dir.to_path_buf(),
        name,
        sub,
        tool,
        tool_found,
        data_dir,
        workspace,
        workspace_locked,
        remote: content.remote,
        needs_init: !initialized,
        unreadable: content.unreadable,
        local_dirs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::Instant;

    const PROVIDER: &str = "provider \"aws\" {}\n";
    const RESOURCE: &str = "resource \"x\" \"y\" {}\n";
    /// The default limits with time to spare: a slow disk must not cut these scans short.
    const PATIENT: ScanLimits = ScanLimits { time: Duration::from_secs(120), ..ScanLimits::DEFAULT };

    fn write(dir: &Path, name: &str, text: &str) {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// A temp folder (canonical) that stands for the home folder.
    fn home() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = tree::canonical(dir.path()).unwrap();
        (dir, path)
    }

    fn env(home: &Path) -> ScanEnv {
        ScanEnv { home: home.to_path_buf(), tools: Tools { terraform: true, tofu: true }, tf_workspace: None, tf_data_dir: None, overrides: HashMap::new() }
    }

    fn found(root: &Path, start: &str) -> Vec<String> {
        let (projects, _) = scan(&[root.join(start)], &env(root), &PATIENT);
        // With `/` on every system.
        projects.iter().map(|project| project.path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/")).collect()
    }

    #[test]
    fn each_signal_makes_a_project() {
        let (_keep, root) = home();
        write(&root, "w/data/main.tf", RESOURCE);
        fs::create_dir_all(root.join("w/data/.terraform")).unwrap();
        write(&root, "w/lock/main.tf", RESOURCE);
        write(&root, "w/lock/.terraform.lock.hcl", "");
        write(&root, "w/backend/main.tf", "terraform {\n  backend \"s3\" {}\n}\n");
        write(&root, "w/provider/main.tf", PROVIDER);
        write(&root, "w/plain/main.tf", RESOURCE);
        write(&root, "w/empty/README.md", "x");
        assert_eq!(found(&root, "w"), ["w/backend", "w/data", "w/lock", "w/provider"]);
    }

    #[test]
    fn the_start_folder_is_a_project_when_it_has_files() {
        let (_keep, root) = home();
        write(&root, "plain/main.tf", RESOURCE);
        assert_eq!(found(&root, "plain"), ["plain"]);
        fs::create_dir_all(root.join("none")).unwrap();
        assert!(found(&root, "none").is_empty());
        assert!(found(&root, "missing").is_empty());
    }

    #[test]
    fn a_local_module_folder_is_not_a_project() {
        let (_keep, root) = home();
        write(&root, "w/envs/dev/main.tf", &format!("{PROVIDER}module \"eks\" {{\n  source = \"../../modules/eks\"\n}}\n"));
        write(&root, "w/modules/eks/main.tf", PROVIDER);
        assert_eq!(found(&root, "w"), ["w/envs/dev"]);
    }

    #[test]
    fn skipped_folders_are_not_entered() {
        let (_keep, root) = home();
        for folder in [".hidden", "node_modules", "vendor", "target", "dist"] {
            write(&root, &format!("w/{folder}/main.tf"), PROVIDER);
        }
        write(&root, "w/ok/main.tf", PROVIDER);
        assert_eq!(found(&root, "w"), ["w/ok"]);
    }

    #[test]
    fn the_scan_goes_three_levels_down() {
        let (_keep, root) = home();
        write(&root, "w/a/b/c/main.tf", PROVIDER);
        write(&root, "w/a/b/c/d/main.tf", PROVIDER);
        assert_eq!(found(&root, "w"), ["w/a/b/c"]);
    }

    #[test]
    fn a_scan_stops_at_the_entry_budget() {
        let (_keep, root) = home();
        for i in 0..200 {
            write(&root, &format!("w/f{i:03}/main.tf"), PROVIDER);
        }
        let started = Instant::now();
        let limits = ScanLimits { entries: 50, ..PATIENT };
        let (projects, truncated) = scan(&[root.join("w")], &env(&root), &limits);
        assert!(truncated);
        assert!(projects.len() < 50, "{}", projects.len());
        // It stopped at the budget, not at the time limit (which is far away here).
        assert!(started.elapsed() < Duration::from_secs(60));
    }

    #[test]
    fn a_scan_stops_at_its_time() {
        let (_keep, root) = home();
        for i in 0..40 {
            write(&root, &format!("w/f{i:03}/main.tf"), PROVIDER);
        }
        let limits = ScanLimits { time: Duration::ZERO, ..ScanLimits::DEFAULT };
        let (projects, truncated) = scan(&[root.join("w")], &env(&root), &limits);
        assert!(truncated);
        assert!(projects.is_empty());
    }

    #[test]
    fn at_most_32_projects_are_listed() {
        let (_keep, root) = home();
        for i in 0..40 {
            write(&root, &format!("w/f{i:03}/main.tf"), PROVIDER);
        }
        let (projects, truncated) = scan(&[root.join("w")], &env(&root), &PATIENT);
        assert_eq!(projects.len(), 32);
        assert!(truncated);
        assert_eq!(projects[0].path, root.join("w/f000"));
        let (few, whole) = scan(&[root.join("w/f000"), root.join("w/f001")], &env(&root), &PATIENT);
        assert_eq!((few.len(), whole), (2, false));
    }

    #[test]
    fn overlapping_folders_list_a_project_once() {
        let (_keep, root) = home();
        write(&root, "w/envs/dev/main.tf", PROVIDER);
        let (projects, _) = scan(&[root.join("w"), root.join("w/envs/dev"), root.join("w/envs")], &env(&root), &PATIENT);
        assert_eq!(projects.len(), 1);
    }

    #[test]
    fn names_come_from_the_repository_root() {
        let (_keep, root) = home();
        fs::create_dir_all(root.join("acme-infra/.git")).unwrap();
        fs::create_dir_all(root.join("acme-infra/envs/dev")).unwrap();
        fs::create_dir_all(root.join("x/y")).unwrap();
        let name = |project: &str, start: &str| names(&root.join(project), &root.join(start), &root);
        assert_eq!(name("acme-infra/envs/dev", "acme-infra/envs"), ("envs/dev".to_owned(), "~/acme-infra".to_owned()));
        assert_eq!(name("acme-infra", "acme-infra"), ("acme-infra".to_owned(), "~".to_owned()));
        assert_eq!(name("x/y", "x"), ("y".to_owned(), "~/x".to_owned()));
        assert_eq!(name("x", "x"), ("x".to_owned(), "~".to_owned()));
        // Outside the home folder, paths are whole.
        let (_other, outside) = home();
        fs::create_dir_all(outside.join("p/q")).unwrap();
        assert_eq!(names(&outside.join("p/q"), &outside.join("p"), &root), ("q".to_owned(), outside.join("p").to_string_lossy().into_owned()));
    }

    #[test]
    fn the_tool_follows_the_files_then_what_is_installed() {
        let (_keep, root) = home();
        write(&root, "tofu/main.tofu", RESOURCE);
        write(&root, "pinned/main.tf", RESOURCE);
        write(&root, "pinned/.opentofu-version", "1.8.5");
        write(&root, "plain/main.tf", RESOURCE);
        let both = Tools { terraform: true, tofu: true };
        assert_eq!(pick_tool(&root.join("tofu"), &both, None), (Tool::Tofu, true));
        assert_eq!(pick_tool(&root.join("pinned"), &both, None), (Tool::Tofu, true));
        assert_eq!(pick_tool(&root.join("plain"), &both, None), (Tool::Terraform, true));
        assert_eq!(pick_tool(&root.join("plain"), &Tools { terraform: false, tofu: true }, None), (Tool::Tofu, true));
        assert_eq!(pick_tool(&root.join("tofu"), &Tools { terraform: true, tofu: false }, None), (Tool::Terraform, true));
        assert_eq!(pick_tool(&root.join("plain"), &Tools { terraform: false, tofu: false }, None), (Tool::Terraform, false));
        assert_eq!(pick_tool(&root.join("plain"), &both, Some(Tool::Tofu)), (Tool::Tofu, true));
        // A chosen tool that is not installed is not used.
        assert_eq!(pick_tool(&root.join("plain"), &Tools { terraform: true, tofu: false }, Some(Tool::Tofu)), (Tool::Terraform, true));
    }

    #[test]
    fn the_workspace_comes_from_the_environment_file_or_the_variable() {
        let (_keep, root) = home();
        write(&root, "p/main.tf", PROVIDER);
        let data = data_dir(&root.join("p"), None);
        assert_eq!(data, root.join("p/.terraform"));
        assert_eq!(workspace_of(&data, None), ("default".to_owned(), false));
        write(&root, "p/.terraform/environment", "dev\n");
        assert_eq!(workspace_of(&data, None), ("dev".to_owned(), false));
        assert_eq!(workspace_of(&data, Some("prod")), ("prod".to_owned(), true));
        write(&root, "p/.terraform/environment", "  \n");
        assert_eq!(workspace_of(&data, None), ("default".to_owned(), false));
        assert_eq!(data_dir(&root.join("p"), Some(".tfdata")), root.join("p/.tfdata"));
        #[cfg(unix)]
        assert_eq!(data_dir(&root.join("p"), Some("/abs/data")), PathBuf::from("/abs/data"));
    }

    #[test]
    fn a_project_carries_what_the_panel_shows() {
        let (_keep, root) = home();
        fs::create_dir_all(root.join("acme/.git")).unwrap();
        write(&root, "acme/envs/dev/main.tf", "terraform {\n  cloud {}\n}\nmodule \"m\" {\n  source = \"./m\"\n}\n");
        write(&root, "acme/envs/dev/m/main.tf", RESOURCE);
        write(&root, "acme/envs/dev/bad.tf", "resource {");
        write(&root, "acme/envs/dev/.tfdata/environment", "stage");
        let mut scan_env = env(&root);
        scan_env.tf_data_dir = Some(".tfdata".to_owned());
        scan_env.overrides.insert(root.join("acme/envs/dev"), Tool::Tofu);
        let (projects, truncated) = scan(&[root.join("acme")], &scan_env, &PATIENT);
        assert!(!truncated);
        let [project] = projects.as_slice() else { panic!("{} projects", projects.len()) };
        assert_eq!(project.path, root.join("acme/envs/dev"));
        assert_eq!(project.id, project_id(&root.join("acme/envs/dev")));
        assert_eq!((project.name.as_str(), project.sub.as_str()), ("envs/dev", "~/acme"));
        assert_eq!((project.tool, project.tool_found), (Tool::Tofu, true));
        assert_eq!(project.data_dir, root.join("acme/envs/dev/.tfdata"));
        assert_eq!((project.workspace.as_str(), project.workspace_locked), ("stage", false));
        assert!(project.remote);
        assert!(!project.needs_init, "the data folder is there");
        assert_eq!(project.unreadable, ["bad.tf"]);
        assert_eq!(project.local_dirs, [root.join("acme/envs/dev"), root.join("acme/envs/dev/m")]);
        // Without TF_DATA_DIR the project has no data folder.
        let (plain, _) = scan(&[root.join("acme")], &env(&root), &PATIENT);
        assert!(plain[0].needs_init);
        assert_eq!(plain[0].workspace, "default");
    }

    #[test]
    fn the_same_path_spelled_another_way_has_the_same_id() {
        // These are one `Path`; the id must not tell them apart.
        let id = project_id(Path::new("/w/envs/dev"));
        for spelled in ["/w/envs//dev", "/w/envs/./dev", "/w/envs/dev/"] {
            assert_eq!(Path::new(spelled), Path::new("/w/envs/dev"), "{spelled}");
            assert_eq!(project_id(Path::new(spelled)), id, "{spelled}");
        }
        #[cfg(windows)]
        assert_eq!(project_id(Path::new(r"C:\w\envs\dev")), project_id(Path::new("C:/w/envs/dev")));
    }

    #[test]
    fn ids_are_16_hex_and_stable() {
        let id = project_id(Path::new("/w/envs/dev"));
        assert_eq!(id.len(), 16);
        assert!(id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')));
        assert_eq!(id, project_id(Path::new("/w/envs/dev")));
        assert_ne!(id, project_id(Path::new("/w/envs/prod")));
        assert_eq!((ScanLimits::DEFAULT.depth, ScanLimits::DEFAULT.entries, ScanLimits::DEFAULT.time, ScanLimits::DEFAULT.projects), (3, 5000, Duration::from_secs(3), 32));
    }
}
