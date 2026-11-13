//! What a project's code says: the `module` and `resource` blocks of its
//! `.tf` files, and the tree they make with the modules below them. Only
//! files are read; no tool runs and nothing is asked of a backend.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::plugins::terraform::Tool;

/// A `.tf` file larger than this is not read.
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// A `module` block. `source` and `version` are None unless the code gives a plain string.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleCall {
    pub name: String,
    pub source: Option<String>,
    pub version: Option<String>,
}

/// A `resource` block: its type and name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceDecl {
    pub kind: String,
    pub name: String,
}

/// What the files directly in a folder declare.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DirContent {
    pub modules: Vec<ModuleCall>,
    pub resources: Vec<ResourceDecl>,
    /// A `terraform` block holds a `backend` or a `cloud` block.
    pub backend: bool,
    /// Runs happen elsewhere: a `cloud` block, or `backend "remote"`.
    pub remote: bool,
    /// A `provider` block.
    pub provider: bool,
    /// The folder has files the tool reads.
    pub has_files: bool,
    /// The names of files that could not be read (a syntax error, too large).
    pub unreadable: Vec<String>,
}

/// Whether the tool merges this file into the others instead of adding to them.
fn is_override(stem: &str) -> bool {
    stem == "override" || stem.ends_with("_override")
}

/// The files of `dir` the tool reads, by name: (name, is an override file).
/// Terraform reads `.tf`. OpenTofu reads `.tf` and `.tofu`, and `a.tofu` hides `a.tf`.
fn tool_files(dir: &Path, tool: Tool) -> Vec<(String, bool)> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| !kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    let tofu_stems: Vec<&str> = names.iter().filter_map(|name| name.strip_suffix(".tofu")).collect();
    let mut files: Vec<(String, bool)> = names
        .iter()
        .filter_map(|name| {
            if let Some(stem) = name.strip_suffix(".tofu") {
                return (tool == Tool::Tofu).then(|| (name.clone(), is_override(stem)));
            }
            let stem = name.strip_suffix(".tf")?;
            (tool == Tool::Terraform || !tofu_stems.contains(&stem)).then(|| (name.clone(), is_override(stem)))
        })
        .collect();
    files.sort();
    files
}

/// The value of the attribute `key` when the code gives a plain string.
fn plain_string(body: &hcl::Body, key: &str) -> Option<String> {
    body.attributes().find(|attribute| attribute.key() == key).and_then(|attribute| match attribute.expr() {
        hcl::Expression::String(text) => Some(text.clone()),
        _ => None,
    })
}

/// What the files directly in `dir` declare, as `tool` reads them. Files are
/// taken by name; override files come last and only change modules that exist.
pub fn read_dir(dir: &Path, tool: Tool) -> DirContent {
    let mut content = DirContent::default();
    let mut files = tool_files(dir, tool);
    content.has_files = !files.is_empty();
    files.sort_by_key(|(_, overrides)| *overrides);
    for (name, overrides) in files {
        let path = dir.join(&name);
        let body = fs::metadata(&path)
            .ok()
            .filter(|meta| meta.len() <= MAX_FILE_BYTES)
            .and_then(|_| fs::read_to_string(&path).ok())
            .and_then(|text| hcl::parse(&text).ok());
        let Some(body) = body else {
            content.unreadable.push(name);
            continue;
        };
        for block in body.blocks() {
            let labels: Vec<&str> = block.labels().iter().map(hcl::BlockLabel::as_str).collect();
            match (block.identifier(), labels.as_slice()) {
                ("module", [name]) => {
                    let (source, version) = (plain_string(block.body(), "source"), plain_string(block.body(), "version"));
                    let has = |key: &str| block.body().attributes().any(|attribute| attribute.key() == key);
                    match content.modules.iter_mut().find(|module| module.name == *name) {
                        Some(module) if overrides => {
                            if has("source") {
                                module.source = source;
                            }
                            if has("version") {
                                module.version = version;
                            }
                        }
                        None if !overrides => content.modules.push(ModuleCall { name: (*name).to_owned(), source, version }),
                        _ => {}
                    }
                }
                ("resource", [kind, name]) if !overrides => {
                    let resource = ResourceDecl { kind: (*kind).to_owned(), name: (*name).to_owned() };
                    if !content.resources.contains(&resource) {
                        content.resources.push(resource);
                    }
                }
                ("terraform", _) => {
                    for inner in block.body().blocks() {
                        let first = inner.labels().first().map(hcl::BlockLabel::as_str);
                        match inner.identifier() {
                            "cloud" => (content.backend, content.remote) = (true, true),
                            "backend" => {
                                content.backend = true;
                                content.remote |= first == Some("remote");
                            }
                            _ => {}
                        }
                    }
                }
                ("provider", _) => content.provider = true,
                _ => {}
            }
        }
    }
    content
}

