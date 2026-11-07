//! Splits PTY output into text and Kitty graphics commands (`ESC _ G … ESC \`).
//! A command can arrive across any number of reads; the splitter keeps the
//! unfinished part. Other APC strings and every other byte stay text.

/// A piece of the stream, in order.
#[derive(Debug, PartialEq, Eq)]
pub enum Piece<'a> {
    Text(&'a [u8]),
    /// The command body between `ESC _ G` and `ESC \`.
    Graphics(Vec<u8>),
}

/// Bodies longer than this are dropped: Kitty clients send at most 4096
/// bytes of payload per chunk.
pub const MAX_BODY: usize = 1 << 20;

#[derive(Default)]
pub struct Splitter {
    state: State,
    body: Vec<u8>,
    overflow: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum State {
    #[default]
    Text,
    /// Saw ESC in text.
    Esc,
    /// Saw `ESC _`.
    EscUnderscore,
    /// Inside a graphics command.
    Body,
    /// Saw ESC inside a graphics command.
    BodyEsc,
}

impl State {
    /// Bytes of a possible opener (ESC, or ESC _) seen but not yet passed on.
    fn prefix_len(self) -> usize {
        match self {
            State::Esc => 1,
            State::EscUnderscore => 2,
            _ => 0,
        }
    }
}

impl Splitter {
    /// Calls `on` for each piece of `data`, in order.
    pub fn feed<'a>(&mut self, data: &'a [u8], mut on: impl FnMut(Piece<'a>)) {
        // Opener bytes held back from the last read. They come before
        // data[0] once it is clear whether they open a command.
        let mut held = self.state.prefix_len();
        let mut text_from = 0;
        for (i, &byte) in data.iter().enumerate() {
            let in_body = matches!(self.state, State::Body | State::BodyEsc);
            match (self.state, byte) {
                (State::Text, 0x1b) => self.state = State::Esc,
                (State::Text, _) => {}
                (State::Esc, b'_') => self.state = State::EscUnderscore,
                (State::EscUnderscore, b'G') => {
                    // The opener's bytes in this read start here.
                    let opener_at = (i + held).saturating_sub(2);
                    if opener_at > text_from {
                        on(Piece::Text(&data[text_from..opener_at]));
                    }
                    held = 0;
                    self.state = State::Body;
                    self.body.clear();
                    self.overflow = false;
                    text_from = i + 1;
                }
                (State::Esc | State::EscUnderscore, _) => {
                    // Not an opener: held bytes are plain text after all.
                    if held > 0 {
                        on(Piece::Text(&b"\x1b_"[..held]));
                        held = 0;
                    }
                    self.state = if byte == 0x1b { State::Esc } else { State::Text };
                }
                (State::Body, 0x1b) => self.state = State::BodyEsc,
                (State::Body, _) => self.push(byte),
                (State::BodyEsc, b'\\') => {
                    let body = std::mem::take(&mut self.body);
                    if !self.overflow {
                        on(Piece::Graphics(body));
                    }
                    self.state = State::Text;
                }
                (State::BodyEsc, _) => {
                    // Not a terminator: the ESC belongs to the body.
                    self.push(0x1b);
                    if byte != 0x1b {
                        self.push(byte);
                        self.state = State::Body;
                    }
                }
            }
            if in_body {
                text_from = i + 1;
            }
        }
        // Hold a trailing opener prefix until the next read shows what follows.
        let text_to = data.len() - (self.state.prefix_len() - held);
        if text_to > text_from {
            on(Piece::Text(&data[text_from..text_to]));
        }
    }

    fn push(&mut self, byte: u8) {
        if self.body.len() < MAX_BODY {
            self.body.push(byte);
        } else {
            self.overflow = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn split(splitter: &mut Splitter, data: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>) {
        let mut text = Vec::new();
        let mut commands = Vec::new();
        splitter.feed(data, |piece| match piece {
            Piece::Text(bytes) => text.extend_from_slice(bytes),
            Piece::Graphics(body) => commands.push(body),
        });
        (text, commands)
    }

    #[test]
    fn text_passes_through_and_a_command_is_taken_out() {
        let mut splitter = Splitter::default();
        let (text, commands) = split(&mut splitter, b"a\x1b[1;1H\x1b_Ga=T,i=1;QUJD\x1b\\b");
        assert_eq!(text, b"a\x1b[1;1Hb");
        assert_eq!(commands, vec![b"a=T,i=1;QUJD".to_vec()]);
    }

    #[test]
    fn a_command_split_across_reads_is_joined() {
        let stream = b"x\x1b_Ga=T;AAAA\x1b\\y";
        // Every split point, including inside ESC _ G and ESC \.
        for at in 0..stream.len() {
            let mut splitter = Splitter::default();
            let (t1, c1) = split(&mut splitter, &stream[..at]);
            let (t2, c2) = split(&mut splitter, &stream[at..]);
            assert_eq!([t1, t2].concat(), b"xy", "split at {at}");
            assert_eq!([c1, c2].concat(), vec![b"a=T;AAAA".to_vec()], "split at {at}");
        }
    }

    #[test]
    fn text_survives_one_byte_reads() {
        let stream = b"\x1b\x1b_X\x1b_\x1b_Gq=2\x1b\x1b\\z\x1b";
        let mut splitter = Splitter::default();
        let mut text = Vec::new();
        let mut commands = Vec::new();
        for byte in stream.chunks(1) {
            let (t, c) = split(&mut splitter, byte);
            text.extend(t);
            commands.extend(c);
        }
        assert_eq!(text, b"\x1b\x1b_X\x1b_z");
        assert_eq!(commands, vec![b"q=2\x1b".to_vec()]);
    }

    #[test]
    fn other_escapes_and_apc_strings_stay_text() {
        let mut splitter = Splitter::default();
        let (text, commands) = split(&mut splitter, b"\x1b_Xnot graphics\x1b\\\x1b]2;t\x07\x1b\x1b_Gq=2\x1b\\");
        assert_eq!(text, b"\x1b_Xnot graphics\x1b\\\x1b]2;t\x07\x1b");
        assert_eq!(commands, vec![b"q=2".to_vec()]);
    }

    #[test]
    fn an_oversized_command_is_dropped_and_the_stream_recovers() {
        let mut splitter = Splitter::default();
        let mut stream = b"\x1b_Ga=T;".to_vec();
        stream.extend(std::iter::repeat_n(b'A', MAX_BODY + 10));
        stream.extend_from_slice(b"\x1b\\ok");
        let (text, commands) = split(&mut splitter, &stream);
        assert_eq!(text, b"ok");
        assert!(commands.is_empty());
    }
}
