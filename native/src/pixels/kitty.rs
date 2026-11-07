//! Kitty graphics commands: the keys, the payload, and the pixels it names.
//! Pixels come inline (base64, maybe zlib) or through POSIX shared memory,
//! which the terminal reads and then unlinks, as Kitty does.

use super::canvas::{Image, Rect};

/// Largest decoded image accepted, in bytes.
pub const MAX_IMAGE_BYTES: usize = 64 << 20;
/// Largest side accepted, in pixels.
pub const MAX_SIDE: u32 = 10_000;

/// The keys of one command. Defaults follow the Kitty protocol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// a: t transmit, T transmit and show, p show, d delete, q query, f frame.
    pub action: u8,
    /// f: 24 RGB, 32 RGBA, 100 PNG.
    pub format: u32,
    /// t: d inline, s shared memory, f file, t temp file.
    pub medium: u8,
    /// o=z
    pub zlib: bool,
    /// s, v
    pub width: u32,
    pub height: u32,
    /// i, I, p
    pub id: u32,
    pub number: u32,
    pub placement: u32,
    /// x, y, w, h: part of the image to show.
    pub src: Rect,
    /// X, Y: pixel offset in the cell.
    pub dx: u32,
    pub dy: u32,
    pub z: i32,
    /// q: 1 hides OK replies, 2 hides every reply.
    pub quiet: u8,
    /// m=1: more chunks follow.
    pub more: bool,
    /// d: what to delete.
    pub delete: u8,
}

impl Default for Command {
    fn default() -> Self {
        Self {
            action: b't',
            format: 32,
            medium: b'd',
            zlib: false,
            width: 0,
            height: 0,
            id: 0,
            number: 0,
            placement: 0,
            src: Rect::default(),
            dx: 0,
            dy: 0,
            z: 0,
            quiet: 0,
            more: false,
            delete: b'a',
        }
    }
}

/// Splits a command body into its keys and its payload.
pub fn parse(body: &[u8]) -> (Command, &[u8]) {
    let (keys, payload) = match body.iter().position(|&b| b == b';') {
        Some(at) => (&body[..at], &body[at + 1..]),
        None => (body, &body[body.len()..]),
    };
    let mut command = Command::default();
    for pair in keys.split(|&b| b == b',') {
        let [key, b'=', value @ ..] = pair else { continue };
        let number = || std::str::from_utf8(value).ok().and_then(|text| text.parse::<u32>().ok()).unwrap_or(0);
        let letter = value.first().copied().unwrap_or(0);
        match key {
            b'a' => command.action = letter,
            b'f' => command.format = number(),
            b't' => command.medium = letter,
            b'o' => command.zlib = letter == b'z',
            b's' => command.width = number(),
            b'v' => command.height = number(),
            b'i' => command.id = number(),
            b'I' => command.number = number(),
            b'p' => command.placement = number(),
            b'x' => command.src.x = number(),
            b'y' => command.src.y = number(),
            b'w' => command.src.w = number(),
            b'h' => command.src.h = number(),
            b'X' => command.dx = number(),
            b'Y' => command.dy = number(),
            b'z' => command.z = std::str::from_utf8(value).ok().and_then(|text| text.parse().ok()).unwrap_or(0),
            b'q' => command.quiet = number().min(2) as u8,
            b'm' => command.more = number() == 1,
            b'd' => command.delete = letter,
            _ => {}
        }
    }
    (command, payload)
}

/// Decodes one chunk of base64 payload.
pub fn decode_base64(payload: &[u8]) -> Option<Vec<u8>> {
    use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
    use base64::Engine;
    const LENIENT: GeneralPurpose = GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
    );
    LENIENT.decode(payload).ok()
}

/// The pixels a transmission names. `data` is the decoded payload of all
/// its chunks.
pub fn load(command: &Command, data: &[u8]) -> Result<Image, String> {
    let (width, height) = (command.width, command.height);
    let channels = match command.format {
        24 => 3,
        32 => 4,
        _ => return Err("EINVAL:only RGB and RGBA pixels are supported".into()),
    };
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err("EINVAL:bad image size".into());
    }
    let expected = width as usize * height as usize * channels;
    if expected > MAX_IMAGE_BYTES {
        return Err("EFBIG:image too large".into());
    }
    let raw = match command.medium {
        b'd' => data.to_vec(),
        #[cfg(unix)]
        b's' => read_shared_memory(data, expected, command.zlib)?,
        _ => return Err("EINVAL:only inline and shared memory images are supported".into()),
    };
    let pixels = if command.zlib { inflate(&raw, expected)? } else { raw };
    if pixels.len() < expected {
        return Err("ENODATA:not enough pixel data".into());
    }
    let mut rgba = pixels;
    rgba.truncate(expected);
    if channels == 3 {
        rgba = rgba.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect();
    }
    Ok(Image { width, height, rgba })
}

