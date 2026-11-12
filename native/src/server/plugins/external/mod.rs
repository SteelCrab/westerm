//! Plugins installed from outside: a folder with a `plugin.json` and a panel.

mod assets;
mod browser;
pub(super) mod catalog;
pub(super) mod manifest;
pub(super) mod package;
mod routes;
mod run;
mod stage;
pub(super) mod store;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use axum::Router;
use tokio::sync::Semaphore;

use crate::server::Shared;

/// Where plugins come from. Tests point these at `file://` folders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sources {
    /// The verified plugin list.
    pub index_url: String,
    /// The GitHub REST API, for the latest release of a repository.
    pub github_api: String,
    /// Allow `file://` downloads besides `https://` (tests only).
    pub allow_file: bool,
}

impl Default for Sources {
    fn default() -> Self {
        Self {
            index_url: "https://raw.githubusercontent.com/SteelCrab/westerm-plugins/main/index.json".into(),
            github_api: "https://api.github.com".into(),
            allow_file: false,
        }
    }
}

/// What the install, update, and catalog routes keep in memory.
#[derive(Default)]
pub(super) struct ExternalState {
    /// Where packages and the plugin list come from.
    sources: Sources,
    /// Packages that were fetched and wait for the user to confirm.
    pending: stage::Pending,
    /// The plugin list and the newest versions found.
    catalog: catalog::Catalog,
    /// One install, update, removal, or storage write changes the disk at a time.
    write_lock: Mutex<()>,
    /// The `run` calls in flight, per plugin.
    runs: Mutex<HashMap<String, Arc<Semaphore>>>,
}

pub(super) fn routes() -> Router<Shared> {
    routes::routes().merge(assets::routes())
}
