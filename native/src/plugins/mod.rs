//! Plugins: what a plugin asks a new shell to start. The session query names
//! a plugin, an action, and a target; each plugin checks them and builds the
//! command itself, so the browser never sends one.

#[cfg(feature = "plugin-db")]
pub mod db;
pub mod docker;
pub mod external;
#[cfg(feature = "plugin-terraform")]
pub mod terraform;
#[cfg(feature = "plugin-terraform")]
pub mod terraform_runner;

use crate::quote::ShellKind;

/// The plugins this build knows, in plugin bar order. Git has no launches:
/// it opens no shells.
pub const PLUGIN_IDS: &[&str] = &[
    "git",
    "docker",
    #[cfg(feature = "plugin-db")]
    db::ID,
    #[cfg(feature = "plugin-terraform")]
    terraform::ID,
];

/// The plugins that are on until the user turns them off. Git was a fixed
/// panel before it was a plugin, so it stays where it was after an update.
pub const DEFAULT_ON: &[&str] = &["git"];

/// Whether `text` is `len` lowercase hex digits: the shape of the ids and tokens the server makes.
#[cfg_attr(not(any(feature = "plugin-db", feature = "plugin-terraform")), allow(dead_code))]
pub fn is_lower_hex(text: &str, len: usize) -> bool {
    text.len() == len && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A checked plugin launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PluginLaunch {
    Docker(docker::Launch),
    /// A Databases console the server prepared.
    #[cfg(feature = "plugin-db")]
    Db(db::Launch),
    /// A Terraform run the server prepared.
    #[cfg(feature = "plugin-terraform")]
    Terraform(terraform::Launch),
    /// An installed plugin's program and arguments.
    External(external::Launch),
}

/// Why a launch cannot be typed into the new shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchError {
    /// Its words cannot be quoted for this shell (not sh-like or fish).
    UnsupportedShell,
    /// A word holds a control character. `parse_argv` refuses these first.
    ControlCharacter,
    /// What the launch would type is gone (a console spec read before, or never written).
    #[cfg_attr(not(any(feature = "plugin-db", feature = "plugin-terraform")), allow(dead_code))]
    Expired,
}

impl PluginLaunch {
    /// None for an unknown plugin, an unknown action, or a target the plugin refuses.
    pub fn parse(plugin: &str, action: &str, target: &str) -> Option<Self> {
        match plugin {
            "docker" => {
                let action = docker::Action::parse(action)?;
                docker::is_container_id(target)
                    .then(|| Self::Docker(docker::Launch { action, container: target.to_owned() }))
            }
            #[cfg(feature = "plugin-db")]
            db::ID => (action == db::CONSOLE_ACTION && db::is_token(target))
                .then(|| Self::Db(db::Launch { token: target.to_owned() })),
            #[cfg(feature = "plugin-terraform")]
            terraform::ID => (action == terraform::RUN_ACTION && terraform::is_token(target))
                .then(|| Self::Terraform(terraform::Launch { token: target.to_owned() })),
            id if external::is_external_id(id) && action == external::RUN_ACTION => {
                let argv = external::parse_argv(target)?;
                Some(Self::External(external::Launch { id: id.to_owned(), argv }))
            }
            _ => None,
        }
    }

    /// Its `plugin` query value.
    pub fn plugin(&self) -> &str {
        match self {
            Self::Docker(_) => "docker",
            #[cfg(feature = "plugin-db")]
            Self::Db(_) => db::ID,
            #[cfg(feature = "plugin-terraform")]
            Self::Terraform(_) => terraform::ID,
            Self::External(launch) => &launch.id,
        }
    }

