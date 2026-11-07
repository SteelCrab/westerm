//! Kitty graphics for pixel apps (terminal-browser, terminal-code), handled in
//! the PTY host instead of the browser.
//!
//! The browser pane cannot read shared memory, so these apps used to send
//! every frame as base64 text through the PTY, and the pane decoded it on its
//! main thread. Here the host takes the graphics commands out of the output,
//! answers the app's questions (shared memory works), keeps the picture, and
//! sends the browser only the changed rectangles as binary messages.

mod apc;
mod canvas;
mod kitty;

use std::num::NonZeroU16;
use std::time::{Duration, Instant};

use crate::protocol::PtySize;

/// First byte of each binary message to a viewer that takes pixel updates.
pub const KIND_TEXT: u8 = 0;
pub const KIND_PIXELS: u8 = 1;
/// Flag in a pixel update: rectangle data is raw deflate.
pub const FLAG_DEFLATE: u8 = 1;
/// Most image bytes kept per session, as the browser pane allowed before.
pub const IMAGE_BUDGET: usize = 96 << 20;

pub use crate::protocol::PixelCodec as Codec;

/// What one read of PTY output turned into.
#[derive(Default)]
pub struct Fed {
    /// Output without graphics commands, for the terminal in the browser.
    pub text: Vec<u8>,
    /// Answers for the app, to write to the PTY.
    pub reply: Vec<u8>,
}

/// A synchronized update open longer than this is shown anyway, as
/// terminals do for an app that never closes one.
pub const SYNC_LIMIT: Duration = Duration::from_secs(1);

/// Terminal modes vt100 leaves to its callbacks: synchronized output (2026).
#[derive(Default)]
struct Modes {
    syncing_since: Option<Instant>,
    /// A synchronized update ended since the last look.
    ended: bool,
}

impl vt100::Callbacks for Modes {
    fn unhandled_csi(&mut self, _: &mut vt100::Screen, i1: Option<u8>, _: Option<u8>, params: &[&[u16]], c: char) {
        if i1 != Some(b'?') || !params.iter().any(|param| param.first() == Some(&2026)) {
            return;
        }
        match c {
            'h' => self.syncing_since = self.syncing_since.or_else(|| Some(Instant::now())),
            'l' => {
                self.ended |= self.syncing_since.is_some();
                self.syncing_since = None;
            }
            _ => {}
        }
    }
}

pub struct PixelStream {
    splitter: apc::Splitter,
    /// Follows the cursor, which says where an image goes, and the screen.
    vt: vt100::Parser<Modes>,
    alternate: bool,
    canvas: canvas::Canvas,
    /// A chunked transmission: its first command and the payload so far.
    transfer: Option<(kitty::Command, Vec<u8>)>,
}

fn grid(size: PtySize) -> canvas::Grid {
    canvas::Grid {
        cols: u32::from(size.cols),
        rows: u32::from(size.rows),
        width: u32::from(size.width),
        height: u32::from(size.height),
    }
}

fn dims(size: PtySize) -> (NonZeroU16, NonZeroU16) {
    (NonZeroU16::new(size.rows.max(1)).unwrap(), NonZeroU16::new(size.cols.max(1)).unwrap())
}

impl PixelStream {
    pub fn new(size: PtySize) -> Self {
        let (rows, cols) = dims(size);
        Self {
            splitter: apc::Splitter::default(),
            vt: vt100::Parser::new_with_callbacks(rows, cols, 0, Modes::default()),
            alternate: false,
            canvas: canvas::Canvas::new(grid(size), IMAGE_BUDGET),
            transfer: None,
        }
    }