/// Inflates zlib data, refusing more than `limit` bytes.
fn inflate(data: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut out = Vec::with_capacity(limit);
    flate2::read::ZlibDecoder::new(data)
        .take(limit as u64)
        .read_to_end(&mut out)
        .map_err(|error| format!("EINVAL:bad zlib data: {error}"))?;
    Ok(out)
}

/// Reads a POSIX shared memory segment and unlinks it, whether or not the
/// read works. Compressed data may be shorter than `expected`.
#[cfg(unix)]
fn read_shared_memory(name: &[u8], expected: usize, compressed: bool) -> Result<Vec<u8>, String> {
    use std::ffi::CString;
    // One leading slash and printable ASCII, as POSIX segment names are.
    let valid = name.first() == Some(&b'/')
        && name.len() <= 255
        && name[1..].iter().all(|&b| b.is_ascii_graphic() && b != b'/');
    if !valid {
        return Err("EINVAL:bad shared memory name".into());
    }
    let name = CString::new(name).map_err(|_| "EINVAL:bad shared memory name")?;
    // SAFETY: plain POSIX calls on a valid C string; the mapping is read
    // inside its length and unmapped before returning.
    unsafe {
        let fd = libc::shm_open(name.as_ptr(), libc::O_RDONLY, 0);
        if fd < 0 {
            return Err("ENOENT:no such shared memory".into());
        }
        let mut stat: libc::stat = std::mem::zeroed();
        let size = if libc::fstat(fd, &mut stat) == 0 { stat.st_size.max(0) as usize } else { 0 };
        let wanted = if compressed { size.min(MAX_IMAGE_BYTES) } else { expected };
        let result = if size < wanted || wanted == 0 {
            Err("ENODATA:shared memory is too small".to_owned())
        } else {
            let map = libc::mmap(std::ptr::null_mut(), wanted, libc::PROT_READ, libc::MAP_SHARED, fd, 0);
            if map == libc::MAP_FAILED {
                Err("EIO:cannot map shared memory".to_owned())
            } else {
                let data = std::slice::from_raw_parts(map.cast::<u8>(), wanted).to_vec();
                libc::munmap(map, wanted);
                Ok(data)
            }
        };
        libc::close(fd);
        libc::shm_unlink(name.as_ptr());
        result
    }
}

