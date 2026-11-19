//! Newer module versions: what the public registry lists for the registry
//! modules a project uses. Only the module's namespace, name, and provider
//! are sent, at most once a day for each module, and only while the user
//! leaves the check on.

use std::collections::HashMap;
use std::future::Future;

use futures_util::StreamExt;

use super::tree::{Node, RegistryUse};
#[cfg(test)]
use super::tree::SourceKind;
use crate::plugins::terraform::Tool;

/// A module is asked about again after this long.
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
/// Requests at once.
const AT_ONCE: usize = 4;
/// How long a request may take, in seconds.
const TIMEOUT_SECS: u32 = 5;
/// An answer is at most this many bytes.
const MAX_BYTES: u64 = 1024 * 1024;

/// The public registry of the tool.
pub fn host(tool: Tool) -> &'static str {
    match tool {
        Tool::Terraform => "registry.terraform.io",
        Tool::Tofu => "registry.opentofu.org",
    }
}

/// The address that lists a module's versions. None unless each part is a
/// plain word, so nothing but the three names reaches the URL.
pub fn versions_url(tool: Tool, namespace: &str, name: &str, provider: &str) -> Option<String> {
    let plain = |part: &str| (1..=64).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    (plain(namespace) && plain(name) && plain(provider)).then(|| format!("https://{}/v1/modules/{namespace}/{name}/{provider}/versions", host(tool)))
}

/// The three numbers of a release version; None for a pre-release or any other shape.
fn release(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.').map(|part| part.parse::<u64>().ok());
    match (parts.next()??, parts.next()??, parts.next()??, parts.next()) {
        (major, minor, patch, None) => Some((major, minor, patch)),
        _ => None,
    }
}

/// The highest release version in a registry answer (`modules[0].versions[].version`).
pub fn newest(body: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let versions = value.get("modules")?.get(0)?.get("versions")?.as_array()?;
    versions.iter().filter_map(|version| version.get("version")?.as_str()).filter_map(|text| Some((release(text)?, text))).max().map(|(_, text)| text.to_owned())
}

/// Whether `candidate` is a later release than the installed `resolved`.
pub fn is_newer(candidate: &str, resolved: &str) -> bool {
    matches!((release(candidate), release(resolved)), (Some(candidate), Some(resolved)) if candidate > resolved)
}

/// The words after `curl` for a request: https only (also across
/// redirects), a short time, a small answer.
fn curl_args(url: &str) -> Vec<String> {
    // `--disable` must be first: the user's own curl settings (a proxy, headers, a login) do not apply.
    let mut args: Vec<String> = ["--disable", "--fail", "--silent", "--show-error", "--location", "--proto", "=https", "--proto-redir", "=https"].map(str::to_owned).to_vec();
    args.extend(["--max-time".to_owned(), TIMEOUT_SECS.to_string(), "--max-filesize".to_owned(), MAX_BYTES.to_string()]);
    args.extend(["--user-agent".to_owned(), format!("westerm/{}", crate::update::CURRENT), url.to_owned()]);
    args
}

/// Asks the registry; None when the request fails or the answer is too large.
#[cfg(not(test))]
pub async fn fetch(url: String) -> Option<Vec<u8>> {
    let mut command = tokio::process::Command::new("curl");
    command.args(curl_args(&url)).kill_on_drop(true).stdin(std::process::Stdio::null());
    let limit = std::time::Duration::from_secs(u64::from(TIMEOUT_SECS) + 1);
    let output = tokio::time::timeout(limit, command.output()).await.ok()?.ok()?;
    (output.status.success() && output.stdout.len() as u64 <= MAX_BYTES).then_some(output.stdout)
}

/// In tests nothing leaves the machine: every request is counted and
/// answered with one version, 9.9.9.
#[cfg(test)]
pub use test_fetch::fetch;

#[cfg(test)]
pub(crate) mod test_fetch {
    use std::sync::Mutex;

    static ASKED: Mutex<Vec<String>> = Mutex::new(Vec::new());

    pub async fn fetch(url: String) -> Option<Vec<u8>> {
        ASKED.lock().unwrap().push(url);
        Some(super::tests::versions(&["9.9.9"]))
    }

    /// How often `url` was asked for. The route tests that count are Unix only.
    #[cfg_attr(not(unix), allow(dead_code))]
    pub(crate) fn asked(url: &str) -> usize {
        ASKED.lock().unwrap().iter().filter(|asked| *asked == url).count()
    }
}

/// A module of a registry: the tool, the namespace, the name, the provider.
pub type Key = (Tool, String, String, String);

/// What the registries said: the newest release of each module, and when it was asked.
#[derive(Default)]
pub struct Cache {
    asked: tokio::sync::Mutex<HashMap<Key, (Option<String>, u64)>>,
}