/// Where a module's code comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Local,
    Registry,
    Git,
    Other,
}

pub fn source_kind(source: &str) -> SourceKind {
    if source.starts_with("./") || source.starts_with("../") {
        SourceKind::Local
    } else if ["git::", "git@", "github.com/", "bitbucket.org/"].iter().any(|prefix| source.starts_with(prefix)) {
        SourceKind::Git
    } else if registry_parts(source).is_some() {
        SourceKind::Registry
    } else {
        SourceKind::Other
    }
}

/// The namespace, name, and provider of a public registry source
/// (`<namespace>/<name>/<provider>`, with or without `//<subfolder>`). A
/// source with a host name is a private registry: None.
pub fn registry_parts(source: &str) -> Option<(String, String, String)> {
    let address = source.split_once("//").map_or(source, |(address, _)| address);
    let part = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    match address.split('/').collect::<Vec<_>>().as_slice() {
        [namespace, name, provider] if part(namespace) && part(name) && part(provider) => {
            Some(((*namespace).to_owned(), (*name).to_owned(), (*provider).to_owned()))
        }
        _ => None,
    }
}

/// The `ref` of a git source's query.
pub fn git_ref(source: &str) -> Option<String> {
    let (_, query) = source.split_once('?')?;
    query.split('&').find_map(|pair| pair.strip_prefix("ref=")).filter(|value| !value.is_empty()).map(str::to_owned)
}

/// How far the tree goes.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Module levels below the project.
    pub depth: usize,
    pub modules: usize,
    pub resources: usize,
}

impl Limits {
    pub const DEFAULT: Self = Self { depth: 8, modules: 200, resources: 5000 };
}

/// A row of the tree, in the shape the page reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Node {
    Module {
        name: String,
        address: String,
        source: SourceKind,
        /// The version, `git · <ref>`, or `local`.
        label: Option<String>,
        /// A newer version in the public registry; the registry check fills it.
        newer: Option<String>,
        /// The module's folder, when it is known.
        dir: Option<PathBuf>,
        children: Vec<Node>,
    },
    Resource {
        address: String,
        #[serde(rename = "type")]
        kind: String,
        name: String,
    },
}

/// A module of the public registry, to ask for newer versions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryUse {
    pub address: String,
    pub namespace: String,
    pub name: String,
    pub provider: String,
    /// The installed version (`modules.json`); None before init.
    pub resolved: Option<String>,
}

/// A project's tree and what was met on the way.
#[derive(Clone, Debug, Default)]
pub struct Tree {
    pub nodes: Vec<Node>,
    /// A limit cut the tree short.
    pub truncated: bool,
    /// The project's folder and the folders of its local modules, once each.
    pub local_dirs: Vec<PathBuf>,
    pub registry: Vec<RegistryUse>,
}

/// An installed module, from `modules.json`.
struct Installed {
    version: Option<String>,
    dir: PathBuf,
}