/// The reply a terminal sends for this command, if any.
pub fn reply(command: &Command, result: &Result<(), String>) -> Option<Vec<u8>> {
    if command.id == 0 && command.number == 0 {
        return None;
    }
    let quiet = if command.action == b'q' { 0 } else { command.quiet };
    let message = match result {
        Ok(()) if quiet == 0 => "OK",
        Err(error) if quiet < 2 => error.as_str(),
        _ => return None,
    };
    let mut keys = Vec::new();
    if command.id != 0 {
        keys.push(format!("i={}", command.id));
    }
    if command.number != 0 {
        keys.push(format!("I={}", command.number));
    }
    if command.placement != 0 {
        keys.push(format!("p={}", command.placement));
    }
    Some(format!("\x1b_G{};{message}\x1b\\", keys.join(",")).into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(keys: &str) -> Command {
        parse(keys.as_bytes()).0
    }

    #[test]
    fn keys_and_payload_are_read() {
        let (command, payload) = parse(b"a=T,f=24,t=s,o=z,s=2,v=3,i=5,I=6,p=7,x=1,y=2,w=3,h=4,X=8,Y=9,z=-2,q=2,m=1,d=I;QUJD");
        assert_eq!(payload, b"QUJD");
        assert_eq!(
            command,
            Command {
                action: b'T',
                format: 24,
                medium: b's',
                zlib: true,
                width: 2,
                height: 3,
                id: 5,
                number: 6,
                placement: 7,
                src: Rect { x: 1, y: 2, w: 3, h: 4 },
                dx: 8,
                dy: 9,
                z: -2,
                quiet: 2,
                more: true,
                delete: b'I',
            }
        );
        assert_eq!(parse(b"m=0").0, Command::default());
    }

    #[test]
    fn base64_chunks_decode_with_or_without_padding() {
        assert_eq!(decode_base64(b"QUJD").unwrap(), b"ABC");
        assert_eq!(decode_base64(b"QQ==").unwrap(), b"A");
        assert_eq!(decode_base64(b"QQ").unwrap(), b"A");
        assert!(decode_base64(b"!!!!").is_none());
    }

    #[test]
    fn inline_rgba_loads() {
        let image = load(&command("a=T,f=32,s=2,v=1"), &[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.rgba, [1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn zlib_rgba_loads() {
        use std::io::Write;
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&[9; 16]).unwrap();
        let image = load(&command("a=T,f=32,o=z,s=2,v=2"), &encoder.finish().unwrap()).unwrap();
        assert_eq!(image.rgba, [9; 16]);
    }

    #[test]
    fn rgb_gets_an_opaque_alpha() {
        let image = load(&command("a=T,f=24,s=1,v=2"), &[1, 2, 3, 4, 5, 6]).unwrap();
        assert_eq!(image.rgba, [1, 2, 3, 255, 4, 5, 6, 255]);
    }

    #[test]
    fn a_short_payload_or_a_bad_size_is_refused() {
        assert!(load(&command("a=T,f=32,s=2,v=2"), &[0; 8]).is_err());
        assert!(load(&command("a=T,f=32,s=0,v=2"), &[]).is_err());
        assert!(load(&command("a=T,f=32,s=20000,v=1"), &[0; 80000]).is_err());
        assert!(load(&command("a=T,f=100,s=1,v=1"), &[0; 4]).is_err(), "PNG is not taken");
        assert!(load(&command("a=T,f=32,t=f,s=1,v=1"), b"/etc/hosts").is_err(), "files are not read");
    }

    #[cfg(unix)]
    #[test]
    fn shared_memory_is_read_and_unlinked() {
        let name = format!("/wst-{}-a", std::process::id());
        let pixels: Vec<u8> = (0..16).collect();
        shm::create(&name, &pixels);
        let image = load(&command("a=T,f=32,t=s,s=2,v=2"), name.as_bytes()).unwrap();
        assert_eq!(image.rgba, pixels);
        assert!(!shm::exists(&name), "the segment is unlinked after reading");
    }

    #[cfg(unix)]
    #[test]
    fn a_missing_or_short_segment_fails_and_a_short_one_is_unlinked() {
        assert!(load(&command("a=T,f=32,t=s,s=1,v=1"), b"/wst-none").is_err());
        let name = format!("/wst-{}-b", std::process::id());
        // macOS rounds a segment up to whole pages, so ask for more than one.
        shm::create(&name, &[1, 2, 3]);
        assert!(load(&command("a=T,f=32,t=s,s=100,v=100"), name.as_bytes()).is_err());
        assert!(!shm::exists(&name));
        assert!(load(&command("a=T,f=32,t=s,s=1,v=1"), b"/a/b").is_err(), "names have one slash");
    }

    #[test]
    fn replies_follow_the_quiet_level() {
        let ok: Result<(), String> = Ok(());
        let failed: Result<(), String> = Err("ENOENT:gone".into());
        assert_eq!(reply(&command("a=q,i=31"), &ok).unwrap(), b"\x1b_Gi=31;OK\x1b\\");
        assert_eq!(reply(&command("a=T,i=31,p=2"), &failed).unwrap(), b"\x1b_Gi=31,p=2;ENOENT:gone\x1b\\");
        assert_eq!(reply(&command("a=T,i=31,q=1"), &ok), None);
        assert_eq!(reply(&command("a=T,i=31,q=1"), &failed).unwrap(), b"\x1b_Gi=31;ENOENT:gone\x1b\\");
        assert_eq!(reply(&command("a=T,i=31,q=2"), &failed), None);
        assert_eq!(reply(&command("a=T"), &ok), None, "no id, no reply");
        assert_eq!(reply(&command("a=T,I=4"), &ok).unwrap(), b"\x1b_GI=4;OK\x1b\\");
    }

    /// Test helpers for POSIX shared memory.
    #[cfg(unix)]
    mod shm {
        use std::ffi::CString;

        pub fn create(name: &str, data: &[u8]) {
            let name = CString::new(name).unwrap();
            unsafe {
                let fd = libc::shm_open(name.as_ptr(), libc::O_CREAT | libc::O_RDWR, 0o600);
                assert!(fd >= 0);
                assert_eq!(libc::ftruncate(fd, data.len() as libc::off_t), 0);
                let map = libc::mmap(std::ptr::null_mut(), data.len(), libc::PROT_WRITE, libc::MAP_SHARED, fd, 0);
                assert_ne!(map, libc::MAP_FAILED);
                std::ptr::copy_nonoverlapping(data.as_ptr(), map.cast::<u8>(), data.len());
                libc::munmap(map, data.len());
                libc::close(fd);
            }
        }

        pub fn exists(name: &str) -> bool {
            let name = CString::new(name).unwrap();
            unsafe {
                let fd = libc::shm_open(name.as_ptr(), libc::O_RDONLY, 0);
                if fd >= 0 {
                    libc::close(fd);
                }
                fd >= 0
            }
        }
    }
}
