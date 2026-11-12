//! Words a shell reads literally. A plugin pane and an agent resume type a
//! command line into a new shell, so each word is quoted by that shell's
//! rules. Control characters are refused, not quoted: the PTY acts on Ctrl-C,
//! Ctrl-U, Esc, and CR before the shell sees any quotes.

/// How a shell reads a quoted word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellKind {
    /// sh, bash, zsh, dash, ksh: nothing is special inside single quotes.
    Sh,
    /// fish: `\\` and `\'` are escapes inside single quotes.
    Fish,
}

impl ShellKind {
    /// Known by the program's file name. None for any other shell (nu, pwsh, ...).
    pub fn from_path(path: &str) -> Option<Self> {
        match path.rsplit('/').next().unwrap_or(path) {
            "sh" | "bash" | "zsh" | "dash" | "ksh" => Some(Self::Sh),
            "fish" => Some(Self::Fish),
            _ => None,
        }
    }
}

/// A word holds a control character (C0, DEL, or C1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlCharacter;

pub fn has_control(text: &str) -> bool {
    text.chars().any(char::is_control)
}

/// `word` as `shell` reads it literally. Plain words stay as they are.
pub fn quote_word(word: &str, shell: ShellKind) -> String {
    let plain = !word.is_empty()
        && word.bytes().all(|b| b.is_ascii_alphanumeric() || b"_./:=@+,-".contains(&b))
        // zsh reads a leading `=` as a command path (`=ls` is `/bin/ls`).
        && !(shell == ShellKind::Sh && word.starts_with('='));
    if plain {
        word.to_owned()
    } else {
        quoted(word, shell)
    }
}

/// `word` in single quotes, by `shell`'s rules.
fn quoted(word: &str, shell: ShellKind) -> String {
    match shell {
        ShellKind::Sh => format!("'{}'", word.replace('\'', r"'\''")),
        ShellKind::Fish => format!("'{}'", word.replace('\\', r"\\").replace('\'', r"\'")),
    }
}

/// The words as one command line for `shell`; plain words stay as they are.
/// For commands westerm builds itself (agent resumes).
pub fn command_line(words: &[String], shell: ShellKind) -> Result<String, ControlCharacter> {
    join(words, |word| quote_word(word, shell))
}

/// The words as one command line for `shell`, each in quotes. For words
/// westerm does not choose (plugin launches): an unquoted word can still be a
/// zsh global alias (`alias -g G='| grep'`) or a fish abbreviation.
pub fn quoted_line(words: &[String], shell: ShellKind) -> Result<String, ControlCharacter> {
    join(words, |word| quoted(word, shell))
}