/// `<data_dir>/modules/modules.json` by module key (`vpc`, `eks.node_group`).
fn installed_modules(project: &Path, data_dir: &Path) -> HashMap<String, Installed> {
    let read = || -> Option<serde_json::Value> { serde_json::from_slice(&fs::read(data_dir.join("modules").join("modules.json")).ok()?).ok() };
    let Some(value) = read() else { return HashMap::new() };
    let modules = value.get("Modules").and_then(|modules| modules.as_array()).cloned().unwrap_or_default();
    modules
        .iter()
        .filter_map(|module| {
            let key = module.get("Key")?.as_str()?.to_owned();
            let dir = project.join(module.get("Dir")?.as_str()?);
            let version = module.get("Version").and_then(|version| version.as_str()).filter(|version| !version.is_empty()).map(str::to_owned);
            Some((key, Installed { version, dir }))
        })
        .collect()
}

struct Walk<'a> {
    tool: Tool,
    limits: &'a Limits,
    installed: HashMap<String, Installed>,
    tree: Tree,
    modules: usize,
    resources: usize,
    /// The folders from the project down to the one being read.
    above: Vec<PathBuf>,
}

impl Walk<'_> {
    /// The rows of `dir`: modules by name, then resources by type and name.
    /// `key` and `address` are those of the module `dir` belongs to (empty for the project).
    fn rows(&mut self, dir: &Path, key: &str, address: &str, depth: usize) -> Vec<Node> {
        let mut content = read_dir(dir, self.tool);
        content.modules.sort_by(|a, b| a.name.cmp(&b.name));
        content.resources.sort_by(|a, b| (&a.kind, &a.name).cmp(&(&b.kind, &b.name)));
        let join = |prefix: &str, rest: String| if prefix.is_empty() { rest } else { format!("{prefix}.{rest}") };
        let mut nodes = Vec::new();
        for module in content.modules {
            if self.modules == self.limits.modules {
                self.tree.truncated = true;
                break;
            }
            self.modules += 1;
            let own_key = join(key, module.name.clone());
            let own_address = join(address, format!("module.{}", module.name));
            let kind = module.source.as_deref().map_or(SourceKind::Other, source_kind);
            let installed = self.installed.get(&own_key);
            let folder = match (kind, &module.source) {
                (SourceKind::Local, Some(source)) => Some(dir.join(source)),
                _ => installed.map(|installed| installed.dir.clone()),
            };
            let folder = folder.and_then(|folder| fs::canonicalize(folder).ok()).filter(|folder| folder.is_dir());
            let resolved = installed.and_then(|installed| installed.version.clone());
            let label = match kind {
                SourceKind::Local => Some("local".to_owned()),
                SourceKind::Registry => resolved.clone().or(module.version.clone()),
                SourceKind::Git => Some(module.source.as_deref().and_then(git_ref).map_or("git".to_owned(), |reference| format!("git · {reference}"))),
                SourceKind::Other => None,
            };
            if let Some((namespace, name, provider)) = module.source.as_deref().filter(|_| kind == SourceKind::Registry).and_then(registry_parts) {
                self.tree.registry.push(RegistryUse { address: own_address.clone(), namespace, name, provider, resolved });
            }
            let mut children = Vec::new();
            if let Some(folder) = &folder {
                if kind == SourceKind::Local && !self.tree.local_dirs.contains(folder) {
                    self.tree.local_dirs.push(folder.clone());
                }
                if depth < self.limits.depth && !self.above.contains(folder) {
                    self.above.push(folder.clone());
                    children = self.rows(folder, &own_key, &own_address, depth + 1);
                    self.above.pop();
                }
            }
            nodes.push(Node::Module { name: module.name, address: own_address, source: kind, label, newer: None, dir: folder, children });
        }
        for resource in content.resources {
            if self.resources == self.limits.resources {
                self.tree.truncated = true;
                break;
            }
            self.resources += 1;
            nodes.push(Node::Resource { address: join(address, format!("{}.{}", resource.kind, resource.name)), kind: resource.kind, name: resource.name });
        }
        nodes
    }
}

