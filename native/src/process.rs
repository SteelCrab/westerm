//! Command line of another process: sysctl KERN_PROCARGS2 on macOS, /proc on
//! Linux, the process's own record (PEB) on Windows. Only processes of the
//! same user can be read, which is all the host needs: the programs running
//! in its own terminals.
//!
//! Windows also lists the processes below a shell here: ConPTY has no
//! foreground process group to ask.

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

#[cfg(windows)]
pub fn process_args(pid: u32) -> Option<Vec<String>> {
    let line = windows::command_line(pid)?;
    (!line.trim().is_empty()).then(|| split_command_line(&line))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub fn process_args(_pid: u32) -> Option<Vec<String>> {
    None
}

/// The processes below `root` that started after it, nearest first. Windows
/// only; see `descendants`.
#[cfg(windows)]
pub fn below(root: u32) -> Vec<u32> {
    descendants(root, &windows::processes(), windows::created)
}

/// A Windows command line as the C runtime of the program splits it. The
/// program name: quotes group and a backslash is plain. The arguments: 2n
/// backslashes before a quote are n backslashes and the quote groups; 2n+1 are
/// n backslashes and a plain quote; two quotes inside a group are one quote.
#[cfg(any(windows, test))]
fn split_command_line(line: &str) -> Vec<String> {
    let mut chars = line.chars().peekable();
    let mut program = String::new();
    let mut quoted = false;
    for c in chars.by_ref() {
        match c {
            '"' => quoted = !quoted,
            ' ' | '\t' if !quoted => break,
            _ => program.push(c),
        }
    }
    let mut args = vec![program];
    let mut word = String::new();
    // An empty pair of quotes is an argument too, so text alone cannot tell.
    let mut started = false;
    let mut quoted = false;
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' if !quoted => {
                if started {
                    args.push(std::mem::take(&mut word));
                    started = false;
                }
                continue;
            }
            '\\' => {
                let mut slashes = 1;
                while chars.next_if_eq(&'\\').is_some() {
                    slashes += 1;
                }
                if chars.peek() == Some(&'"') {
                    word.extend(std::iter::repeat_n('\\', slashes / 2));
                    if slashes % 2 == 1 {
                        chars.next();
                        word.push('"');
                    }
                } else {
                    word.extend(std::iter::repeat_n('\\', slashes));
                }
            }
            '"' if quoted && chars.peek() == Some(&'"') => {
                chars.next();
                word.push('"');
            }
            '"' => quoted = !quoted,
            _ => word.push(c),
        }
        started = true;
    }
    if started {
        args.push(word);
    }
    args
}

/// A folder as a process records it, without the `\` that ends it. A drive's
/// root keeps it: `C:` alone is another folder.
#[cfg(any(windows, test))]
fn folder_path(text: &str) -> &str {
    let trimmed = text.trim_end_matches('\\');
    if trimmed.is_empty() || trimmed.ends_with(':') { text } else { trimmed }
}

/// The processes looked at below one shell. An agent runs next to the shell;
/// the limit keeps a large build in the shell from costing a read per
/// process every second.
#[cfg(any(windows, test))]
const NEAREST: usize = 64;

/// The processes below `root`, nearest first: its children, then theirs.
/// `processes` pairs every process with its parent, and `created` tells when
/// a process started. Windows gives the id of an ended process to a new one,
/// so a parent id may name a process that is not the parent; a real child is
/// never older than its parent.
#[cfg(any(windows, test))]
fn descendants(root: u32, processes: &[(u32, u32)], mut created: impl FnMut(u32) -> Option<u64>) -> Vec<u32> {
    let Some(start) = created(root) else { return Vec::new() };
    let mut found = vec![(root, start)];
    let mut next = 0;
    while let Some(&(parent, parent_created)) = found.get(next) {
        next += 1;
        for &(pid, of) in processes {
            if found.len() > NEAREST {
                break;
            }
            if of != parent || found.iter().any(|&(seen, _)| seen == pid) {
                continue;
            }
            if let Some(time) = created(pid).filter(|&time| time >= parent_created) {
                found.push((pid, time));
            }
        }
    }
    found.into_iter().skip(1).map(|(pid, _)| pid).collect()
}

/// Reads of other processes through Win32. A process that is gone, or that
/// this user may not read, gives None.
#[cfg(windows)]
pub(crate) mod windows {
    use std::ffi::c_void;
    use std::path::PathBuf;