fn join(words: &[String], quote: impl Fn(&str) -> String) -> Result<String, ControlCharacter> {
    if words.iter().any(|word| has_control(word)) {
        return Err(ControlCharacter);
    }
    Ok(words.iter().map(|word| quote(word)).collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sh_and_fish_quote_differently() {
        use ShellKind::*;
        assert_eq!(quote_word("it's", Sh), r"'it'\''s'");
        assert_eq!(quote_word("it's", Fish), r"'it\'s'");
        assert_eq!(quote_word(r"a\b", Sh), r"'a\b'");
        assert_eq!(quote_word(r"a\b", Fish), r"'a\\b'");
        assert_eq!(quote_word(r"\'", Fish), r"'\\\''");
        assert_eq!(quote_word("", Fish), "''");
        assert_eq!(quote_word("", Sh), "''");
        assert_eq!(quote_word("pod/web-1", Fish), "pod/web-1");
        assert_eq!(quote_word("100%", Sh), "'100%'");
    }

    #[test]
    fn sh_quotes_a_word_that_starts_with_equals() {
        // zsh reads `=ls` as the path of the `ls` command.
        assert_eq!(quote_word("=ls", ShellKind::Sh), "'=ls'");
        assert_eq!(quote_word("a=b", ShellKind::Sh), "a=b");
        assert_eq!(quote_word("=", ShellKind::Sh), "'='");
    }

    #[test]
    fn quoted_lines_quote_every_word() {
        // A plain word can still be a zsh global alias or a fish abbreviation.
        let words = ["kubectl".to_owned(), "G".into(), "it's".into(), r"a\b".into(), "".into()];
        assert_eq!(quoted_line(&words, ShellKind::Sh).as_deref(), Ok(r"'kubectl' 'G' 'it'\''s' 'a\b' ''"));
        assert_eq!(quoted_line(&words, ShellKind::Fish).as_deref(), Ok(r"'kubectl' 'G' 'it\'s' 'a\\b' ''"));
        assert_eq!(quoted_line(&["a\u{1b}".to_owned()], ShellKind::Sh), Err(ControlCharacter));
    }

    #[test]
    fn control_characters_are_refused() {
        for bad in ["a\u{3}b", "a\u{15}", "\u{1b}[A", "a\rb", "a\nb", "a\tb", "a\u{7f}", "a\u{85}", "a\u{9b}", "\0"] {
            assert!(command_line(&["echo".into(), bad.into()], ShellKind::Fish).is_err(), "{bad:?}");
            assert!(has_control(bad), "{bad:?}");
        }
        assert_eq!(command_line(&["echo".into(), "é ü".into()], ShellKind::Sh).as_deref(), Ok("echo 'é ü'"));
    }

    #[test]
    fn shells_are_known_by_file_name() {
        for path in ["/bin/sh", "/bin/bash", "/bin/zsh", "/bin/ksh", "/usr/bin/dash"] {
            assert_eq!(ShellKind::from_path(path), Some(ShellKind::Sh), "{path}");
        }
        assert_eq!(ShellKind::from_path("/opt/homebrew/bin/fish"), Some(ShellKind::Fish));
        for path in ["/opt/homebrew/bin/nu", "/usr/local/bin/pwsh", "/usr/bin/xonsh", "/usr/bin/elvish", ""] {
            assert_eq!(ShellKind::from_path(path), None, "{path}");
        }
    }

    #[cfg(unix)]
    type Builder = fn(&[String], ShellKind) -> Result<String, ControlCharacter>;

    /// Runs `printf '%s\n' <words>` in a real shell and reads the words back.
    /// None when the shell is not installed.
    #[cfg(unix)]
    fn echoed(shell: &str, kind: ShellKind, words: &[&str], build: Builder) -> Option<String> {
        let mut line = vec!["printf".to_owned(), r"%s\n".to_owned()];
        line.extend(words.iter().map(|word| (*word).to_owned()));
        let line = build(&line, kind).unwrap();
        let output = match std::process::Command::new(shell).arg("-c").arg(&line).output() {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
            Err(error) => panic!("{shell}: {error}"),
        };
        assert!(output.status.success(), "{shell} -c {line}: {}", String::from_utf8_lossy(&output.stderr));
        Some(String::from_utf8(output.stdout).unwrap())
    }

    #[cfg(unix)]
    #[test]
    fn quoted_words_round_trip_through_real_shells() {
        let words = ["it's", r"a\b", r"\'", "$(x)", "a b", "", "=ls", "100%", "%self", "~", "*", "a;b", "`x`", "!x"];
        let expected: String = words.iter().map(|word| format!("{word}\n")).collect();
        for (shell, kind) in [("sh", ShellKind::Sh), ("bash", ShellKind::Sh), ("zsh", ShellKind::Sh), ("fish", ShellKind::Fish)] {
            for build in [command_line as Builder, quoted_line] {
                // zsh and fish may be missing (Linux CI, this Mac without fish): skip that shell.
                if let Some(output) = echoed(shell, kind, &words, build) {
                    assert_eq!(output, expected, "{shell}");
                }
            }
        }
    }
}