    /// Its `action` query value.
    pub fn action(&self) -> &'static str {
        match self {
            Self::Docker(launch) => launch.action.as_str(),
            #[cfg(feature = "plugin-db")]
            Self::Db(_) => db::CONSOLE_ACTION,
            #[cfg(feature = "plugin-terraform")]
            Self::Terraform(_) => terraform::RUN_ACTION,
            Self::External(_) => external::RUN_ACTION,
        }
    }

    /// Its `target` query value.
    pub fn target(&self) -> String {
        match self {
            Self::Docker(launch) => launch.container.clone(),
            #[cfg(feature = "plugin-db")]
            Self::Db(launch) => launch.token.clone(),
            #[cfg(feature = "plugin-terraform")]
            Self::Terraform(launch) => launch.token.clone(),
            Self::External(launch) => serde_json::to_string(&launch.argv).expect("strings serialize"),
        }
    }

    /// The line a new shell types. `shell` is the new shell's kind (None for
    /// any other shell). Docker's fixed commands read the same in every shell.
    pub fn command(&self, shell: Option<ShellKind>) -> Result<String, LaunchError> {
        match self {
            Self::Docker(launch) => Ok(docker::command(launch)),
            #[cfg(feature = "plugin-db")]
            Self::Db(launch) => db::command(launch, shell.ok_or(LaunchError::UnsupportedShell)?),
            #[cfg(feature = "plugin-terraform")]
            Self::Terraform(launch) => terraform::command(launch, shell.ok_or(LaunchError::UnsupportedShell)?),
            Self::External(launch) => {
                let shell = shell.ok_or(LaunchError::UnsupportedShell)?;
                external::command(launch, shell).map_err(|_| LaunchError::ControlCharacter)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quote::ShellKind;

    #[test]
    fn launches_parse_only_known_plugins_actions_and_targets() {
        let launch = PluginLaunch::parse("docker", "shell", "0123456789ab").unwrap();
        assert_eq!((launch.plugin(), launch.action(), launch.target().as_str()), ("docker", "shell", "0123456789ab"));
        assert!(launch.command(None).unwrap().starts_with("docker exec -it 0123456789ab "));
        assert!(PluginLaunch::parse("nope", "logs", "0123456789ab").is_none());
        assert!(PluginLaunch::parse("docker", "rm", "0123456789ab").is_none());
        assert!(PluginLaunch::parse("docker", "logs", "x;rm -rf ~").is_none());
    }

    #[test]
    fn external_launches_quote_for_the_shell() {
        let launch = PluginLaunch::parse("k8s-tools.kubernetes", "run", r#"["kubectl","exec","-it","pod/it's","--","sh"]"#).unwrap();
        // Every word is quoted, and `env` runs it: argv[0] is a program on PATH,
        // never a builtin, keyword, function, or alias of the user's shell.
        assert_eq!(launch.command(Some(ShellKind::Sh)).unwrap(), r"'env' '--' 'kubectl' 'exec' '-it' 'pod/it'\''s' '--' 'sh'");
        assert_eq!(launch.command(Some(ShellKind::Fish)).unwrap(), r"'env' '--' 'kubectl' 'exec' '-it' 'pod/it\'s' '--' 'sh'");
        let plain = PluginLaunch::parse("k8s-tools.kubernetes", "run", r#"["kubectl","get","G"]"#).unwrap();
        assert_eq!(plain.command(Some(ShellKind::Sh)).unwrap(), "'env' '--' 'kubectl' 'get' 'G'", "a plain word may be a global alias");
        assert_eq!(plain.command(Some(ShellKind::Fish)).unwrap(), "'env' '--' 'kubectl' 'get' 'G'", "or a fish abbreviation");
        assert_eq!(launch.command(None), Err(LaunchError::UnsupportedShell));
        assert_eq!((launch.plugin(), launch.action()), ("k8s-tools.kubernetes", "run"));
        assert_eq!(launch.target(), r#"["kubectl","exec","-it","pod/it's","--","sh"]"#);
        assert!(PluginLaunch::parse("k8s-tools.kubernetes", "logs", r#"["kubectl"]"#).is_none());
        assert!(PluginLaunch::parse("docker", "run", r#"["kubectl"]"#).is_none());
        assert!(PluginLaunch::parse("docker", "logs", "0123456789ab").unwrap().command(None).is_ok());
    }

    /// A launch each listed plugin accepts.
    fn sample(id: &str) -> (&'static str, String) {
        match id {
            "db" => ("console", "a".repeat(32)),
            "terraform" => ("run", "a".repeat(32)),
            _ => ("logs", "0123456789ab".to_owned()),
        }
    }

    #[test]
    fn every_listed_plugin_with_launches_parses() {
        for id in PLUGIN_IDS.iter().filter(|id| **id != "git") {
            let (action, target) = sample(id);
            assert!(PluginLaunch::parse(id, action, &target).is_some(), "{id}");
        }
        assert!(PluginLaunch::parse("git", "logs", "0123456789ab").is_none(), "Git opens no shells");
    }

    #[test]
    fn default_on_plugins_are_known_plugins() {
        assert!(DEFAULT_ON.iter().all(|id| PLUGIN_IDS.contains(id)));
    }

    #[cfg(feature = "plugin-db")]
    #[test]
    fn db_launches_need_a_token() {
        let token = "a".repeat(32);
        let launch = PluginLaunch::parse("db", "console", &token).unwrap();
        assert_eq!((launch.plugin(), launch.action(), launch.target()), ("db", "console", token.clone()));
        assert!(PluginLaunch::parse("db", "run", &token).is_none());
        assert!(PluginLaunch::parse("db", "console", "x;rm -rf ~").is_none());
        assert!(PLUGIN_IDS.contains(&"db"));
    }
}
