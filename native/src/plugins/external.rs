//! Shell panes of installed plugins. A plugin asks for a program and its
//! arguments; the session query carries them as a JSON array of strings
//! (`action=run&target=["kubectl","logs","pod/x"]`), and the host quotes them
//! for the user's shell (see quote.rs). The server and the host check the
//! array with the same function.

use crate::quote::{has_control, quoted_line, ControlCharacter, ShellKind};

/// The `action` of every external launch.
pub const RUN_ACTION: &str = "run";
/// Most words in a launch, the program included.
pub const MAX_ARGS: usize = 32;
/// Longest word in a launch, in UTF-8 bytes.
pub const MAX_ARG_BYTES: usize = 1024;

/// A program and its arguments, checked by `parse_argv`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub id: String,
    pub argv: Vec<String>,
}

/// `<author>.<name>`, each part `[a-z0-9-]{1,32}`. Built-in ids have no dot.
pub fn is_external_id(id: &str) -> bool {
    let part = |text: &str| (1..=32).contains(&text.len()) && text.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'));
    id.split_once('.').is_some_and(|(author, name)| part(author) && part(name))
}

/// A program found on PATH, never a path or an option: `[A-Za-z0-9][A-Za-z0-9._-]*`.
pub fn is_program_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_alphanumeric())
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// A launch target: a JSON array of 1 to `MAX_ARGS` strings, each at most
/// `MAX_ARG_BYTES` long and free of control characters, whose first word is a
/// program name.
pub fn parse_argv(target: &str) -> Option<Vec<String>> {
    let argv: Vec<String> = serde_json::from_str(target).ok()?;
    let fits = (1..=MAX_ARGS).contains(&argv.len())
        && argv.iter().all(|word| word.len() <= MAX_ARG_BYTES && !has_control(word))
        && is_program_name(&argv[0]);
    fits.then_some(argv)
}

/// The line a new shell types: `'env' '--' 'program' 'arg'...`. Through `env`,
/// the program is always one found on PATH, never a builtin, keyword, or
/// function of the user's shell (`export`, `trap`, `noglob`), which would run
/// as shell code. A program name has no `=`, so `env` never reads it as a
/// variable. `env` and `--` are quoted too, so no alias or abbreviation
/// rewrites them.
pub fn command(launch: &Launch, shell: ShellKind) -> Result<String, ControlCharacter> {
    let words: Vec<String> = ["env", "--"].into_iter().map(str::to_owned).chain(launch.argv.iter().cloned()).collect();
    quoted_line(&words, shell)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_ids_are_author_dot_name() {
        assert!(is_external_id("k8s-tools.kubernetes"));
        assert!(is_external_id(&format!("{}.{}", "a".repeat(32), "b".repeat(32))));
        let long = format!("{}.b", "a".repeat(33));
        for bad in ["docker", "a.b.c", "A.b", ".b", "a.", "a_b.c", "", ".", long.as_str()] {
            assert!(!is_external_id(bad), "{bad}");
        }
    }

    #[test]
    fn program_names_are_plain_names() {
        for ok in ["kubectl", "k9s", "aws", "terraform-1.5", "a_b"] {
            assert!(is_program_name(ok), "{ok}");
        }
        for bad in ["", "-x", ".x", "_x", "/bin/kubectl", "a/b", "a b", "a;b", "é"] {
            assert!(!is_program_name(bad), "{bad}");
        }
    }

    #[test]
    fn argv_targets_are_checked() {
        assert_eq!(parse_argv(r#"["kubectl","logs","pod/x"]"#), Some(vec!["kubectl".into(), "logs".into(), "pod/x".into()]));
        assert!(parse_argv(&serde_json::json!(["kubectl", "a".repeat(1024)]).to_string()).is_some());
        assert!(parse_argv(&serde_json::json!(vec!["x"; 32]).to_string()).is_some());
        let too_many = serde_json::json!(vec!["x"; 33]).to_string();
        let too_long = serde_json::json!(["kubectl", "a".repeat(1025)]).to_string();
        for bad in ["[]", r#"["/bin/kubectl"]"#, r#"["-x"]"#, "[1]", r#"["kubectl","a\u0000b"]"#, r#"["kubectl","a\u001bb"]"#, &too_many, &too_long] {
            assert!(parse_argv(bad).is_none(), "{bad}");
        }
    }

    fn launch(argv: &[&str]) -> Launch {
        Launch { id: "k8s-tools.kubernetes".into(), argv: argv.iter().map(|word| (*word).to_owned()).collect() }
    }

    #[test]
    fn commands_run_the_program_through_env() {
        let it = launch(&["kubectl", "logs", "it's", "G"]);
        assert_eq!(command(&it, ShellKind::Sh).as_deref(), Ok(r"'env' '--' 'kubectl' 'logs' 'it'\''s' 'G'"));
        assert_eq!(command(&it, ShellKind::Fish).as_deref(), Ok(r"'env' '--' 'kubectl' 'logs' 'it\'s' 'G'"));
        assert_eq!(command(&launch(&["kubectl", "a\u{3}"]), ShellKind::Sh), Err(ControlCharacter));
    }

    /// The typed line in a real interactive-like zsh: a global alias does not
    /// rewrite a word or the `env --` prefix, and a builtin name runs no builtin.
    #[cfg(unix)]
    #[test]
    fn zsh_aliases_and_builtins_do_not_apply() {
        let run = |argv: &[&str]| {
            let line = command(&launch(argv), ShellKind::Sh).unwrap();
            let script = r#"alias -g G='| tr a-z A-Z' env='echo HIJACKED'; alias -g -- --='HIJACKED'; eval "$1""#;
            std::process::Command::new("zsh").args(["-f", "-c", script, "zsh", &line]).output()
        };
        let Ok(output) = run(&["echo", "hi", "G"]) else { return }; // zsh is not installed
        assert_eq!(String::from_utf8_lossy(&output.stdout), "hi G\n");
        let output = run(&["export", "X=1"]).unwrap();
        assert_eq!(output.status.code(), Some(127), "env finds no program named export");
    }
}