    pub fn feed(&mut self, data: &[u8]) -> Fed {
        let mut fed = Fed::default();
        let mut commands = Vec::new();
        // Text and commands in order: the cursor must be where the text left it.
        let mut pieces = Vec::new();
        self.splitter.feed(data, |piece| match piece {
            apc::Piece::Text(text) => pieces.push(Ok(text)),
            apc::Piece::Graphics(body) => {
                commands.push(body);
                pieces.push(Err(commands.len() - 1));
            }
        });
        for piece in pieces {
            match piece {
                Ok(text) => {
                    fed.text.extend_from_slice(text);
                    self.vt.process(text);
                    let alternate = self.vt.screen().alternate_screen();
                    if alternate != self.alternate {
                        // Each screen has its own images; the new one starts empty.
                        self.alternate = alternate;
                        self.canvas.delete(None, None, true);
                    }
                    // A frame ends here: the commands after it are the next one.
                    if std::mem::take(&mut self.vt.callbacks_mut().ended) {
                        self.canvas.finish_frame();
                    }
                }
                Err(index) => {
                    if let Some(reply) = self.command(&commands[index]) {
                        fed.reply.extend(reply);
                    }
                }
            }
        }
        if self.syncing_since().is_none_or(|since| since.elapsed() > SYNC_LIMIT) {
            self.canvas.finish_frame();
        }
        fed
    }

    pub fn resize(&mut self, size: PtySize) {
        let (rows, cols) = dims(size);
        self.vt.screen_mut().set_size(rows, cols);
        self.canvas.resize(grid(size));
        if self.syncing_since().is_none() {
            self.canvas.finish_frame();
        }
    }

    /// A finished frame waits to be sent.
    #[cfg(test)]
    pub fn ready(&self) -> bool {
        self.canvas.has_finished()
    }

    /// When the open synchronized update started, if one is open.
    pub fn syncing_since(&self) -> Option<Instant> {
        self.vt.callbacks().syncing_since
    }

    /// The rectangles of the finished frames not yet sent, as one message;
    /// None when there are none.
    pub fn take_update(&mut self, codec: Codec) -> Option<Vec<u8>> {
        if !self.canvas.has_finished() {
            return None;
        }
        let (width, height) = self.canvas.size();
        Some(encode(width, height, self.canvas.take_finished(), codec))
    }

    /// Handles one command; returns the reply for the app, if any.
    fn command(&mut self, body: &[u8]) -> Option<Vec<u8>> {
        let (keys, payload) = kitty::parse(body);
        let decoded = kitty::decode_base64(payload);
        let (command, data) = match self.transfer.take() {
            Some((first, mut data)) => {
                // A later chunk carries only m (and maybe q).
                data.extend(decoded.unwrap_or_default());
                if keys.more {
                    self.transfer = Some((first, data));
                    return None;
                }
                (first, data)
            }
            None => {
                let data = decoded.unwrap_or_default();
                if keys.more {
                    self.transfer = Some((keys, data));
                    return None;
                }
                (keys, data)
            }
        };
        let result = self.run(&command, &data);
        kitty::reply(&command, &result)
    }

    fn run(&mut self, command: &kitty::Command, data: &[u8]) -> Result<(), String> {
        match command.action {
            b'q' => kitty::load(command, data).map(|_| ()),
            b't' | b'T' => {
                let image = kitty::load(command, data)?;
                self.canvas.transmit(command.id, image);
                if command.action == b'T' { self.place(command) } else { Ok(()) }
            }
            b'p' => self.place(command),
            b'd' => {
                let free = command.delete.is_ascii_uppercase();
                match command.delete.to_ascii_lowercase() {
                    b'a' => self.canvas.delete(None, None, free),
                    b'i' => {
                        let placement = (command.placement != 0).then_some(command.placement);
                        self.canvas.delete(Some(command.id), placement, free);
                    }
                    _ => return Err("EINVAL:this delete is not supported".into()),
                }
                Ok(())
            }
            _ => Err("EINVAL:this action is not supported".into()),
        }
    }

    /// Shows an image at the cursor. Pixel apps send C=1, so the cursor
    /// stays; this host does not move it either way.
    fn place(&mut self, command: &kitty::Command) -> Result<(), String> {
        let (row, col) = self.vt.screen().cursor_position();
        let src = (command.src.w != 0 || command.src.h != 0 || command.src.x != 0 || command.src.y != 0).then(|| {
            let mut src = command.src;
            // Zero width or height means "to the image edge".
            if src.w == 0 {
                src.w = u32::MAX - src.x;
            }
            if src.h == 0 {
                src.h = u32::MAX - src.y;
            }
            src
        });
        let place = canvas::Place {
            image: command.id,
            id: command.placement,
            row: u32::from(row),
            col: u32::from(col),
            dx: command.dx,
            dy: command.dy,
            src,
            z: command.z,
        };
        if self.canvas.place(place) { Ok(()) } else { Err("ENOENT:no such image".into()) }
    }
}

