//! What a remote window is told about its link: the protocol's stage, plus
//! how a connected link got through.

use crate::protocol::{RemoteStage, ServerMessage};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuicPath {
    Punched,
}

/// A stage as a window sees it: the stage, plus how a connected link got through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkStage {
    pub stage: RemoteStage,
    pub path: Option<QuicPath>,
    /// Why the SSH tunnel carries the link.
    pub note: Option<String>,
}

impl From<RemoteStage> for LinkStage {
    fn from(stage: RemoteStage) -> Self {
        Self { stage, path: None, note: None }
    }
}

impl LinkStage {
    pub fn to_json(&self) -> String {
        let plain = ServerMessage::Remote(self.stage.clone()).to_json();
        if self.path.is_none() && self.note.is_none() {
            return plain;
        }
        // protocol.rs is host code, so the extra fields go on its text, after
        // its own keys (a serde_json Value would sort them).
        let Some(head) = plain.strip_suffix('}') else { return plain };
        let mut json = head.to_owned();
        if let Some(QuicPath::Punched) = self.path {
            json.push_str(r#","path":"punched""#);
        }
        if let Some(note) = &self.note {
            json.push_str(&format!(r#","note":{}"#, serde_json::Value::from(note.as_str())));
        }
        json.push('}');
        json
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::RemoteVia;

    #[test]
    fn a_plain_stage_is_sent_as_before() {
        let connected = RemoteStage::Connected { via: RemoteVia::Quic, kill_on_logout: false };
        assert_eq!(LinkStage::from(connected.clone()).to_json(), ServerMessage::Remote(connected.clone()).to_json());
        assert_eq!(LinkStage::from(RemoteStage::Checking).to_json(), r#"{"type":"remote","stage":"checking"}"#);
    }

    #[test]
    fn a_connected_stage_can_say_how_it_got_through() {
        let quic = RemoteStage::Connected { via: RemoteVia::Quic, kill_on_logout: false };
        let punched = LinkStage { stage: quic, path: Some(QuicPath::Punched), note: None };
        assert_eq!(punched.to_json(), r#"{"type":"remote","stage":"connected","via":"quic","killOnLogout":false,"path":"punched"}"#);
        let tunnel = RemoteStage::Connected { via: RemoteVia::Tunnel, kill_on_logout: true };
        let noted = LinkStage { stage: tunnel, path: None, note: Some("QUIC: no answer".into()) };
        assert_eq!(noted.to_json(), r#"{"type":"remote","stage":"connected","via":"tunnel","killOnLogout":true,"note":"QUIC: no answer"}"#);
    }
}