impl Cache {
    /// The newer versions already known, by module address, and the modules
    /// to ask about now. Those are marked as asked, so no one else asks too.
    pub async fn known(&self, tool: Tool, uses: &[RegistryUse], now: u64) -> (HashMap<String, String>, Vec<Key>) {
        let mut asked = self.asked.lock().await;
        let mut newer = HashMap::new();
        let mut ask = Vec::new();
        for used in uses {
            let Some(resolved) = &used.resolved else { continue };
            let key: Key = (tool, used.namespace.clone(), used.name.clone(), used.provider.clone());
            let fresh = asked.get(&key).filter(|(_, at)| now.saturating_sub(*at) < DAY_MS);
            match fresh {
                Some((newest, _)) => {
                    if let Some(newest) = newest.as_ref().filter(|newest| is_newer(newest, resolved)) {
                        newer.insert(used.address.clone(), newest.clone());
                    }
                }
                None => {
                    let before = asked.get(&key).and_then(|(newest, _)| newest.clone());
                    asked.insert(key.clone(), (before, now));
                    ask.push(key);
                }
            }
        }
        (newer, ask)
    }

    /// Asks the registries about `keys`, a few at once, and remembers the
    /// answers. A failed request is remembered as "nothing newer".
    pub async fn refresh<F, Fut>(&self, keys: Vec<Key>, now: u64, fetch: F)
    where
        F: Fn(String) -> Fut,
        Fut: Future<Output = Option<Vec<u8>>>,
    {
        let fetch = &fetch;
        futures_util::stream::iter(keys)
            .for_each_concurrent(AT_ONCE, |key| async move {
                let found = match versions_url(key.0, &key.1, &key.2, &key.3) {
                    Some(url) => fetch(url).await.and_then(|body| newest(&body)),
                    None => None,
                };
                self.asked.lock().await.insert(key, (found, now));
            })
            .await;
    }

    /// `known`, after asking about every module that is due.
    #[cfg(test)]
    pub async fn newer_for<F, Fut>(&self, tool: Tool, uses: &[RegistryUse], now: u64, fetch: F) -> HashMap<String, String>
    where
        F: Fn(String) -> Fut,
        Fut: Future<Output = Option<Vec<u8>>>,
    {
        let (_, ask) = self.known(tool, uses, now).await;
        self.refresh(ask, now, fetch).await;
        self.known(tool, uses, now).await.0
    }
}

