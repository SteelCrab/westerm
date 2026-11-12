//! The Docker plugin's session launches. This file alone builds the commands
//! a shell types, and only from a checked container id.

/// A container id as `docker ps` prints it, in full or short: lowercase hex.
pub fn is_container_id(text: &str) -> bool {
    (12..=64).contains(&text.len()) && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// What a session does with a container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Logs,
    Shell,
}

impl Action {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "logs" => Some(Self::Logs),
            "shell" => Some(Self::Shell),
            _ => None,
        }
    }

    /// Its `action` query value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Logs => "logs",
            Self::Shell => "shell",
        }
    }
}

/// A container and what to do with it. The container is always a checked id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub action: Action,
    pub container: String,
}

/// The line a new shell types for this launch.
pub fn command(launch: &Launch) -> String {
    match launch.action {
        Action::Logs => format!("docker logs -f --tail 200 {}", launch.container),
        Action::Shell => {
            format!("docker exec -it {} sh -c 'command -v bash >/dev/null && exec bash || exec sh'", launch.container)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_ids_are_lowercase_hex_of_12_to_64() {
        assert!(is_container_id("0123456789ab"));
        assert!(is_container_id(&"a".repeat(64)));
        for bad in ["0123456789a", &"a".repeat(65), "0123456789AB", "0123456789ab;", "../etc/passwd"] {
            assert!(!is_container_id(bad), "{bad}");
        }
    }

    #[test]
    fn commands_use_only_the_checked_id() {
        let id = "0123456789ab".to_owned();
        assert_eq!(
            command(&Launch { action: Action::Logs, container: id.clone() }),
            "docker logs -f --tail 200 0123456789ab"
        );
        assert_eq!(
            command(&Launch { action: Action::Shell, container: id }),
            "docker exec -it 0123456789ab sh -c 'command -v bash >/dev/null && exec bash || exec sh'"
        );
    }

    #[test]
    fn actions_round_trip_through_their_names() {
        for action in [Action::Logs, Action::Shell] {
            assert_eq!(Action::parse(action.as_str()), Some(action));
        }
        assert_eq!(Action::parse("rm"), None);
    }
}
