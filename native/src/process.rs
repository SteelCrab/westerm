//! Command line of another process: sysctl KERN_PROCARGS2 on macOS, /proc on
//! Linux. Only processes of the same user can be read, which is all the host
//! needs: the programs running in its own terminals.

#[cfg(target_os = "macos")]
pub fn process_args(pid: u32) -> Option<Vec<String>> {
    use std::os::raw::{c_int, c_void};

    let mut argmax: c_int = 0;
    let mut size = std::mem::size_of::<c_int>();
    let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    // SAFETY: sysctl writes at most `size` bytes into `argmax`.
    let ok = unsafe {
        libc::sysctl(mib.as_mut_ptr(), 2, &mut argmax as *mut _ as *mut c_void, &mut size, std::ptr::null_mut(), 0)
    };
    if ok != 0 || argmax <= 0 {
        return None;
    }
    let mut buffer = vec![0u8; argmax as usize];
    let mut size = buffer.len();
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid as c_int];
    // SAFETY: sysctl writes at most `size` bytes into the buffer and updates `size`.
    let ok = unsafe {
        libc::sysctl(mib.as_mut_ptr(), 3, buffer.as_mut_ptr() as *mut c_void, &mut size, std::ptr::null_mut(), 0)
    };
    if ok != 0 {
        return None;
    }
    buffer.truncate(size);
    parse_procargs(&buffer)
}

/// KERN_PROCARGS2: argc, the executable path, NUL padding, then argc
/// NUL-terminated arguments (the environment follows).
#[cfg(any(target_os = "macos", test))]
fn parse_procargs(buffer: &[u8]) -> Option<Vec<String>> {
    let count = u32::from_ne_bytes(buffer.get(..4)?.try_into().ok()?) as usize;
    let rest = &buffer[4..];
    let path_end = rest.iter().position(|&b| b == 0)?;
    let mut words = rest[path_end..].split(|&b| b == 0).skip_while(|word| word.is_empty());
    let args: Vec<String> = words.by_ref().take(count).map(|word| String::from_utf8_lossy(word).into_owned()).collect();
    (args.len() == count && count > 0).then_some(args)
}

#[cfg(target_os = "linux")]
pub fn process_args(pid: u32) -> Option<Vec<String>> {
    let bytes = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let args: Vec<String> = bytes
        .strip_suffix(&[0])
        .unwrap_or(&bytes)
        .split(|&b| b == 0)
        .map(|word| String::from_utf8_lossy(word).into_owned())
        .collect();
    (!bytes.is_empty()).then_some(args)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn process_args(_pid: u32) -> Option<Vec<String>> {
    None
}

#[cfg(test)]
mod tests {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn reads_this_process_arguments() {
        let args = super::process_args(std::process::id()).unwrap();
        let ours: Vec<String> = std::env::args().collect();
        assert_eq!(args, ours);
        assert_eq!(super::process_args(4_000_000), None);
    }

    #[test]
    fn parses_procargs_buffers() {
        let mut buffer = 2u32.to_ne_bytes().to_vec();
        buffer.extend_from_slice(b"/usr/local/bin/claude\0\0\0\0claude\0--model\0HOME=/x\0");
        assert_eq!(super::parse_procargs(&buffer), Some(vec!["claude".into(), "--model".into()]));
        assert_eq!(super::parse_procargs(&3u32.to_ne_bytes()), None);
        assert_eq!(super::parse_procargs(b"\x01"), None);
    }
}
