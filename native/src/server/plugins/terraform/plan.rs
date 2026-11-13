//! A plan as the page shows it: the JSON of `show -json`, cut down to the
//! resources that change, their changed attributes, and the counts. Values a
//! tool marks sensitive never leave this file. Also the fingerprint that says
//! whether the code changed after a plan.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::plugins::terraform::Tool;

/// How much of a plan goes to the page.
#[derive(Clone, Copy, Debug)]
pub struct PlanLimits {
    /// The text of one value, in bytes.
    pub value_bytes: usize,
    /// Attribute rows of one change.
    pub attrs: usize,
    pub changes: usize,
}

impl PlanLimits {
    pub const DEFAULT: Self = Self { value_bytes: 2048, attrs: 200, changes: 2000 };
}

/// The numbers of a plan, counted as the tool prints them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub add: usize,
    pub change: usize,
    pub destroy: usize,
    pub import: usize,
    pub forget: usize,
}

impl Counts {
    pub fn total(&self) -> usize {
        self.add + self.change + self.destroy + self.import + self.forget
    }
}

/// What a plan does to a resource in the tree, weakest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mark {
    Create,
    Update,
    Replace,
    Delete,
}

/// What a plan does to a resource instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Create,
    Update,
    Delete,
    Replace,
    Import,
    Forget,
}

impl Action {
    fn mark(self) -> Option<Mark> {
        match self {
            Self::Create => Some(Mark::Create),
            Self::Update => Some(Mark::Update),
            Self::Replace => Some(Mark::Replace),
            Self::Delete => Some(Mark::Delete),
            Self::Import | Self::Forget => None,
        }
    }
}

/// One side of an attribute as the page shows it. A sensitive value has no text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum AttrValue {
    Value { text: String, cut: bool },
    Unknown,
    Sensitive,
}

/// An attribute that changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attr {
    pub path: String,
    pub before: Option<AttrValue>,
    pub after: Option<AttrValue>,
    /// The change of this attribute is why the resource is replaced.
    pub forces: bool,
}

/// A resource instance that changes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub address: String,
    /// The module's address; empty for the project itself.
    pub module: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    pub action: Action,
    pub attrs: Vec<Attr>,
    /// Attribute rows left out.
    pub more_attrs: usize,
}

/// What a plan does to a resource of the tree: the strongest of its instances.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanMark {
    pub module: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    pub mark: Mark,
    /// The resource's instances in the plan, changed or not.
    pub count: usize,
}

/// A plan as the page shows it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reduced {
    pub counts: Counts,
    /// Whether applying the plan would do anything.
    pub applyable: bool,
    /// Resources that only get a new address.
    pub moved: usize,
    /// Outputs that change.
    pub outputs: usize,
    pub changes: Vec<Change>,
    /// Changes left out.
    pub more_changes: usize,
    /// The addresses a plan destroys (delete and replace).
    pub destroys: Vec<String>,
    pub marks: Vec<PlanMark>,
}

/// One step of an attribute's path.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Step {
    Key(String),
    Index(usize),
}

fn path_text(steps: &[Step]) -> String {
    let plain = |key: &str| {
        let mut chars = key.chars();
        chars.next().is_some_and(|first| first.is_ascii_alphabetic() || first == '_') && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    };
    let mut text = String::new();
    for step in steps {
        match step {
            Step::Key(key) if plain(key) => {
                if !text.is_empty() {
                    text.push('.');
                }
                text.push_str(key);
            }
            Step::Key(key) => text.push_str(&format!("[{}]", Value::String(key.clone()))),
            Step::Index(index) => text.push_str(&format!("[{index}]")),
        }
    }
    text
}

/// A value at the end of a path, before it is turned into text.
#[derive(Clone, Debug, PartialEq)]
enum Leaf {
    Plain(Value),
    /// Marked sensitive; the value is kept only to see whether it changes.
    Sensitive(Value),
    Unknown,
}

impl Leaf {
    fn raw(&self) -> Option<&Value> {
        match self {
            Self::Plain(value) | Self::Sensitive(value) => Some(value),
            Self::Unknown => None,
        }
    }

    fn shown(&self, limits: &PlanLimits) -> AttrValue {
        match self {
            Self::Sensitive(_) => AttrValue::Sensitive,
            Self::Unknown => AttrValue::Unknown,
            Self::Plain(value) => {
                let mut text = value.to_string();
                let cut = text.len() > limits.value_bytes;
                if cut {
                    let mut end = limits.value_bytes;
                    while !text.is_char_boundary(end) {
                        end -= 1;
                    }
                    text.truncate(end);
                }
                AttrValue::Value { text, cut }
            }
        }
    }
}