/// Writes the newer versions into the module rows, by address.
pub fn fill_newer(nodes: &mut [Node], newer: &HashMap<String, String>) {
    for node in nodes {
        if let Node::Module { address, newer: slot, children, .. } = node {
            *slot = newer.get(address).cloned();
            fill_newer(children, newer);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    const HOUR: u64 = 60 * 60 * 1000;

    fn uses(resolved: Option<&str>) -> Vec<RegistryUse> {
        vec![RegistryUse { address: "module.vpc".into(), namespace: "terraform-aws-modules".into(), name: "vpc".into(), provider: "aws".into(), resolved: resolved.map(str::to_owned) }]
    }

    pub(crate) fn versions(list: &[&str]) -> Vec<u8> {
        let versions: Vec<serde_json::Value> = list.iter().map(|version| serde_json::json!({ "version": version })).collect();
        serde_json::json!({ "modules": [{ "source": "x", "versions": versions }] }).to_string().into_bytes()
    }

    /// A fetch that counts its calls and answers with `body`.
    fn counting(body: Option<Vec<u8>>) -> (Arc<AtomicUsize>, impl Fn(String) -> std::future::Ready<Option<Vec<u8>>> + Clone) {
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        (calls, move |_url: String| {
            counter.fetch_add(1, Ordering::SeqCst);
            std::future::ready(body.clone())
        })
    }

    #[test]
    fn the_newest_release_is_picked() {
        assert_eq!(newest(&versions(&["5.9.0", "5.13.0", "5.16.0", "6.0.0-rc1", "bad", "7.0"])).as_deref(), Some("5.16.0"));
        assert_eq!(newest(&versions(&[])), None);
        assert_eq!(newest(b"{"), None);
        assert_eq!(newest(b"{\"modules\":[]}"), None);
    }

    #[test]
    fn versions_compare_by_number() {
        assert!(is_newer("5.16.0", "5.13.0"));
        assert!(is_newer("10.0.0", "9.9.9"));
        assert!(!is_newer("5.9.0", "5.13.0"));
        assert!(!is_newer("5.13.0", "5.13.0"));
        assert!(!is_newer("5.16.0", "main"), "an installed version that is not three numbers is not compared");
    }

    #[test]
    fn urls_are_built_for_the_tool_and_refuse_odd_parts() {
        assert_eq!(versions_url(Tool::Terraform, "terraform-aws-modules", "vpc", "aws").as_deref(), Some("https://registry.terraform.io/v1/modules/terraform-aws-modules/vpc/aws/versions"));
        assert_eq!(versions_url(Tool::Tofu, "a", "b_1", "c").as_deref(), Some("https://registry.opentofu.org/v1/modules/a/b_1/c/versions"));
        assert_eq!((host(Tool::Terraform), host(Tool::Tofu)), ("registry.terraform.io", "registry.opentofu.org"));
        for (namespace, name, provider) in [("a/b", "vpc", "aws"), ("a", "x?y", "aws"), ("a", "vpc", ""), ("a", "vpc", "../x"), ("a", &"n".repeat(65), "aws")] {
            assert_eq!(versions_url(Tool::Terraform, namespace, name, provider), None, "{namespace} {name} {provider}");
        }
    }

    #[test]
    fn the_request_is_https_only_small_and_short() {
        let args = curl_args("https://registry.terraform.io/v1/modules/a/b/c/versions");
        let has = |flag: &str, value: &str| args.windows(2).any(|pair| pair[0] == flag && pair[1] == value);
        assert!(has("--proto", "=https") && has("--proto-redir", "=https"));
        assert!(has("--max-time", "5") && has("--max-filesize", "1048576"));
        assert!(args.contains(&"--fail".to_owned()));
        // The user's own curl settings (a proxy, headers, a login) do not apply.
        assert_eq!(args.first().map(String::as_str), Some("--disable"));
        assert_eq!(args.last().map(String::as_str), Some("https://registry.terraform.io/v1/modules/a/b/c/versions"));
    }

    #[tokio::test]
    async fn a_newer_version_is_named_for_the_module() {
        let cache = Cache::default();
        let (_, fetch) = counting(Some(versions(&["5.13.0", "5.16.0"])));
        let newer = cache.newer_for(Tool::Terraform, &uses(Some("5.13.0")), 0, fetch.clone()).await;
        assert_eq!(newer.get("module.vpc").map(String::as_str), Some("5.16.0"));
        assert!(cache.newer_for(Tool::Terraform, &uses(Some("5.16.0")), 0, fetch).await.is_empty(), "the newest is installed");
    }

    #[tokio::test]
    async fn a_module_is_asked_once_a_day() {
        let cache = Cache::default();
        let (calls, fetch) = counting(Some(versions(&["5.16.0"])));
        cache.newer_for(Tool::Terraform, &uses(Some("5.13.0")), 0, fetch.clone()).await;
        cache.newer_for(Tool::Terraform, &uses(Some("5.13.0")), 23 * HOUR, fetch.clone()).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let newer = cache.newer_for(Tool::Terraform, &uses(Some("5.13.0")), 25 * HOUR, fetch.clone()).await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(newer.len(), 1);
        // The other tool asks its own registry.
        cache.newer_for(Tool::Tofu, &uses(Some("5.13.0")), 25 * HOUR, fetch).await;
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn the_same_module_used_twice_is_asked_once() {
        let cache = Cache::default();
        let (calls, fetch) = counting(Some(versions(&["5.16.0"])));
        let mut twice = uses(Some("5.13.0"));
        twice.push(RegistryUse { address: "module.other".into(), ..twice[0].clone() });
        let newer = cache.newer_for(Tool::Terraform, &twice, 0, fetch).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(newer.len(), 2);
    }

    #[tokio::test]
    async fn a_module_without_a_resolved_version_is_not_asked() {
        let cache = Cache::default();
        let (calls, fetch) = counting(Some(versions(&["5.16.0"])));
        assert!(cache.newer_for(Tool::Terraform, &uses(None), 0, fetch).await.is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_failed_request_shows_nothing() {
        let cache = Cache::default();
        let (calls, fetch) = counting(None);
        assert!(cache.newer_for(Tool::Terraform, &uses(Some("5.13.0")), 0, fetch.clone()).await.is_empty());
        assert!(cache.newer_for(Tool::Terraform, &uses(Some("5.13.0")), HOUR, fetch).await.is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 1, "a failure is remembered for a day too");
    }

    #[tokio::test]
    async fn what_is_known_is_given_without_waiting() {
        let cache = Cache::default();
        let (newer, ask) = cache.known(Tool::Terraform, &uses(Some("5.13.0")), 0).await;
        assert!(newer.is_empty());
        assert_eq!(ask.len(), 1);
        // A second reader does not ask for the same module while the first one does.
        assert!(cache.known(Tool::Terraform, &uses(Some("5.13.0")), 1).await.1.is_empty());
        let (_, fetch) = counting(Some(versions(&["5.16.0"])));
        cache.refresh(ask, 2, fetch).await;
        assert_eq!(cache.known(Tool::Terraform, &uses(Some("5.13.0")), 3).await.0.len(), 1);
    }

    #[test]
    fn newer_versions_are_written_into_the_tree() {
        let module = |address: &str, children: Vec<Node>| Node::Module { name: "m".into(), address: address.into(), source: SourceKind::Registry, label: None, newer: None, dir: None, children };
        let mut nodes = vec![module("module.a", vec![module("module.a.module.b", vec![])]), Node::Resource { address: "x.y".into(), kind: "x".into(), name: "y".into() }];
        let newer = HashMap::from([("module.a.module.b".to_owned(), "2.0.0".to_owned())]);
        fill_newer(&mut nodes, &newer);
        let Node::Module { newer: outer, children, .. } = &nodes[0] else { panic!() };
        let Node::Module { newer: inner, .. } = &children[0] else { panic!() };
        assert_eq!((outer.as_deref(), inner.as_deref()), (None, Some("2.0.0")));
    }
}