/// The tree of the project in `project`, whose tool keeps its data in `data_dir`.
pub fn build(project: &Path, data_dir: &Path, tool: Tool, limits: &Limits) -> Tree {
    let root = fs::canonicalize(project).unwrap_or_else(|_| project.to_path_buf());
    let mut walk = Walk {
        tool,
        limits,
        installed: installed_modules(&root, data_dir),
        tree: Tree { local_dirs: vec![root.clone()], ..Tree::default() },
        modules: 0,
        resources: 0,
        above: vec![root.clone()],
    };
    let nodes = walk.rows(&root, "", "", 1);
    Tree { nodes, ..walk.tree }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn write(dir: &Path, name: &str, text: &str) {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn names(content: &DirContent) -> Vec<String> {
        content.resources.iter().map(|resource| format!("{}.{}", resource.kind, resource.name)).collect()
    }

    /// The addresses of a tree, depth first, each with its label and whether it has a folder.
    fn flat(nodes: &[Node]) -> Vec<String> {
        nodes
            .iter()
            .flat_map(|node| match node {
                Node::Module { address, label, dir, children, .. } => {
                    let own = format!("{address} [{}]{}", label.as_deref().unwrap_or("-"), if dir.is_some() { "" } else { " no-dir" });
                    std::iter::once(own).chain(flat(children)).collect::<Vec<_>>()
                }
                Node::Resource { address, .. } => vec![address.clone()],
            })
            .collect()
    }

    #[test]
    fn blocks_are_read_from_tf_files() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "main.tf",
            r#"
terraform {
  required_version = ">= 1.5"
  backend "s3" {
    bucket = "state"
  }
}
provider "aws" {
  region = "eu-west-1"
}
module "vpc" {
  source  = "terraform-aws-modules/vpc/aws"
  version = "5.13.0"
  cidr    = var.cidr
}
resource "aws_route53_record" "app" {
  name = "app.${var.zone}"
}
data "aws_ami" "x" {}
"#,
        );
        write(dir.path(), "notes.txt", "resource \"a\" \"b\" {}");
        let content = read_dir(dir.path(), Tool::Terraform);
        assert_eq!(content.modules.len(), 1);
        let vpc = &content.modules[0];
        assert_eq!((vpc.name.as_str(), vpc.source.as_deref(), vpc.version.as_deref()), ("vpc", Some("terraform-aws-modules/vpc/aws"), Some("5.13.0")));
        assert_eq!(names(&content), ["aws_route53_record.app"]);
        assert!(content.backend && content.provider && content.has_files);
        assert!(!content.remote);
        assert!(content.unreadable.is_empty());
    }

    #[test]
    fn a_folder_without_tf_files_has_no_files() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "README.md", "x");
        write(dir.path(), "sub/main.tf", "resource \"a\" \"b\" {}");
        let content = read_dir(dir.path(), Tool::Terraform);
        assert!(!content.has_files);
        assert!(names(&content).is_empty());
        assert!(!read_dir(&dir.path().join("missing"), Tool::Terraform).has_files);
    }

    #[test]
    fn cloud_and_remote_backends_are_remote() {
        let dir = tempfile::tempdir().unwrap();
        let read = |text: &str| {
            write(dir.path(), "main.tf", text);
            read_dir(dir.path(), Tool::Terraform)
        };
        let cloud = read("terraform {\n  cloud {\n    organization = \"acme\"\n  }\n}");
        assert!(cloud.remote && cloud.backend);
        let remote = read("terraform {\n  backend \"remote\" {\n    organization = \"acme\"\n  }\n}");
        assert!(remote.remote && remote.backend);
        let s3 = read("terraform {\n  backend \"s3\" {}\n}");
        assert!(!s3.remote && s3.backend);
        let none = read("terraform {\n  required_version = \">= 1.5\"\n}");
        assert!(!none.remote && !none.backend);
    }

    #[test]
    fn a_module_with_a_computed_source_has_no_label() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "main.tf",
            r#"