type Leaves = BTreeMap<String, (Vec<Step>, Leaf)>;

/// The leaves of `value`. Where either of `sensitive` (the marks of the
/// value before and of the value after) says true, the whole value below is
/// one sensitive leaf: nothing under it shows, not even its keys. A value
/// that is sensitive on one side of a change is hidden on both. A null reads
/// as nothing.
fn leaves(value: &Value, sensitive: [Option<&Value>; 2], steps: &mut Vec<Step>, out: &mut Leaves) {
    if value.is_null() {
        return;
    }
    if sensitive.contains(&Some(&Value::Bool(true))) {
        out.insert(path_text(steps), (steps.clone(), Leaf::Sensitive(value.clone())));
        return;
    }
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (key, inner) in map {
                steps.push(Step::Key(key.clone()));
                leaves(inner, sensitive.map(|flags| flags.and_then(|flags| flags.get(key))), steps, out);
                steps.pop();
            }
        }
        Value::Array(items) if !items.is_empty() => {
            for (index, inner) in items.iter().enumerate() {
                steps.push(Step::Index(index));
                leaves(inner, sensitive.map(|flags| flags.and_then(|flags| flags.get(index))), steps, out);
                steps.pop();
            }
        }
        _ => {
            out.insert(path_text(steps), (steps.clone(), Leaf::Plain(value.clone())));
        }
    }
}

/// The paths `after_unknown` marks true.
fn unknowns(flags: &Value, steps: &mut Vec<Step>, out: &mut Leaves) {
    match flags {
        Value::Bool(true) => {
            out.insert(path_text(steps), (steps.clone(), Leaf::Unknown));
        }
        Value::Object(map) => {
            for (key, inner) in map {
                steps.push(Step::Key(key.clone()));
                unknowns(inner, steps, out);
                steps.pop();
            }
        }
        Value::Array(items) => {
            for (index, inner) in items.iter().enumerate() {
                steps.push(Step::Index(index));
                unknowns(inner, steps, out);
                steps.pop();
            }
        }
        _ => {}
    }
}

/// The paths of `replace_paths`.
fn replace_paths(change: &Value) -> Vec<Vec<Step>> {
    let step = |value: &Value| match value {
        Value::String(key) => Some(Step::Key(key.clone())),
        Value::Number(index) => index.as_u64().map(|index| Step::Index(index as usize)),
        _ => None,
    };
    let paths = change.get("replace_paths").and_then(Value::as_array).cloned().unwrap_or_default();
    paths.iter().filter_map(|path| path.as_array()?.iter().map(step).collect::<Option<Vec<Step>>>()).collect()
}

/// The attribute rows of a change, by path, and how many were left out.
fn attrs(action: Action, change: &Value, limits: &PlanLimits) -> (Vec<Attr>, usize) {
    if !matches!(action, Action::Create | Action::Update | Action::Replace) {
        return (Vec::new(), 0);
    }
    let marks = [change.get("before_sensitive"), change.get("after_sensitive")];
    let side = |key: &str| {
        let mut out = Leaves::new();
        if let Some(value) = change.get(key) {
            leaves(value, marks, &mut Vec::new(), &mut out);
        }
        out
    };
    let before = if action == Action::Create { Leaves::new() } else { side("before") };
    let mut after = side("after");
    if let Some(flags) = change.get("after_unknown") {
        unknowns(flags, &mut Vec::new(), &mut after);
    }
    // A value that is unknown after the apply covers what was below it before:
    // those old values change to "unknown", not to nothing, and the row of the
    // whole value is left out when its parts are listed.
    let unknown: Vec<Vec<Step>> = after.values().filter(|(_, leaf)| *leaf == Leaf::Unknown).map(|(steps, _)| steps.clone()).collect();
    for (path, (steps, _)) in &before {
        if let Some(prefix) = unknown.iter().find(|prefix| steps.len() > prefix.len() && steps.starts_with(prefix)) {
            after.remove(&path_text(prefix));
            after.insert(path.clone(), (steps.clone(), Leaf::Unknown));
        }
    }
    let forced = replace_paths(change);
    let mut paths: Vec<&String> = before.keys().chain(after.keys()).collect();
    paths.sort();
    paths.dedup();
    let mut rows = Vec::new();
    let mut more = 0;
    for path in paths {
        let (old, new) = (before.get(path), after.get(path));
        let changed = match (old, new) {
            (_, Some((_, Leaf::Unknown))) => true,
            (old, new) => old.and_then(|(_, leaf)| leaf.raw()) != new.and_then(|(_, leaf)| leaf.raw()),
        };
        if !changed {
            continue;
        }
        if rows.len() == limits.attrs {
            more += 1;
            continue;
        }
        let steps = old.or(new).map(|(steps, _)| steps.as_slice()).unwrap_or_default();
        rows.push(Attr {
            path: path.clone(),
            before: old.map(|(_, leaf)| leaf.shown(limits)),
            after: new.map(|(_, leaf)| leaf.shown(limits)),
            forces: forced.iter().any(|prefix| steps.starts_with(prefix)),
        });
    }
    (rows, more)
}

