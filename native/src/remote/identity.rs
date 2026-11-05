//! A self-signed certificate and its key, made once and kept in a private
//! folder. Peers trust each other by the certificate's SHA-256 fingerprint,
//! which they exchange over SSH; there is no certificate authority.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use sha2::{Digest, Sha256};

use crate::state_dir::private_file;

pub struct Identity {
    pub cert: CertificateDer<'static>,
    pub key: PrivatePkcs8KeyDer<'static>,
}

/// `<dir>/<name>.crt` and `<dir>/<name>.key` (DER, 0600). A missing or
/// damaged pair is made again, which changes the fingerprint; the peer then
/// learns the new one through the next SSH setup.
pub fn load_or_create(dir: &Path, name: &str) -> io::Result<Identity> {
    let cert_path = dir.join(format!("{name}.crt"));
    let key_path = dir.join(format!("{name}.key"));
    if let (Ok(cert), Ok(key)) = (fs::read(&cert_path), fs::read(&key_path)) {
        let identity = Identity { cert: CertificateDer::from(cert), key: PrivatePkcs8KeyDer::from(key) };
        if usable(&identity) {
            return Ok(identity);
        }
    }
    let identity = generate()?;
    write_private(&key_path, identity.key.secret_pkcs8_der())?;
    write_private(&cert_path, &identity.cert)?;
    Ok(identity)
}

fn usable(identity: &Identity) -> bool {
    !identity.cert.is_empty()
        && rustls::crypto::ring::sign::any_supported_type(&PrivateKeyDer::Pkcs8(identity.key.clone_key())).is_ok()
}

/// ECDSA P-256, self-signed, for the name "westerm" (peers check the
/// fingerprint, not the name).
fn generate() -> io::Result<Identity> {
    let key = rcgen::KeyPair::generate().map_err(io::Error::other)?;
    let params = rcgen::CertificateParams::new(vec!["westerm".to_owned()]).map_err(io::Error::other)?;
    let cert = params.self_signed(&key).map_err(io::Error::other)?;
    Ok(Identity { cert: cert.der().clone(), key: PrivatePkcs8KeyDer::from(key.serialize_der()) })
}

/// Writes through a temporary file, so a reader never sees half a key.
fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = PathBuf::from(format!("{}.tmp", path.display()));
    let _ = fs::remove_file(&temp);
    let mut file = private_file().create_new(true).write(true).open(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temp, path)
}

/// SHA-256 of a DER certificate, as 64 lowercase hex digits.
pub fn fingerprint(cert_der: &[u8]) -> String {
    Sha256::digest(cert_der).iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn is_fingerprint(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_once_and_reloads_the_same_identity() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_or_create(dir.path(), "client").unwrap();
        let second = load_or_create(dir.path(), "client").unwrap();
        assert_eq!(fingerprint(&first.cert), fingerprint(&second.cert));
        assert_eq!(first.key.secret_pkcs8_der(), second.key.secret_pkcs8_der());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            for file in ["client.key", "client.crt"] {
                let mode = std::fs::metadata(dir.path().join(file)).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o600, "{file}");
            }
        }
    }

    #[test]
    fn fingerprints_are_64_lowercase_hex() {
        let dir = tempfile::tempdir().unwrap();
        let id = load_or_create(dir.path(), "c").unwrap();
        assert!(is_fingerprint(&fingerprint(&id.cert)));
        assert!(!is_fingerprint(&"A".repeat(64)));
        assert!(!is_fingerprint(&"a".repeat(63)));
        assert!(!is_fingerprint(&"g".repeat(64)));
    }

    #[test]
    fn a_damaged_identity_is_made_again() {
        let dir = tempfile::tempdir().unwrap();
        let first = load_or_create(dir.path(), "gate").unwrap();
        std::fs::write(dir.path().join("gate.key"), b"not a key").unwrap();
        let second = load_or_create(dir.path(), "gate").unwrap();
        assert_ne!(fingerprint(&first.cert), fingerprint(&second.cert));
        assert_eq!(fingerprint(&load_or_create(dir.path(), "gate").unwrap().cert), fingerprint(&second.cert));
    }
}