module "a" {
  source  = var.src
  version = local.v
}
module "b" {
  source = "${path.module}/x"
}
resource "null_resource" "after" {}
"#,
        );
        let content = read_dir(dir.path(), Tool::Terraform);
        for module in &content.modules {
            assert_eq!((module.source.as_deref(), module.version.as_deref()), (None, None), "{}", module.name);
        }
        let tree = build(dir.path(), &dir.path().join(".terraform"), Tool::Terraform, &Limits::DEFAULT);
        assert_eq!(flat(&tree.nodes), ["module.a [-] no-dir", "module.b [-] no-dir", "null_resource.after"]);
        assert!(tree.registry.is_empty());
    }

    #[test]
    fn a_broken_file_is_skipped_and_named() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "bad.tf", "resource \"a\" {");
        write(dir.path(), "main.tf", "resource \"x\" \"y\" {}");
        let content = read_dir(dir.path(), Tool::Terraform);
        assert_eq!(content.unreadable, ["bad.tf"]);
        assert_eq!(names(&content), ["x.y"]);
        assert!(content.has_files);
    }

    #[test]
    fn tofu_files_win_over_tf_files_for_tofu_only() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a.tf", "resource \"x\" \"from_tf\" {}");
        write(dir.path(), "a.tofu", "resource \"x\" \"from_tofu\" {}");
        write(dir.path(), "b.tf", "resource \"x\" \"b\" {}");
        let mut tofu = names(&read_dir(dir.path(), Tool::Tofu));
        tofu.sort();
        assert_eq!(tofu, ["x.b", "x.from_tofu"]);
        let mut terraform = names(&read_dir(dir.path(), Tool::Terraform));
        terraform.sort();
        assert_eq!(terraform, ["x.b", "x.from_tf"]);
    }

    #[test]
    fn override_files_change_a_module_and_add_no_rows() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "main.tf", "module \"vpc\" {\n  source  = \"terraform-aws-modules/vpc/aws\"\n  version = \"5.13.0\"\n}");
        write(dir.path(), "main_override.tf", "module \"vpc\" {\n  version = \"5.16.0\"\n}\nmodule \"new\" {\n  source = \"./n\"\n}\nresource \"x\" \"y\" {}");
        write(dir.path(), "override.tf", "resource \"x\" \"z\" {}");
        let content = read_dir(dir.path(), Tool::Terraform);
        assert_eq!(content.modules.len(), 1);
        assert_eq!((content.modules[0].source.as_deref(), content.modules[0].version.as_deref()), (Some("terraform-aws-modules/vpc/aws"), Some("5.16.0")));
        assert!(names(&content).is_empty());
    }

    #[test]
    fn sources_are_classified() {
        assert_eq!(source_kind("./m"), SourceKind::Local);
        assert_eq!(source_kind("../../modules/m"), SourceKind::Local);
        assert_eq!(source_kind("terraform-aws-modules/vpc/aws"), SourceKind::Registry);
        assert_eq!(registry_parts("terraform-aws-modules/vpc/aws"), Some(("terraform-aws-modules".into(), "vpc".into(), "aws".into())));
        assert_eq!(source_kind("terraform-aws-modules/vpc/aws//modules/x"), SourceKind::Registry);
        assert_eq!(registry_parts("terraform-aws-modules/vpc/aws//modules/x"), Some(("terraform-aws-modules".into(), "vpc".into(), "aws".into())));
        assert_eq!(source_kind("app.terraform.io/acme/vpc/aws"), SourceKind::Other);
        assert_eq!(registry_parts("app.terraform.io/acme/vpc/aws"), None);
        assert_eq!(source_kind("git::https://x/y.git?ref=v1.4.0"), SourceKind::Git);
        assert_eq!(git_ref("git::https://x/y.git?ref=v1.4.0"), Some("v1.4.0".into()));
        assert_eq!(git_ref("git::https://x/y.git?depth=1&ref=main"), Some("main".into()));
        assert_eq!(git_ref("git::https://x/y.git"), None);
        assert_eq!(source_kind("git@github.com:a/b.git"), SourceKind::Git);
        assert_eq!(source_kind("github.com/a/b"), SourceKind::Git);
        assert_eq!(source_kind("bitbucket.org/a/b"), SourceKind::Git);
        assert_eq!(source_kind("https://example.com/m.zip"), SourceKind::Other);
        assert_eq!(source_kind("s3::https://bucket/m.zip"), SourceKind::Other);
        assert_eq!(registry_parts("a/b"), None);
        assert_eq!(registry_parts("a//b/c"), None);
    }

    /// A project with a local module that holds another, and a registry module.
    fn project(dir: &Path) {
        write(dir, "main.tf", "module \"eks\" {\n  source = \"./modules/eks\"\n}\nmodule \"vpc\" {\n  source  = \"terraform-aws-modules/vpc/aws\"\n  version = \"~> 5.0\"\n}\nmodule \"rds\" {\n  source = \"git::https://example.com/rds.git?ref=v1.4.0\"\n}\nresource \"aws_route53_record\" \"app\" {}");
        write(dir, "modules/eks/main.tf", "module \"node_group\" {\n  source = \"./ng\"\n}\nresource \"aws_eks_cluster\" \"this\" {}");
        write(dir, "modules/eks/ng/main.tf", "resource \"aws_eks_node_group\" \"this\" {}");
    }

    #[test]
    fn the_tree_follows_local_and_installed_modules() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        project(&root);
        write(&root, ".terraform/modules/vpc/main.tf", "resource \"aws_vpc\" \"this\" {}\nresource \"aws_subnet\" \"private\" {}");
        write(
            &root,
            ".terraform/modules/modules.json",
            r#"{"Modules":[{"Key":"","Source":"","Dir":"."},{"Key":"vpc","Source":"registry.terraform.io/terraform-aws-modules/vpc/aws","Version":"5.13.0","Dir":".terraform/modules/vpc"},{"Key":"eks","Source":"./modules/eks","Dir":"modules/eks"}]}"#,
        );
        let tree = build(&root, &root.join(".terraform"), Tool::Terraform, &Limits::DEFAULT);
        assert_eq!(
            flat(&tree.nodes),
            [
                "module.eks [local]",
                "module.eks.module.node_group [local]",
                "module.eks.module.node_group.aws_eks_node_group.this",
                "module.eks.aws_eks_cluster.this",
                "module.rds [git · v1.4.0] no-dir",
                "module.vpc [5.13.0]",
                "module.vpc.aws_subnet.private",
                "module.vpc.aws_vpc.this",
                "aws_route53_record.app",
            ]
        );
        assert!(!tree.truncated);
        let dir_of = |wanted: &str| {
            tree.nodes.iter().find_map(|node| match node {
                Node::Module { address, dir, .. } if address == wanted => dir.clone(),
                _ => None,
            })
        };
        assert_eq!(dir_of("module.eks"), Some(root.join("modules/eks")));
        assert_eq!(dir_of("module.vpc"), Some(root.join(".terraform/modules/vpc")));
        assert_eq!(tree.registry.len(), 1);
        let used = &tree.registry[0];
        assert_eq!((used.address.as_str(), used.namespace.as_str(), used.name.as_str(), used.provider.as_str(), used.resolved.as_deref()), ("module.vpc", "terraform-aws-modules", "vpc", "aws", Some("5.13.0")));
        assert_eq!(tree.local_dirs, [root.clone(), root.join("modules/eks"), root.join("modules/eks/ng")]);
    }

    #[test]
    fn the_tree_serializes_as_the_wire_shape() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        write(&root, "main.tf", "module \"m\" {\n  source = \"./m\"\n}\nresource \"aws_vpc\" \"this\" {}");
        write(&root, "m/main.tf", "");
        let tree = build(&root, &root.join(".terraform"), Tool::Terraform, &Limits::DEFAULT);
        assert_eq!(
            serde_json::to_value(&tree.nodes).unwrap(),
            serde_json::json!([
                { "kind": "module", "name": "m", "address": "module.m", "source": "local", "label": "local", "newer": null, "dir": root.join("m"), "children": [] },
                { "kind": "resource", "address": "aws_vpc.this", "type": "aws_vpc", "name": "this" },
            ])
        );
    }

    #[test]
    fn a_module_without_an_installed_folder_has_no_children() {
        let dir = tempfile::tempdir().unwrap();
        project(dir.path());
        let tree = build(dir.path(), &dir.path().join(".terraform"), Tool::Terraform, &Limits::DEFAULT);
        assert!(flat(&tree.nodes).contains(&"module.vpc [~> 5.0] no-dir".to_owned()), "{:?}", flat(&tree.nodes));
        assert_eq!(tree.registry[0].resolved, None);
    }

    #[test]
    fn the_same_module_folder_can_be_used_twice() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "main.tf", "module \"a\" {\n  source = \"./m\"\n}\nmodule \"b\" {\n  source = \"./m\"\n}");
        write(dir.path(), "m/main.tf", "resource \"x\" \"y\" {}");
        let tree = build(dir.path(), &dir.path().join(".terraform"), Tool::Terraform, &Limits::DEFAULT);
        assert_eq!(flat(&tree.nodes), ["module.a [local]", "module.a.x.y", "module.b [local]", "module.b.x.y"]);
        assert_eq!(tree.local_dirs.len(), 2);
    }

    #[test]
    fn a_module_cycle_stops() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "a/main.tf", "module \"b\" {\n  source = \"../b\"\n}");
        write(dir.path(), "b/main.tf", "module \"a\" {\n  source = \"../a\"\n}\nresource \"x\" \"y\" {}");
        let tree = build(&dir.path().join("a"), &dir.path().join("a/.terraform"), Tool::Terraform, &Limits::DEFAULT);
        assert_eq!(flat(&tree.nodes), ["module.b [local]", "module.b.module.a [local]", "module.b.x.y"]);
    }

    #[test]
    fn the_tree_stops_at_its_limits() {
        let dir = tempfile::tempdir().unwrap();
        let modules: String = (0..4).map(|i| format!("module \"m{i}\" {{\n  source = \"./m\"\n}}\n")).collect();
        let resources: String = (0..5).map(|i| format!("resource \"x\" \"r{i}\" {{}}\n")).collect();
        write(dir.path(), "main.tf", &format!("{modules}{resources}"));
        write(dir.path(), "m/main.tf", "");
        let count = |nodes: &[Node]| {
            let modules = nodes.iter().filter(|node| matches!(node, Node::Module { .. })).count();
            (modules, nodes.len() - modules)
        };
        let limited = build(dir.path(), &dir.path().join(".terraform"), Tool::Terraform, &Limits { depth: 8, modules: 2, resources: 3 });
        assert!(limited.truncated);
        assert_eq!(count(&limited.nodes), (2, 3));
        let whole = build(dir.path(), &dir.path().join(".terraform"), Tool::Terraform, &Limits::DEFAULT);
        assert!(!whole.truncated);
        assert_eq!(count(&whole.nodes), (4, 5));
        // Depth 1 lists the project's own modules and reads none of them.
        write(dir.path(), "m/main.tf", "resource \"x\" \"inner\" {}");
        let shallow = build(dir.path(), &dir.path().join(".terraform"), Tool::Terraform, &Limits { depth: 1, modules: 200, resources: 5000 });
        assert!(!flat(&shallow.nodes).iter().any(|row| row.ends_with("x.inner")));
        assert_eq!((Limits::DEFAULT.depth, Limits::DEFAULT.modules, Limits::DEFAULT.resources), (8, 200, 5000));
    }
}