/// The plan JSON of `show -json`, cut down. None when it is not one.
pub fn reduce(json: &Value, limits: &PlanLimits) -> Option<Reduced> {
    json.get("format_version")?.as_str()?;
    let listed = match json.get("resource_changes") {
        None | Some(Value::Null) => &Vec::new(),
        Some(Value::Array(listed)) => listed,
        Some(_) => return None,
    };
    let text = |value: &Value, key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
    let mut counts = Counts::default();
    let mut moved = 0;
    let mut changes = Vec::new();
    let mut destroys = Vec::new();
    // (module, type, name) → (the strongest mark, the instances).
    let mut resources: BTreeMap<(String, String, String), (Option<Mark>, usize)> = BTreeMap::new();
    for resource in listed.iter().filter(|resource| resource.get("mode").and_then(Value::as_str) == Some("managed")) {
        let Some(change) = resource.get("change") else { continue };
        let words: Vec<&str> = change.get("actions").and_then(Value::as_array).map(|actions| actions.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        let importing = change.get("importing").is_some_and(|importing| !importing.is_null());
        if resource.get("previous_address").is_some_and(|previous| !previous.is_null()) {
            moved += 1;
        }
        let (address, module, kind, name) = (text(resource, "address"), text(resource, "module_address"), text(resource, "type"), text(resource, "name"));
        let seen = resources.entry((module.clone(), kind.clone(), name.clone())).or_default();
        seen.1 += 1;
        let action = match words.as_slice() {
            ["create"] | ["create", "forget"] => Action::Create,
            ["update"] => Action::Update,
            ["delete"] => Action::Delete,
            ["delete", "create"] | ["create", "delete"] => Action::Replace,
            ["forget"] => Action::Forget,
            ["no-op"] if importing => Action::Import,
            ["no-op"] | ["read"] => continue,
            // An action this code does not know: a list without it would hide
            // what an apply does, so the plan is not shown at all.
            _ => return None,
        };
        // A new object made while the old one is only forgotten.
        if words.as_slice() == ["create", "forget"] {
            counts.forget += 1;
        }
        seen.0 = seen.0.max(action.mark());
        match action {
            Action::Create => counts.add += 1,
            Action::Update => counts.change += 1,
            Action::Delete => counts.destroy += 1,
            Action::Replace => (counts.add, counts.destroy) = (counts.add + 1, counts.destroy + 1),
            Action::Forget => counts.forget += 1,
            Action::Import => {}
        }
        if importing {
            counts.import += 1;
        }
        if matches!(action, Action::Delete | Action::Replace) {
            destroys.push(address.clone());
        }
        let (attrs, more_attrs) = attrs(action, change, limits);
        changes.push(Change { address, module, kind, name, action, attrs, more_attrs });
    }
    changes.sort_by(|a, b| a.address.cmp(&b.address));
    let more_changes = changes.len().saturating_sub(limits.changes);
    changes.truncate(limits.changes);
    destroys.sort();
    destroys.truncate(limits.changes);
    let outputs = json.get("output_changes").and_then(Value::as_object).map_or(0, |outputs| {
        outputs.values().filter(|output| output.get("actions").and_then(Value::as_array).is_some_and(|actions| actions.as_slice() != [Value::from("no-op")])).count()
    });
    let applyable = json.get("applyable").and_then(Value::as_bool).unwrap_or(counts.total() > 0 || moved > 0 || outputs > 0);
    let marks = resources
        .into_iter()
        .filter_map(|((module, kind, name), (mark, count))| Some(PlanMark { module, kind, name, mark: mark?, count }))
        .collect();
    Some(Reduced { counts, applyable, moved, outputs, changes, more_changes, destroys, marks })
}

/// Whether a file of this name can change what a plan does.
fn counts_for_a_plan(name: &str) -> bool {
    [".tf", ".tofu", ".tfvars", ".tfvars.json"].iter().any(|suffix| name.ends_with(suffix)) || name == ".terraform.lock.hcl"
}

/// A digest of the code a plan was made from: the name, size, and time of
/// each file that counts, directly in `dirs` (the project's folder and its
/// local modules), with the workspace and the tool. A different value means
/// the plan is stale.
pub fn fingerprint(dirs: &[PathBuf], workspace: &str, tool: Tool) -> String {
    let mut lines = Vec::new();
    for (index, dir) in dirs.iter().enumerate() {
        let Ok(entries) = fs::read_dir(dir) else { continue };
        for entry in entries.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(meta) = entry.metadata().ok().filter(|meta| meta.is_file() && counts_for_a_plan(&name)) else { continue };
            let modified = meta.modified().ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |since| since.as_nanos());
            lines.push(format!("{index}/{name}\t{}\t{modified}\n", meta.len()));
        }
    }
    lines.sort();
    let mut hasher = Sha256::new();
    for line in &lines {
        hasher.update(line.as_bytes());
    }
    hasher.update(format!("workspace\t{workspace}\ntool\t{}\n", tool.program()).as_bytes());
    hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::fs;

    /// A managed resource change of the plan JSON.
    fn rc(address: &str, actions: &[&str], extra: Value) -> Value {
        let (module, rest) = match address.rfind(".module.").map(|at| at + 1).or_else(|| address.starts_with("module.").then_some(0)) {
            Some(_) => {
                // module.a.module.b.type.name[0] → the module is everything before the type.
                let parts: Vec<&str> = address.split('.').collect();
                let cut = parts.len() - 2;
                (Some(parts[..cut].join(".")), parts[cut..].join("."))
            }
            None => (None, address.to_owned()),
        };
        let (kind, name) = rest.split_once('.').unwrap();
        let name = name.split('[').next().unwrap();
        let mut change = json!({ "actions": actions, "before": null, "after": null });
        let mut top = json!({ "address": address, "mode": "managed", "type": kind, "name": name });
        if let Some(module) = module {
            top["module_address"] = json!(module);
        }
        for (key, value) in extra.as_object().cloned().unwrap_or_default() {
            match key.as_str() {
                "previous_address" | "mode" => top[key] = value,
                _ => change[key] = value,
            }
        }
        top["change"] = change;
        top
    }

    fn plan(changes: Vec<Value>) -> Value {
        json!({ "format_version": "1.2", "resource_changes": changes })
    }

    fn reduced(changes: Vec<Value>) -> Reduced {
        reduce(&plan(changes), &PlanLimits::DEFAULT).expect("a plan")
    }

    fn attrs(change: &Change) -> Value {
        serde_json::to_value(&change.attrs).unwrap()
    }

    fn value(text: &str) -> Value {
        json!({ "kind": "value", "text": text, "cut": false })
    }

    #[test]
    fn the_six_actions_and_their_counts() {
        let reduced = reduced(vec![
            rc("a.create", &["create"], json!({})),
            rc("a.update", &["update"], json!({})),
            rc("a.delete", &["delete"], json!({})),
            rc("a.replace", &["delete", "create"], json!({})),
            rc("a.replace_cbd", &["create", "delete"], json!({})),
            rc("a.forget", &["forget"], json!({})),
            rc("a.imported", &["no-op"], json!({ "importing": { "id": "i-1" } })),
            rc("a.imported_update", &["update"], json!({ "importing": { "id": "i-2" } })),
            rc("a.noop", &["no-op"], json!({})),
            rc("a.read", &["read"], json!({})),
            rc("a.data", &["create"], json!({ "mode": "data" })),
        ]);
        assert_eq!(reduced.counts, Counts { add: 3, change: 2, destroy: 3, import: 2, forget: 1 });
        let actions: Vec<(String, Value)> = reduced.changes.iter().map(|change| (change.address.clone(), serde_json::to_value(change.action).unwrap())).collect();
        assert_eq!(
            actions,
            [
                ("a.create".to_owned(), json!("create")),
                ("a.delete".to_owned(), json!("delete")),
                ("a.forget".to_owned(), json!("forget")),
                ("a.imported".to_owned(), json!("import")),
                ("a.imported_update".to_owned(), json!("update")),
                ("a.replace".to_owned(), json!("replace")),
                ("a.replace_cbd".to_owned(), json!("replace")),
                ("a.update".to_owned(), json!("update")),
            ]
        );
        assert_eq!(reduced.destroys, ["a.delete", "a.replace", "a.replace_cbd"]);
        assert!(reduced.applyable);
        assert_eq!((reduced.moved, reduced.outputs, reduced.more_changes), (0, 0, 0));
    }

    #[test]
    fn only_changed_paths_are_kept() {
        let reduced = reduced(vec![rc(
            "module.vpc.aws_nat_gateway.this",
            &["update"],
            json!({
                "before": { "tags": { "Env": "dev", "Team": "a" }, "cidr": "10.0.0.0/16", "ports": [80, 443], "on": true },
                "after": { "tags": { "Env": "development", "Team": "a", "Owner": "platform" }, "cidr": "10.0.0.0/16", "ports": [80, 8443], "on": true },
            }),
        )]);
        let change = &reduced.changes[0];
        assert_eq!((change.module.as_str(), change.kind.as_str(), change.name.as_str()), ("module.vpc", "aws_nat_gateway", "this"));
        assert_eq!(
            attrs(change),
            json!([
                { "path": "ports[1]", "before": value("443"), "after": value("8443"), "forces": false },
                { "path": "tags.Env", "before": value("\"dev\""), "after": value("\"development\""), "forces": false },
                { "path": "tags.Owner", "before": null, "after": value("\"platform\""), "forces": false },
            ])
        );
        assert_eq!(change.more_attrs, 0);
    }

    #[test]
    fn a_create_lists_the_after_values() {
        let reduced = reduced(vec![rc("aws_route53_record.app", &["create"], json!({ "after": { "name": "app.dev.acme.io", "ttl": 300, "alias": [], "meta": {}, "note": null } }))]);
        assert_eq!(
            attrs(&reduced.changes[0]),
            json!([
                { "path": "alias", "before": null, "after": value("[]"), "forces": false },
                { "path": "meta", "before": null, "after": value("{}"), "forces": false },
                { "path": "name", "before": null, "after": value("\"app.dev.acme.io\""), "forces": false },
                { "path": "ttl", "before": null, "after": value("300"), "forces": false },
            ])
        );
        assert_eq!(reduced.changes[0].module, "");
    }

    #[test]
    fn a_delete_lists_no_attributes() {
        let reduced = reduced(vec![
            rc("a.gone", &["delete"], json!({ "before": { "name": "x" } })),
            rc("a.forgotten", &["forget"], json!({ "before": { "name": "x" } })),
            rc("a.imported", &["no-op"], json!({ "importing": { "id": "i" }, "before": { "name": "x" }, "after": { "name": "x" } })),
        ]);
        assert!(reduced.changes.iter().all(|change| change.attrs.is_empty()));
    }

    #[test]
    fn sensitive_values_never_leave() {
        let reduced = reduced(vec![
            rc(
                "aws_db_instance.main",
                &["update"],
                json!({
                    "before": { "password": "old-hunter2", "tags": { "secret": "old-s3cr3t" }, "size": 1 },
                    "after": { "password": "hunter2", "tags": { "secret": "s3cr3t" }, "size": 2 },
                    "before_sensitive": { "password": true, "tags": true },
                    "after_sensitive": { "password": true, "tags": true },
                }),
            ),
            rc("aws_ssm_parameter.new", &["create"], json!({ "after": { "value": "t0ken", "name": "n" }, "after_sensitive": { "value": true } })),
        ]);
        assert_eq!(
            attrs(&reduced.changes[0]),
            json!([
                { "path": "password", "before": { "kind": "sensitive" }, "after": { "kind": "sensitive" }, "forces": false },
                { "path": "size", "before": value("1"), "after": value("2"), "forces": false },
                { "path": "tags", "before": { "kind": "sensitive" }, "after": { "kind": "sensitive" }, "forces": false },
            ])
        );
        assert_eq!(attrs(&reduced.changes[1])[1], json!({ "path": "value", "before": null, "after": { "kind": "sensitive" }, "forces": false }));
        let text = serde_json::to_string(&reduced).unwrap();
        for secret in ["hunter2", "s3cr3t", "t0ken", "secret\""] {
            assert!(!text.contains(secret), "{secret} left: {text}");
        }
    }

    #[test]
    fn a_value_marked_sensitive_on_one_side_is_hidden_on_both() {
        // A variable becomes sensitive (or stops being so) in the same change.
        let reduced = reduced(vec![
            rc("a.becomes", &["update"], json!({ "before": { "password": "old-hunter2", "n": 1 }, "after": { "password": "new-hunter2", "n": 2 }, "before_sensitive": {}, "after_sensitive": { "password": true } })),
            rc("a.was", &["update"], json!({ "before": { "token": { "id": "old-t0ken" } }, "after": { "token": { "id": "new-t0ken" } }, "before_sensitive": { "token": true }, "after_sensitive": false })),
        ]);
        assert_eq!(attrs(&reduced.changes[0])[1], json!({ "path": "password", "before": { "kind": "sensitive" }, "after": { "kind": "sensitive" }, "forces": false }));
        assert_eq!(attrs(&reduced.changes[1]), json!([{ "path": "token", "before": { "kind": "sensitive" }, "after": { "kind": "sensitive" }, "forces": false }]));
        let text = serde_json::to_string(&reduced).unwrap();
        for secret in ["hunter2", "t0ken"] {
            assert!(!text.contains(secret), "{secret} left: {text}");
        }
    }

    #[test]
    fn a_create_that_forgets_the_old_object_counts_as_both() {
        let reduced = reduced(vec![rc("a.b", &["create", "forget"], json!({ "after": { "n": 1 } }))]);
        assert_eq!(reduced.counts, Counts { add: 1, forget: 1, ..Counts::default() });
        assert_eq!(serde_json::to_value(reduced.changes[0].action).unwrap(), json!("create"));
    }

    #[test]
    fn a_plan_with_an_action_it_does_not_know_is_not_shown_at_all() {
        // Showing the rest would hide a change that an apply makes.
        for actions in [vec!["explode"], vec!["update", "delete"], vec![]] {
            let odd = plan(vec![rc("a.ok", &["create"], json!({})), rc("a.odd", &actions, json!({}))]);
            assert!(reduce(&odd, &PlanLimits::DEFAULT).is_none(), "{actions:?}");
        }
        assert!(reduce(&plan(vec![rc("a.read", &["read"], json!({})), rc("a.same", &["no-op"], json!({}))]), &PlanLimits::DEFAULT).is_some());
    }

    #[test]
    fn an_unchanged_sensitive_value_is_not_listed() {
        let reduced = reduced(vec![rc(
            "a.b",
            &["update"],
            json!({ "before": { "password": "same", "n": 1 }, "after": { "password": "same", "n": 2 }, "before_sensitive": { "password": true }, "after_sensitive": { "password": true } }),
        )]);
        assert_eq!(attrs(&reduced.changes[0]).as_array().unwrap().len(), 1);
    }

    #[test]
    fn unknown_values_are_marked() {
        let reduced = reduced(vec![
            rc("a.new", &["create"], json!({ "after": { "name": "x" }, "after_unknown": { "zone_id": true, "tags_all": true, "name": false, "rule": [{ "id": true }] } })),
            rc("a.old", &["update"], json!({ "before": { "arn": "arn:1", "n": 1 }, "after": { "n": 1 }, "after_unknown": { "arn": true } })),
        ]);
        assert_eq!(
            attrs(&reduced.changes[0]),
            json!([
                { "path": "name", "before": null, "after": value("\"x\""), "forces": false },
                { "path": "rule[0].id", "before": null, "after": { "kind": "unknown" }, "forces": false },
                { "path": "tags_all", "before": null, "after": { "kind": "unknown" }, "forces": false },
                { "path": "zone_id", "before": null, "after": { "kind": "unknown" }, "forces": false },
            ])
        );
        assert_eq!(attrs(&reduced.changes[1]), json!([{ "path": "arn", "before": value("\"arn:1\""), "after": { "kind": "unknown" }, "forces": false }]));
    }

    #[test]
    fn values_below_an_unknown_value_change_to_unknown_not_to_nothing() {
        // What the tool prints as `output = { name = "a", ttl = 300 } -> (known after apply)`.
        let reduced = reduced(vec![rc(
            "terraform_data.dns",
            &["update"],
            json!({
                "before": { "input": { "ttl": 300 }, "output": { "name": "a", "ttl": 300 }, "id": "x" },
                "after": { "input": { "ttl": 600 }, "id": "x" },
                "after_unknown": { "output": true },
            }),
        )]);
        assert_eq!(
            attrs(&reduced.changes[0]),
            json!([
                { "path": "input.ttl", "before": value("300"), "after": value("600"), "forces": false },
                { "path": "output.name", "before": value("\"a\""), "after": { "kind": "unknown" }, "forces": false },
                { "path": "output.ttl", "before": value("300"), "after": { "kind": "unknown" }, "forces": false },
            ])
        );
    }

    #[test]
    fn replace_paths_force() {
        let reduced = reduced(vec![rc(
            "aws_instance.web",
            &["delete", "create"],
            json!({
                "before": { "ami": "ami-1", "tags": { "a": "1" }, "disk": [{ "size": 8 }] },
                "after": { "ami": "ami-2", "tags": { "a": "2" }, "disk": [{ "size": 16 }] },
                "replace_paths": [["ami"], ["disk", 0]],
            }),
        )]);
        let forced: Vec<(String, bool)> = reduced.changes[0].attrs.iter().map(|attr| (attr.path.clone(), attr.forces)).collect();
        assert_eq!(forced, [("ami".to_owned(), true), ("disk[0].size".to_owned(), true), ("tags.a".to_owned(), false)]);
    }

    #[test]
    fn long_values_are_cut_on_a_character() {
        let long = "가".repeat(5000 / 3);
        let reduced = reduced(vec![rc("a.b", &["create"], json!({ "after": { "user_data": long } }))]);
        let Some(AttrValue::Value { text, cut }) = &reduced.changes[0].attrs[0].after else { panic!("a value") };
        assert!(*cut);
        assert!(text.len() <= 2048 && text.len() > 2040, "{}", text.len());
        assert!(text.starts_with("\"가"));
        assert!(text.chars().all(|c| c == '"' || c == '가'));
    }

    #[test]
    fn odd_keys_are_quoted_in_the_path() {
        let reduced = reduced(vec![rc("a.b", &["create"], json!({ "after": { "tags": { "kubernetes.io/role": "x", "plain_1": "y" } } }))]);
        let paths: Vec<&str> = reduced.changes[0].attrs.iter().map(|attr| attr.path.as_str()).collect();
        assert_eq!(paths, ["tags.plain_1", "tags[\"kubernetes.io/role\"]"]);
    }

    #[test]
    fn attribute_and_change_limits_keep_the_counts() {
        let after: serde_json::Map<String, Value> = (0..250).map(|i| (format!("k{i:03}"), json!(i))).collect();
        let reduced = reduced(vec![rc("a.b", &["create"], json!({ "after": after }))]);
        assert_eq!((reduced.changes[0].attrs.len(), reduced.changes[0].more_attrs), (200, 50));
        assert_eq!(reduced.changes[0].attrs[0].path, "k000");

        let many: Vec<Value> = (0..2100).map(|i| rc(&format!("a.r{i:04}"), &["create"], json!({}))).collect();
        let reduced = reduce(&plan(many), &PlanLimits::DEFAULT).unwrap();
        assert_eq!((reduced.changes.len(), reduced.more_changes, reduced.counts.add), (2000, 100, 2100));
        assert_eq!(reduced.changes[0].address, "a.r0000");
        assert_eq!((PlanLimits::DEFAULT.value_bytes, PlanLimits::DEFAULT.attrs, PlanLimits::DEFAULT.changes), (2048, 200, 2000));

        let gone: Vec<Value> = (0..2100).map(|i| rc(&format!("a.r{i:04}"), &["delete"], json!({}))).collect();
        assert_eq!(reduce(&plan(gone), &PlanLimits::DEFAULT).unwrap().destroys.len(), 2000);
    }

    #[test]
    fn a_plan_of_only_moves_or_outputs_can_be_applied() {
        let moved = reduced(vec![rc("a.new_name", &["no-op"], json!({ "previous_address": "a.old_name" }))]);
        assert!(moved.applyable);
        assert_eq!((moved.moved, moved.changes.len()), (1, 0));

        let mut outputs = plan(vec![]);
        outputs["output_changes"] = json!({ "url": { "actions": ["update"] }, "same": { "actions": ["no-op"] } });
        let outputs = reduce(&outputs, &PlanLimits::DEFAULT).unwrap();
        assert!(outputs.applyable);
        assert_eq!(outputs.outputs, 1);

        assert!(!reduced(vec![]).applyable);
        assert!(!reduced(vec![rc("a.b", &["no-op"], json!({}))]).applyable);
        let mut refused = plan(vec![rc("a.b", &["create"], json!({}))]);
        refused["applyable"] = json!(false);
        assert!(!reduce(&refused, &PlanLimits::DEFAULT).unwrap().applyable);
        let mut allowed = plan(vec![]);
        allowed["applyable"] = json!(true);
        assert!(reduce(&allowed, &PlanLimits::DEFAULT).unwrap().applyable);
    }

    #[test]
    fn marks_take_the_strongest_instance() {
        let reduced = reduced(vec![
            rc("module.vpc.aws_subnet.private[0]", &["create"], json!({})),
            rc("module.vpc.aws_subnet.private[1]", &["delete"], json!({})),
            rc("module.vpc.aws_subnet.private[2]", &["no-op"], json!({})),
            rc("aws_route53_record.app", &["update"], json!({})),
            rc("aws_vpc.kept", &["no-op"], json!({})),
            rc("aws_vpc.imported", &["no-op"], json!({ "importing": { "id": "x" } })),
            rc("module.a.module.b.aws_eip.x[\"k\"]", &["create", "delete"], json!({})),
        ]);
        assert_eq!(
            serde_json::to_value(&reduced.marks).unwrap(),
            json!([
                { "module": "", "type": "aws_route53_record", "name": "app", "mark": "update", "count": 1 },
                { "module": "module.a.module.b", "type": "aws_eip", "name": "x", "mark": "replace", "count": 1 },
                { "module": "module.vpc", "type": "aws_subnet", "name": "private", "mark": "delete", "count": 3 },
            ])
        );
    }

    #[test]
    fn not_a_plan_reads_as_none() {
        for value in [json!({}), json!([]), json!("x"), json!({ "format_version": "1.2", "resource_changes": 3 }), json!({ "resource_changes": [] })] {
            assert!(reduce(&value, &PlanLimits::DEFAULT).is_none(), "{value}");
        }
        // A plan of an empty configuration has no list of changes.
        let empty = reduce(&json!({ "format_version": "1.2" }), &PlanLimits::DEFAULT).unwrap();
        assert!(empty.changes.is_empty() && !empty.applyable);
    }

    #[test]
    fn the_reduced_plan_serializes_as_the_wire_shape() {
        let reduced = reduced(vec![rc("a.b", &["create"], json!({ "after": { "n": 1 } }))]);
        assert_eq!(
            serde_json::to_value(&reduced).unwrap(),
            json!({
                "counts": { "add": 1, "change": 0, "destroy": 0, "import": 0, "forget": 0 },
                "applyable": true,
                "moved": 0,
                "outputs": 0,
                "changes": [{ "address": "a.b", "module": "", "type": "a", "name": "b", "action": "create", "attrs": [{ "path": "n", "before": null, "after": value("1"), "forces": false }], "moreAttrs": 0 }],
                "moreChanges": 0,
                "destroys": [],
                "marks": [{ "module": "", "type": "a", "name": "b", "mark": "create", "count": 1 }],
            })
        );
    }

    #[test]
    fn the_fingerprint_changes_with_files_the_workspace_and_the_tool() {
        let dir = tempfile::tempdir().unwrap();
        let (project, module) = (dir.path().join("p"), dir.path().join("m"));
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&module).unwrap();
        fs::write(project.join("main.tf"), "a").unwrap();
        fs::write(project.join("x.tfvars"), "b").unwrap();
        fs::write(module.join("main.tf"), "c").unwrap();
        let dirs = [project.clone(), module.clone()];
        let print = |workspace: &str, tool: Tool| fingerprint(&dirs, workspace, tool);
        let first = print("dev", Tool::Terraform);
        assert_eq!(first.len(), 64);
        assert_eq!(first, print("dev", Tool::Terraform));

        fs::write(project.join("README.md"), "docs").unwrap();
        fs::create_dir_all(project.join("sub")).unwrap();
        fs::write(project.join("sub/main.tf"), "below").unwrap();
        assert_eq!(first, print("dev", Tool::Terraform), "other files and folders below do not count");

        assert_ne!(first, print("prod", Tool::Terraform));
        assert_ne!(first, print("dev", Tool::Tofu));

        fs::write(project.join("main.tf"), "ab").unwrap();
        let grown = print("dev", Tool::Terraform);
        assert_ne!(first, grown);

        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        fs::File::options().write(true).open(project.join("x.tfvars")).unwrap().set_modified(later).unwrap();
        let touched = print("dev", Tool::Terraform);
        assert_ne!(grown, touched, "the same size at a new time");

        fs::write(module.join("main.tf"), "cc").unwrap();
        let module_changed = print("dev", Tool::Terraform);
        assert_ne!(touched, module_changed);

        for name in ["new.tofu", "y.tfvars.json", ".terraform.lock.hcl"] {
            let before = print("dev", Tool::Terraform);
            fs::write(project.join(name), "").unwrap();
            assert_ne!(before, print("dev", Tool::Terraform), "{name}");
        }
        let before = print("dev", Tool::Terraform);
        fs::remove_file(project.join("new.tofu")).unwrap();
        assert_ne!(before, print("dev", Tool::Terraform), "a removed file");
    }
}
