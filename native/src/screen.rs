//! The host's copy of a shell's screen: replayed to a reattaching browser and
//! saved to disk for revival after the process is gone.

use std::fmt::Write;
use std::num::NonZeroU16;

use vt100::capture::{BasicFormattedCaptureRange, BasicFormattedCaptureState};

use crate::i18n::Lang;
use crate::protocol::PtySize;

/// Rows of history kept and replayed.
pub const HISTORY_LINES: usize = 5000;
/// Rows written to disk.
pub const SAVED_LINES: usize = 1000;

pub struct Screen {
    parser: vt100::Parser,
}

fn dims(size: PtySize) -> (NonZeroU16, NonZeroU16) {
    (NonZeroU16::new(size.rows.max(1)).unwrap(), NonZeroU16::new(size.cols.max(1)).unwrap())
}

impl Screen {
    pub fn new(size: PtySize) -> Self {
        let (rows, cols) = dims(size);
        Self { parser: vt100::Parser::new(rows, cols, HISTORY_LINES) }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    pub fn resize(&mut self, size: PtySize) {
        let (rows, cols) = dims(size);
        self.parser.screen_mut().set_size(rows, cols);
    }

    /// Bytes that rebuild this screen, history first, in a fresh terminal of
    /// the same size: rows, colors, cursor, and input modes.
    pub fn replay(&self) -> Vec<u8> {
        let screen = self.parser.screen();
        let mut out = String::new();
        if screen.alternate_screen() {
            // A full-screen program: its own screen only; the normal screen
            // behind it is not kept by the parser.
            out.push_str("\x1b[?1049h");
            out.push_str(&screen.state_formatted());
            return out.into_bytes();
        }
        let mut state = BasicFormattedCaptureState::default();
        let mut body = String::new();
        // Writing into a String cannot fail.
        let _ = screen.write_contents_formatted_basic(&mut body, BasicFormattedCaptureRange::Full(&mut state));
        out.push_str(&body.replace('\n', "\r\n"));
        out.push_str("\x1b[m");
        let (row, col) = screen.cursor_position();
        let _ = write!(out, "\x1b[{};{}H", row + 1, col + 1);
        out.push_str(&screen.attributes_formatted());
        out.push_str(&screen.input_mode_formatted());
        if screen.hide_cursor() {
            out.push_str("\x1b[?25l");
        }
        out.into_bytes()
    }

    /// Up to `count` rows in view as plain text, ending at the last row with
    /// text: where an agent shows its status line and its questions, also
    /// before its output reaches the bottom of the screen.
    pub fn last_text(&self, count: usize) -> String {
        let screen = self.parser.screen();
        let rows: Vec<String> = screen.rows(0, screen.size().1.get()).collect();
        let end = rows.iter().rposition(|row| !row.trim().is_empty()).map_or(0, |last| last + 1);
        rows[end.saturating_sub(count)..end].join("\n")
    }

    /// Plain colored lines for disk, without modes. None while a full-screen
    /// program runs: the previous snapshot of the normal screen stays.
    pub fn snapshot(&self) -> Option<String> {
        let screen = self.parser.screen();
        if screen.alternate_screen() {
            return None;
        }
        let mut state = BasicFormattedCaptureState::default();
        let mut body = String::new();
        let _ = screen.write_contents_formatted_basic(&mut body, BasicFormattedCaptureRange::Full(&mut state));
        let trimmed = body.trim_end();
        let lines: Vec<&str> = trimmed.split('\n').collect();
        let start = lines.len().saturating_sub(SAVED_LINES);
        Some(format!("{}\x1b[m", lines[start..].join("\r\n")))
    }
}

/// Shown between a saved screen and the new process that replaces it.
pub fn restored_notice(saved_at: &str, lang: Lang) -> String {
    format!("\x1b[0m\r\n\x1b[2m── {} ──\x1b[0m\r\n", lang.restored(&crate::time::local_label(saved_at)))
}

/// Shown under the restore notice when the shell resumes an AI agent.
pub fn agent_notice(agent: &str, conversation: Option<&str>, lang: Lang) -> String {
    let what = match conversation {
        Some(id) => format!("{agent} · {}", id.get(..8).unwrap_or(id)),
        None => agent.to_owned(),
    };
    format!("\x1b[2m── {} ──\x1b[0m\r\n", lang.resuming(&what))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: PtySize = PtySize { cols: 20, rows: 4, width: 160, height: 64 };

    fn rebuild(bytes: &[u8]) -> vt100::Parser {
        let mut fresh = vt100::Parser::new(NonZeroU16::new(4).unwrap(), NonZeroU16::new(20).unwrap(), 100);
        fresh.process(bytes);
        fresh
    }

    #[test]
    fn replay_rebuilds_screen_history_and_cursor() {
        let mut screen = Screen::new(SIZE);
        for i in 0..10 {
            screen.process(format!("line {i}\r\n").as_bytes());
        }
        screen.process("\x1b[31m한글 red\x1b[0m $ ".as_bytes());
        let rebuilt = rebuild(&screen.replay());
        let original = screen.parser.screen();
        assert_eq!(rebuilt.screen().contents(), original.contents());
        assert_eq!(rebuilt.screen().cursor_position(), original.cursor_position());
        assert_eq!(rebuilt.screen().scrollback_len(), original.scrollback_len(), "history survives");
        assert_eq!(rebuilt.screen().cell(3, 0).unwrap().fgcolor(), vt100::Color::Idx(1), "colors survive");
    }

    #[test]
    fn replay_keeps_full_screen_programs() {
        let mut screen = Screen::new(SIZE);
        screen.process(b"prompt\r\n\x1b[?1049h\x1b[Hvim buffer");
        let rebuilt = rebuild(&screen.replay());
        assert!(rebuilt.screen().alternate_screen());
        assert!(rebuilt.screen().contents().contains("vim buffer"));
        assert!(screen.snapshot().is_none(), "no snapshot of a full-screen program");
    }

    #[test]
    fn last_text_ends_at_the_last_row_with_text() {
        let mut screen = Screen::new(SIZE);
        screen.process(b"one\r\ntwo\r\n* esc to interrupt");
        assert_eq!(screen.last_text(2), "two\n* esc to interrupt", "the empty bottom row is skipped");
        assert_eq!(screen.last_text(99).lines().count(), 3);
        screen.process(b"\x1b[2J");
        assert_eq!(screen.last_text(5), "");
        assert!(agent_notice("Codex", None, Lang::En).contains("Resuming Codex"));
        assert!(agent_notice("Claude Code", Some("9f00f957-d4e0"), Lang::Ko).contains("Claude Code · 9f00f957"));
    }

    #[test]
    fn snapshots_keep_the_newest_lines() {
        let mut screen = Screen::new(SIZE);
        for i in 0..(SAVED_LINES + 50) {
            screen.process(format!("n{i}\r\n").as_bytes());
        }
        let saved = screen.snapshot().unwrap();
        assert!(saved.contains(&format!("n{}", SAVED_LINES + 49)));
        assert!(!saved.contains("\nn0\r"), "the oldest lines are dropped");
        assert!(saved.split("\r\n").count() <= SAVED_LINES);
    }
}