    use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation};
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };

    const POINTER: usize = std::mem::size_of::<usize>();

    /// Where the PEB points to the process parameters, and where those keep
    /// the current folder and the command line. The layout is not in the
    /// Windows headers, but it has been the same since Windows XP, and tools
    /// such as Process Explorer read it this way.
    #[cfg(target_pointer_width = "64")]
    const OFFSETS: (usize, usize, usize) = (0x20, 0x38, 0x70);
    #[cfg(target_pointer_width = "32")]
    const OFFSETS: (usize, usize, usize) = (0x10, 0x24, 0x40);

    /// PROCESS_BASIC_INFORMATION.
    #[repr(C)]
    struct BasicInformation {
        exit_status: i32,
        peb: usize,
        affinity: usize,
        priority: i32,
        pid: usize,
        parent: usize,
    }

    struct Process(HANDLE);

    impl Process {
        fn open(pid: u32, access: u32) -> Option<Self> {
            // SAFETY: OpenProcess takes plain values and returns null when it fails.
            let handle = unsafe { OpenProcess(access, 0, pid) };
            (!handle.is_null()).then_some(Self(handle))
        }

        fn read(&self, address: usize, len: usize) -> Option<Vec<u8>> {
            let mut bytes = vec![0u8; len];
            let mut done = 0usize;
            // SAFETY: ReadProcessMemory writes at most `len` bytes into `bytes`.
            let ok = unsafe {
                ReadProcessMemory(self.0, address as *const c_void, bytes.as_mut_ptr() as *mut c_void, len, &mut done)
            };
            (ok != 0 && done == len).then_some(bytes)
        }

        fn read_pointer(&self, address: usize) -> Option<usize> {
            let bytes = self.read(address, POINTER)?;
            Some(usize::from_ne_bytes(bytes.try_into().ok()?))
        }

        /// A UNICODE_STRING: its length in bytes, then a pointer to UTF-16 text.
        fn read_text(&self, address: usize) -> Option<String> {
            let header = self.read(address, 2 * POINTER)?;
            let len = usize::from(u16::from_ne_bytes([header[0], header[1]]));
            let buffer = usize::from_ne_bytes(header[POINTER..].try_into().ok()?);
            let bytes = self.read(buffer, len)?;
            let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|&pair| u16::from_ne_bytes(pair)).collect();
            Some(String::from_utf16_lossy(&units))
        }

        fn parameters(&self) -> Option<usize> {
            // SAFETY: every field is a plain number, so zero is a value.
            let mut info: BasicInformation = unsafe { std::mem::zeroed() };
            // SAFETY: the call writes at most the given size into `info`.
            let status = unsafe {
                NtQueryInformationProcess(
                    self.0,
                    ProcessBasicInformation,
                    &mut info as *mut _ as *mut c_void,
                    std::mem::size_of::<BasicInformation>() as u32,
                    std::ptr::null_mut(),
                )
            };
            if status < 0 || info.peb == 0 {
                return None;
            }
            self.read_pointer(info.peb.checked_add(OFFSETS.0)?).filter(|&parameters| parameters != 0)
        }
    }

    impl Drop for Process {
        fn drop(&mut self) {
            // SAFETY: the handle came from OpenProcess and closes once.
            unsafe { CloseHandle(self.0) };
        }
    }

    fn parameter(pid: u32, offset: usize) -> Option<String> {
        let process = Process::open(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ)?;
        process.read_text(process.parameters()?.checked_add(offset)?)
    }

    pub fn command_line(pid: u32) -> Option<String> {
        parameter(pid, OFFSETS.2)
    }

    pub fn current_directory(pid: u32) -> Option<PathBuf> {
        let text = parameter(pid, OFFSETS.1)?;
        (!text.is_empty()).then(|| PathBuf::from(super::folder_path(&text)))
    }

    /// Every process with its parent's id.
    pub fn processes() -> Vec<(u32, u32)> {
        // SAFETY: the call takes plain values.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Vec::new();
        }
        // SAFETY: every field is a plain number, so zero is a value.
        let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut processes = Vec::new();
        // SAFETY: `entry` has its size set and lives through the calls; the
        // snapshot handle is open until CloseHandle.
        unsafe {
            let mut more = Process32FirstW(snapshot, &mut entry);
            while more != 0 {
                processes.push((entry.th32ProcessID, entry.th32ParentProcessID));
                more = Process32NextW(snapshot, &mut entry);
            }
            CloseHandle(snapshot);
        }
        processes
    }

    /// When a process started, in FILETIME ticks.
    pub fn created(pid: u32) -> Option<u64> {
        let process = Process::open(pid, PROCESS_QUERY_LIMITED_INFORMATION)?;
        let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
        // SAFETY: the call writes the four times.
        let ok = unsafe { GetProcessTimes(process.0, &mut created, &mut exited, &mut kernel, &mut user) };
        (ok != 0).then(|| u64::from(created.dwHighDateTime) << 32 | u64::from(created.dwLowDateTime))
    }
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

    #[cfg(windows)]
    #[test]
    fn reads_this_process_arguments() {
        let args = super::process_args(std::process::id()).unwrap();
        let ours: Vec<String> = std::env::args().collect();
        assert_eq!(args[1..], ours[1..]);
        assert!(!args[0].is_empty());
        assert_eq!(super::process_args(0), None);
    }

    #[cfg(windows)]
    #[test]
    fn finds_and_reads_a_child_process() {
        let dir = tempfile::tempdir().unwrap();
        // ping.exe is on every Windows; it runs until the test ends it.
        let mut child = std::process::Command::new("ping.exe")
            .args(["-n", "30", "127.0.0.1"])
            .current_dir(dir.path())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let below = super::below(std::process::id());
        let args = super::process_args(child.id());
        let cwd = crate::cwd::process_cwd(child.id());
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(below.contains(&child.id()), "{below:?} lacks {}", child.id());
        let args = args.unwrap();
        assert!(args[0].to_lowercase().ends_with("ping.exe"), "{args:?}");
        assert_eq!(args[1..], ["-n", "30", "127.0.0.1"]);
        // The same folder, whatever the letter case or short names.
        assert_eq!(cwd.unwrap().canonicalize().unwrap(), dir.path().canonicalize().unwrap());
    }

    #[test]
    fn splits_windows_command_lines() {
        let split = super::split_command_line;
        assert_eq!(
            split(r#""C:\Program Files\nodejs\node.exe" C:\npm\node_modules\@openai\codex\bin\codex.js --model gpt-5"#),
            [r"C:\Program Files\nodejs\node.exe", r"C:\npm\node_modules\@openai\codex\bin\codex.js", "--model", "gpt-5"]
        );
        assert_eq!(split(r"C:\Users\x\.local\bin\claude.exe"), [r"C:\Users\x\.local\bin\claude.exe"]);
        // A backslash before the closing quote makes that quote plain text.
        assert_eq!(split(r#"claude  --add-dir "C:\my work\" -c"#), ["claude", "--add-dir", r#"C:\my work" -c"#]);
        assert_eq!(split(r#"claude --add-dir "C:\my work\\" -c"#), ["claude", "--add-dir", r"C:\my work\", "-c"]);
        assert_eq!(split(r#"claude --system-prompt "say \"hi\"" """#), ["claude", "--system-prompt", r#"say "hi""#, ""]);
        assert_eq!(split(r#"claude "a""b" c\\d"#), ["claude", r#"a"b"#, r"c\\d"]);
        assert_eq!(split("claude "), ["claude"]);
    }

    #[test]
    fn folders_lose_the_backslash_that_ends_them() {
        assert_eq!(super::folder_path(r"C:\Users\x\"), r"C:\Users\x");
        assert_eq!(super::folder_path(r"C:\Users\x"), r"C:\Users\x");
        assert_eq!(super::folder_path(r"C:\"), r"C:\");
        assert_eq!(super::folder_path(r"\\server\share\"), r"\\server\share");
    }

    #[test]
    fn lists_the_processes_below_a_shell_nearest_first() {
        // 10 is the shell. 11 and 12 are its children, 13 is below 12.
        let processes = [(0, 0), (4, 0), (13, 12), (11, 10), (12, 10), (20, 4), (10, 4)];
        let created = |pid: u32| Some(u64::from(pid) * 100);
        assert_eq!(super::descendants(10, &processes, created), [11, 12, 13]);
        assert_eq!(super::descendants(13, &processes, created), [0u32; 0]);
        assert_eq!(super::descendants(10, &processes, |_| None), [0u32; 0], "the shell is gone");
    }

    #[test]
    fn a_reused_parent_id_does_not_adopt_older_processes() {
        // 30 kept its parent id 10 from a process that ended before this shell took the id.
        let processes = [(30, 10), (31, 10), (32, 30), (33, 31)];
        let created = |pid: u32| match pid {
            10 => Some(500),
            30 => Some(100),
            31 => Some(600),
            32 => Some(700),
            33 => Some(650),
            _ => None,
        };
        assert_eq!(super::descendants(10, &processes, created), [31, 33]);
    }

    #[test]
    fn looks_only_at_the_nearest_processes() {
        let processes: Vec<(u32, u32)> = (2..500).map(|pid| (pid, 1)).collect();
        assert_eq!(super::descendants(1, &processes, |_| Some(0)).len(), super::NEAREST);
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
