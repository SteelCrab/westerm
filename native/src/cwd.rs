//! Current directory of a process: proc_pidinfo on macOS, /proc on Linux,
//! the process's own record (PEB) on Windows.
//!
//! PowerShell keeps its location to itself: `cd` does not move the process.
//! So a PowerShell shell reads as the folder it started in. A program it
//! starts, such as an AI agent, does start in its location.

use std::path::PathBuf;

#[cfg(target_os = "macos")]
pub fn process_cwd(pid: u32) -> Option<PathBuf> {
    use std::ffi::CStr;
    use std::os::raw::{c_char, c_int, c_void};

    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as c_int;
    // SAFETY: proc_pidinfo writes at most `size` bytes into `info`.
    let written = unsafe {
        libc::proc_pidinfo(
            pid as c_int,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            &mut info as *mut _ as *mut c_void,
            size,
        )
    };
    if written != size {
        return None;
    }
    // vip_path is a NUL-terminated MAXPATHLEN buffer stored as [[c_char; 32]; 32].
    let path = &info.pvi_cdir.vip_path;
    let bytes: &[c_char] = unsafe { std::slice::from_raw_parts(path.as_ptr() as *const c_char, 32 * 32) };
    if !bytes.contains(&0) {
        return None;
    }
    let text = unsafe { CStr::from_ptr(bytes.as_ptr()) }.to_str().ok()?;
    (!text.is_empty()).then(|| PathBuf::from(text))
}

#[cfg(target_os = "linux")]
pub fn process_cwd(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

#[cfg(windows)]
pub fn process_cwd(pid: u32) -> Option<PathBuf> {
    crate::process::windows::current_directory(pid)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub fn process_cwd(_pid: u32) -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    #[test]
    fn reads_this_process_directory() {
        let here = std::env::current_dir().unwrap();
        assert_eq!(super::process_cwd(std::process::id()).as_deref(), Some(here.as_path()));
        assert_eq!(super::process_cwd(4_000_000), None);
    }
}
