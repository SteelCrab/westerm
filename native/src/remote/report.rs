//! What `westerm remote-setup` tells the dashboard: one JSON line, the last
//! one on stdout (starting a host prints a line of its own before it).

use serde::{Deserialize, Serialize};

pub const SETUP_VERSION: u32 = 2;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SetupReport {
    pub v: u32,
    /// The server's `westerm --version` line, without the tagline.
    pub version: String,
    /// The gate's UDP port.
    pub port: u16,
    /// The gate certificate's fingerprint.
    pub fingerprint: String,
    /// The server dashboard's Unix socket on the server, for an SSH tunnel.
    pub dashboard: String,
    /// The host running after setup.
    pub host: HostInfo,
    /// The server ends a user's processes at logout and lingering is off.
    #[serde(rename = "killOnLogout")]
    pub kill_on_logout: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HostInfo {
    pub build: String,
    pub protocol: u32,
    /// Live sessions: what a replacement would end.
    pub sessions: usize,
}

/// The report on the last non-empty line, if it is one of this version.
pub fn parse_report(stdout: &str) -> Option<SetupReport> {
    let line = stdout.lines().rev().map(str::trim).find(|line| !line.is_empty())?;
    serde_json::from_str::<SetupReport>(line).ok().filter(|report| report.v == SETUP_VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SetupReport {
        SetupReport {
            v: SETUP_VERSION,
            version: "westerm 0.2.0 (host rs-1)".into(),
            port: 4317,
            fingerprint: "ab".repeat(32),
            dashboard: "/s/dashboard.sock".into(),
            host: HostInfo { build: "rs-1".into(), protocol: 1, sessions: 2 },
            kill_on_logout: false,
        }
    }

    #[test]
    fn reads_the_last_line_as_the_report() {
        let json = serde_json::to_string(&sample()).unwrap();
        assert!(json.contains("\"killOnLogout\":false"));
        assert_eq!(parse_report(&format!("> pty host rs-1 started (pid 7)\n{json}\n\n")), Some(sample()));
        assert_eq!(parse_report(&json.replace("\"v\":2", "\"v\":3")), None, "another report version");
        assert_eq!(parse_report("garbage"), None);
        assert_eq!(parse_report(""), None);
    }

    #[test]
    fn a_report_without_the_dashboard_is_refused() {
        let v1 = r#"{"v":1,"version":"westerm 0.2.0 (host rs-1)","port":4317,"fingerprint":"ab","socket":"/s/host.sock","host":{"build":"rs-1","protocol":1,"sessions":0},"killOnLogout":false}"#;
        assert_eq!(parse_report(v1), None);
    }
}