/// One pixel update: frame size, then each rectangle with its pixels.
fn encode(width: u32, height: u32, rects: Vec<(canvas::Rect, Vec<u8>)>, codec: Codec) -> Vec<u8> {
    let flags = if codec == Codec::Deflate { FLAG_DEFLATE } else { 0 };
    let size: usize = rects.iter().map(|(_, data)| 12 + data.len()).sum();
    let mut out = Vec::with_capacity(8 + size);
    out.push(KIND_PIXELS);
    out.push(flags);
    out.extend_from_slice(&(width as u16).to_le_bytes());
    out.extend_from_slice(&(height as u16).to_le_bytes());
    out.extend_from_slice(&(rects.len() as u16).to_le_bytes());
    for (rect, data) in rects {
        for value in [rect.x, rect.y, rect.w, rect.h] {
            out.extend_from_slice(&(value as u16).to_le_bytes());
        }
        let data = match codec {
            Codec::Raw => data,
            Codec::Deflate => deflate(&data),
        };
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
    }
    out
}

fn deflate(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut encoder = flate2::write::DeflateEncoder::new(Vec::with_capacity(data.len() / 4), flate2::Compression::fast());
    let _ = encoder.write_all(data);
    encoder.finish().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];

    /// 4 x 2 cells of 10 x 10 pixels.
    fn stream() -> PixelStream {
        PixelStream::new(PtySize { cols: 4, rows: 2, width: 40, height: 20 })
    }

    fn b64(data: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(data)
    }

    /// An inline RGBA image command.
    fn image(keys: &str, width: u32, height: u32, color: [u8; 4]) -> String {
        let pixels = color.repeat((width * height) as usize);
        format!("\x1b_G{keys},f=32,s={width},v={height};{}\x1b\\", b64(&pixels))
    }

    struct Update {
        width: u16,
        height: u16,
        flags: u8,
        rects: Vec<(u16, u16, u16, u16, Vec<u8>)>,
    }

    fn decode(message: &[u8]) -> Update {
        let u16_at = |at: usize| u16::from_le_bytes([message[at], message[at + 1]]);
        assert_eq!(message[0], KIND_PIXELS);
        let mut update = Update { flags: message[1], width: u16_at(2), height: u16_at(4), rects: Vec::new() };
        let mut at = 8;
        for _ in 0..u16_at(6) {
            let len = u32::from_le_bytes(message[at + 8..at + 12].try_into().unwrap()) as usize;
            let data = message[at + 12..at + 12 + len].to_vec();
            update.rects.push((u16_at(at), u16_at(at + 2), u16_at(at + 4), u16_at(at + 6), data));
            at += 12 + len;
        }
        assert_eq!(at, message.len(), "no bytes after the last rectangle");
        update
    }

    fn bounds(rect: &(u16, u16, u16, u16, Vec<u8>)) -> [u16; 4] {
        [rect.0, rect.1, rect.2, rect.3]
    }

    /// The color at a frame pixel in an update that covers it.
    fn color_at(update: &Update, x: u16, y: u16) -> Option<[u8; 4]> {
        update.rects.iter().find_map(|(rx, ry, w, h, data)| {
            (x >= *rx && y >= *ry && x < rx + w && y < ry + h).then(|| {
                let at = ((y - ry) as usize * *w as usize + (x - rx) as usize) * 4;
                data[at..at + 4].try_into().unwrap()
            })
        })
    }

    #[test]
    fn an_inline_query_is_answered() {
        let fed = stream().feed(b"\x1b_Gi=4207,a=q,t=d,f=24,s=1,v=1;AAAA\x1b\\\x1b[c");
        assert_eq!(fed.reply, b"\x1b_Gi=4207;OK\x1b\\");
        assert_eq!(fed.text, b"\x1b[c", "the rest still reaches the terminal");
    }

    #[cfg(unix)]
    #[test]
    fn shared_memory_is_offered_when_the_probe_segment_can_be_read() {
        let name = format!("/wsp-{}-probe", std::process::id());
        let c_name = std::ffi::CString::new(name.clone()).unwrap();
        unsafe {
            let fd = libc::shm_open(c_name.as_ptr(), libc::O_CREAT | libc::O_RDWR, 0o600);
            libc::ftruncate(fd, 4);
            libc::close(fd);
        }
        let probe = format!("\x1b_Gi=299,a=q,t=s,f=32,s=1,v=1;{}\x1b\\", b64(name.as_bytes()));
        let file = format!("\x1b_Gi=300,a=q,t=f,f=32,s=1,v=1;{}\x1b\\", b64(b"/tmp/x"));
        let fed = stream().feed(format!("{probe}{file}").as_bytes());
        let reply = String::from_utf8(fed.reply).unwrap();
        assert!(reply.starts_with("\x1b_Gi=299;OK\x1b\\"), "{reply:?}");
        assert!(reply.contains("i=300;E"), "files are refused: {reply:?}");
    }

    #[test]
    fn an_image_leaves_the_text_and_lands_at_the_cursor() {
        let mut stream = stream();
        let input = format!("\x1b[2;3H{}after", image("a=T,i=1,p=1,C=1,q=2,X=1,Y=2", 2, 2, RED));
        let fed = stream.feed(input.as_bytes());
        assert_eq!(fed.text, b"\x1b[2;3Hafter");
        assert!(fed.reply.is_empty(), "q=2 hides replies");
        assert!(stream.ready());
        let update = decode(&stream.take_update(Codec::Raw).unwrap());
        assert_eq!((update.width, update.height, update.flags), (40, 20, 0));
        assert_eq!(update.rects.len(), 1);
        assert_eq!(bounds(&update.rects[0]), [21, 12, 2, 2]);
        assert_eq!(color_at(&update, 22, 13), Some(RED));
        assert!(stream.take_update(Codec::Raw).is_none(), "nothing changed since");
    }

    #[test]
    fn a_chunked_transfer_is_joined() {
        let pixels = GREEN.repeat(4);
        let encoded = b64(&pixels);
        let (first, rest) = encoded.split_at(8);
        let input = format!("\x1b_Ga=T,i=2,f=32,s=2,v=2,q=2,m=1;{first}\x1b\\\x1b_Gm=0;{rest}\x1b\\");
        let mut stream = stream();
        stream.feed(input.as_bytes());
        let update = decode(&stream.take_update(Codec::Raw).unwrap());
        assert_eq!(color_at(&update, 1, 1), Some(GREEN));
    }

    #[test]
    fn a_frame_is_ready_only_when_its_synchronized_update_ends() {
        let mut stream = stream();
        stream.feed(format!("\x1b[?2026h\x1b[H{}", image("a=T,i=1,q=2", 1, 1, RED)).as_bytes());
        assert!(!stream.ready());
        assert!(stream.syncing_since().is_some());
        stream.feed(b"\x1b[?2026l");
        assert!(stream.ready());
        assert!(stream.syncing_since().is_none());
    }

    #[test]
    fn a_finished_frame_is_sent_while_the_next_one_is_still_open() {
        let mut stream = stream();
        let first = format!("\x1b[?2026h\x1b[H{}\x1b[?2026l", image("a=T,i=1,q=2", 2, 2, RED));
        let second = format!("\x1b[?2026h\x1b[H{}", image("a=T,i=2,z=1,q=2", 2, 2, GREEN));
        stream.feed(format!("{first}{second}").as_bytes());
        assert!(stream.ready(), "the first frame is complete");
        let update = decode(&stream.take_update(Codec::Raw).unwrap());
        assert_eq!(color_at(&update, 0, 0), Some(RED), "not half of the second frame");
        assert!(!stream.ready());
        stream.feed(b"\x1b[?2026l");
        assert_eq!(color_at(&decode(&stream.take_update(Codec::Raw).unwrap()), 0, 0), Some(GREEN));
    }

    #[test]
    fn put_and_delete_commands_change_the_picture() {
        let mut stream = stream();
        stream.feed(image("a=t,i=3,q=2", 2, 2, RED).as_bytes());
        stream.take_update(Codec::Raw);
        stream.feed(b"\x1b[1;2H\x1b_Ga=p,i=3,p=1,q=2\x1b\\");
        assert_eq!(color_at(&decode(&stream.take_update(Codec::Raw).unwrap()), 10, 0), Some(RED));
        stream.feed(b"\x1b_Ga=d,d=i,i=3,q=2\x1b\\");
        assert_eq!(color_at(&decode(&stream.take_update(Codec::Raw).unwrap()), 10, 0), Some([0; 4]));
        stream.feed(b"\x1b_Ga=d,d=I,i=3,q=2\x1b\\\x1b_Ga=p,i=3,p=1\x1b\\");
        let fed = stream.feed(b"\x1b_Ga=p,i=3,p=1\x1b\\");
        assert!(String::from_utf8(fed.reply).unwrap().contains("ENOENT"), "a freed image cannot be shown");
    }

    #[test]
    fn leaving_the_alternate_screen_clears_the_picture() {
        let mut stream = stream();
        stream.feed(format!("\x1b[?1049h{}", image("a=T,i=1,q=2", 40, 20, RED)).as_bytes());
        stream.take_update(Codec::Raw);
        stream.feed(b"\x1b[?1049l");
        let update = decode(&stream.take_update(Codec::Raw).unwrap());
        assert_eq!(color_at(&update, 39, 19), Some([0; 4]));
    }

    #[test]
    fn a_resize_redraws_the_whole_frame() {
        let mut stream = stream();
        stream.feed(image("a=T,i=1,q=2", 2, 2, RED).as_bytes());
        stream.take_update(Codec::Raw);
        stream.resize(PtySize { cols: 4, rows: 2, width: 80, height: 40 });
        let update = decode(&stream.take_update(Codec::Raw).unwrap());
        assert_eq!((update.width, update.height), (80, 40));
        assert_eq!(bounds(&update.rects[0]), [0, 0, 80, 40]);
    }

    /// Replays recorded PTY output (bench/pixel/capture.py) and prints the
    /// time spent. Run with:
    /// WESTERM_PIXEL_REPLAY=out.bin cargo test --release replay_a_recording -- --ignored --nocapture
    #[test]
    #[ignore]
    fn replay_a_recording() {
        let Ok(path) = std::env::var("WESTERM_PIXEL_REPLAY") else { return };
        let data = std::fs::read(&path).unwrap();
        let size = PtySize { cols: 150, rows: 43, width: 1350, height: 780 };
        for codec in [Codec::Raw, Codec::Deflate] {
            let mut stream = PixelStream::new(size);
            let (mut updates, mut update_bytes, mut text_bytes) = (0, 0, 0);
            let start = Instant::now();
            for chunk in data.chunks(64 * 1024) {
                text_bytes += stream.feed(chunk).text.len();
                if stream.ready() {
                    let update = stream.take_update(codec).unwrap();
                    updates += 1;
                    update_bytes += update.len();
                }
            }
            let ms = start.elapsed().as_secs_f64() * 1000.0;
            println!(
                "{{\"file\":{path:?},\"codec\":\"{}\",\"inputBytes\":{},\"hostMs\":{ms:.1},\"updates\":{updates},\"updateBytes\":{update_bytes},\"textBytes\":{text_bytes}}}",
                codec.as_str(),
                data.len()
            );
        }
    }

    #[test]
    fn deflate_updates_carry_the_same_pixels() {
        use std::io::Read;
        let mut stream = stream();
        stream.feed(image("a=T,i=1,q=2", 40, 20, GREEN).as_bytes());
        let update = decode(&stream.take_update(Codec::Deflate).unwrap());
        assert_eq!(update.flags, FLAG_DEFLATE);
        let (_, _, w, h, data) = &update.rects[0];
        assert!(data.len() < (*w as usize * *h as usize * 4) / 10, "a flat color compresses");
        let mut pixels = Vec::new();
        flate2::read::DeflateDecoder::new(&data[..]).read_to_end(&mut pixels).unwrap();
        assert_eq!(pixels, GREEN.repeat(800));
    }
}
