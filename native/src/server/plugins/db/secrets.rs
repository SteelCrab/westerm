//! Passwords: the OS keychain (service `westerm-db`, account = connection id),
//! with server memory as the fallback when the keychain cannot be used now.
//! Every keychain call runs on its own thread with a time limit, so a
//! keychain prompt never stalls a request.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The keychain service name.
#[cfg_attr(test, allow(dead_code))]
pub const SERVICE: &str = "westerm-db";
/// How long one keychain call may take.
pub const KEYCHAIN_TIMEOUT: Duration = Duration::from_secs(5);

/// A place that keeps passwords by connection id.
pub trait Keychain: Send + Sync + 'static {
    fn get(&self, id: &str) -> Result<Option<String>, String>;
    fn set(&self, id: &str, password: &str) -> Result<(), String>;
    fn delete(&self, id: &str) -> Result<(), String>;
    /// Whether it can be used, without reading an entry (no prompt).
    fn probe(&self) -> bool;
}

/// The OS keychain.
#[cfg_attr(test, allow(dead_code))]
pub struct SystemKeychain;

/// A keychain in memory, for tests: it can fail or hang on purpose.
#[cfg(test)]
#[derive(Default)]
pub struct FakeKeychain {
    pub entries: Mutex<HashMap<String, String>>,
    pub fail: AtomicBool,
    pub hang: AtomicBool,
    /// How many get, set, and delete calls reached it.
    pub calls: std::sync::atomic::AtomicUsize,
}

/// The first line of an error, for the UI.
fn first_line(error: impl std::fmt::Display) -> String {
    error.to_string().lines().next().unwrap_or_default().to_owned()
}

impl Keychain for SystemKeychain {
    fn get(&self, id: &str) -> Result<Option<String>, String> {
        match keyring::Entry::new(SERVICE, id).and_then(|entry| entry.get_password()) {
            Ok(password) => Ok(Some(password)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(first_line(error)),
        }
    }

    fn set(&self, id: &str, password: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, id).and_then(|entry| entry.set_password(password)).map_err(first_line)
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        match keyring::Entry::new(SERVICE, id).and_then(|entry| entry.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(first_line(error)),
        }
    }

    /// Looks up an account that is never written: "no entry" means the
    /// keychain answers, and a missing item asks the user nothing.
    fn probe(&self) -> bool {
        matches!(
            keyring::Entry::new(SERVICE, "westerm-probe").and_then(|entry| entry.get_password()),
            Ok(_) | Err(keyring::Error::NoEntry)
        )
    }
}

#[cfg(test)]
impl FakeKeychain {
    fn check(&self) -> Result<(), String> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.hang.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(10));
        }
        if self.fail.load(Ordering::Relaxed) {
            return Err("The keychain is locked".to_owned());
        }
        Ok(())
    }
}

#[cfg(test)]
impl Keychain for FakeKeychain {
    fn get(&self, id: &str) -> Result<Option<String>, String> {
        self.check()?;
        Ok(self.entries.lock().map_err(first_line)?.get(id).cloned())
    }

    fn set(&self, id: &str, password: &str) -> Result<(), String> {
        self.check()?;
        self.entries.lock().map_err(first_line)?.insert(id.to_owned(), password.to_owned());
        Ok(())
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        self.check()?;
        self.entries.lock().map_err(first_line)?.remove(id);
        Ok(())
    }

    fn probe(&self) -> bool {
        if self.hang.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_secs(10));
        }
        !self.fail.load(Ordering::Relaxed)
    }
}

/// The keychain the server uses. Test builds never touch the real one.
pub fn system_keychain() -> Arc<dyn Keychain> {
    #[cfg(test)]
    {
        Arc::new(FakeKeychain::default())
    }
    #[cfg(not(test))]
    {
        Arc::new(SystemKeychain)
    }
}

/// Passwords by connection id: the keychain first, then memory.
pub struct Secrets {
    keychain: Arc<dyn Keychain>,
    timeout: Duration,
    /// Passwords the keychain could not take, and ones read from it (so the
    /// keychain is asked once per connection and server run).
    memory: Mutex<HashMap<String, String>>,
    /// Ids the keychain said it has no password for.
    absent: Mutex<HashSet<String>>,
    /// The last keychain call worked.
    available: AtomicBool,
    /// Some keychain call has run (a probe or a real one).
    probed: AtomicBool,
    /// A keychain call failed: reads stop asking (no prompt on every poll)
    /// until an explicit save works again.
    failed: AtomicBool,
}

impl Default for Secrets {
    fn default() -> Self {
        Self::new(system_keychain(), KEYCHAIN_TIMEOUT)
    }
}

impl Secrets {
    pub fn new(keychain: Arc<dyn Keychain>, timeout: Duration) -> Self {
        Self {
            keychain,
            timeout,
            memory: Mutex::default(),
            absent: Mutex::default(),
            available: AtomicBool::new(false),
            probed: AtomicBool::new(false),
            failed: AtomicBool::new(false),
        }
    }

    /// Runs `call` on its own thread and waits without holding the async
    /// runtime; an answer later than the time limit is an error (the thread
    /// is left to finish on its own).
    async fn call<T: Send + 'static>(&self, call: impl FnOnce(&dyn Keychain) -> Result<T, String> + Send + 'static) -> Result<T, String> {
        let keychain = Arc::clone(&self.keychain);
        let (tx, rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(call(keychain.as_ref()));
        });
        let result = match tokio::time::timeout(self.timeout, rx).await {
            Ok(Ok(result)) => result,
            _ => Err("The keychain did not answer in time".to_owned()),
        };
        self.probed.store(true, Ordering::Relaxed);
        self.available.store(result.is_ok(), Ordering::Relaxed);
        self.failed.store(result.is_err(), Ordering::Relaxed);
        result
    }

    fn memory(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.memory.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn absent(&self) -> std::sync::MutexGuard<'_, HashSet<String>> {
        self.absent.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The password of `id`: the one in memory, else the keychain's (not
    /// asked again after a failure).
    pub async fn password(&self, id: &str) -> Option<String> {
        if let Some(password) = self.memory().get(id) {
            return Some(password.clone());
        }
        if self.absent().contains(id) || self.failed.load(Ordering::Relaxed) {
            return None;
        }
        let owned = id.to_owned();
        match self.call(move |keychain| keychain.get(&owned)).await {
            Ok(Some(password)) => {
                self.memory().insert(id.to_owned(), password.clone());
                Some(password)
            }
            Ok(None) => {
                self.absent().insert(id.to_owned());
                None
            }
            Err(_) => None,
        }
    }

    /// Saves the password in the keychain. When the keychain cannot take it,
    /// it is kept in memory and the keychain's first error line comes back.
    pub async fn save(&self, id: &str, password: &str) -> Result<(), String> {
        self.absent().remove(id);
        self.memory().insert(id.to_owned(), password.to_owned());
        let (owned, secret) = (id.to_owned(), password.to_owned());
        self.call(move |keychain| keychain.set(&owned, &secret)).await
    }

    /// Forgets the password everywhere.
    pub async fn forget(&self, id: &str) {
        self.memory().remove(id);
        self.absent().insert(id.to_owned());
        let owned = id.to_owned();
        let _ = self.call(move |keychain| keychain.delete(&owned)).await;
    }

    /// Whether a password is known for `id` (memory or keychain).
    #[cfg(test)]
    pub async fn saved(&self, id: &str) -> bool {
        self.password(id).await.is_some()
    }

    /// Whether the keychain worked the last time it was used (probed once, without a prompt).
    pub async fn available(&self) -> bool {
        if !self.probed.load(Ordering::Relaxed) {
            let _ = self.call(|keychain| if keychain.probe() { Ok(()) } else { Err(String::new()) }).await;
        }
        self.available.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secrets(fake: FakeKeychain) -> (Secrets, Arc<FakeKeychain>) {
        let fake = Arc::new(fake);
        (Secrets::new(fake.clone(), Duration::from_millis(200)), fake)
    }

    #[tokio::test]
    async fn passwords_go_to_the_keychain() {
        let (secrets, fake) = secrets(FakeKeychain::default());
        secrets.save("aaaaaaaaaaaaaaaa", "pw").await.unwrap();
        assert_eq!(fake.entries.lock().unwrap().get("aaaaaaaaaaaaaaaa").map(String::as_str), Some("pw"));
        assert_eq!(secrets.password("aaaaaaaaaaaaaaaa").await.as_deref(), Some("pw"));
        assert!(secrets.available().await);
        secrets.forget("aaaaaaaaaaaaaaaa").await;
        assert!(fake.entries.lock().unwrap().is_empty());
        assert!(!secrets.saved("aaaaaaaaaaaaaaaa").await);
    }

    #[tokio::test]
    async fn a_failing_keychain_keeps_the_password_in_memory() {
        let (secrets, fake) = secrets(FakeKeychain::default());
        fake.fail.store(true, Ordering::Relaxed);
        assert!(secrets.save("bbbbbbbbbbbbbbbb", "pw").await.is_err());
        assert_eq!(secrets.password("bbbbbbbbbbbbbbbb").await.as_deref(), Some("pw"));
        assert!(!secrets.available().await);
        secrets.forget("bbbbbbbbbbbbbbbb").await;
        assert_eq!(secrets.password("bbbbbbbbbbbbbbbb").await, None);
    }

    #[tokio::test]
    async fn a_hung_keychain_times_out_and_falls_back() {
        let (secrets, fake) = secrets(FakeKeychain::default());
        fake.hang.store(true, Ordering::Relaxed);
        let started = std::time::Instant::now();
        assert!(secrets.save("cccccccccccccccc", "pw").await.is_err());
        assert!(started.elapsed() < Duration::from_secs(2), "the time limit holds");
        assert_eq!(secrets.password("cccccccccccccccc").await.as_deref(), Some("pw"));
        assert!(!secrets.available().await);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_hung_keychain_does_not_block_the_runtime() {
        let (secrets, fake) = secrets(FakeKeychain::default());
        fake.hang.store(true, Ordering::Relaxed);
        let started = std::time::Instant::now();
        let other = async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            started.elapsed()
        };
        let (password, other_ran_at) = tokio::join!(secrets.password("eeeeeeeeeeeeeeee"), other);
        assert_eq!(password, None);
        assert!(other_ran_at < Duration::from_millis(150), "another task ran while the keychain hung: {other_ran_at:?}");
    }

    #[tokio::test]
    async fn a_failing_keychain_is_not_asked_again_on_reads() {
        let (secrets, fake) = secrets(FakeKeychain::default());
        fake.fail.store(true, Ordering::Relaxed);
        assert_eq!(secrets.password("ffffffffffffffff").await, None);
        assert_eq!(fake.calls.load(Ordering::Relaxed), 1);
        assert_eq!(secrets.password("0000000000000001").await, None);
        assert!(!secrets.saved("0000000000000002").await);
        assert!(!secrets.available().await);
        assert_eq!(fake.calls.load(Ordering::Relaxed), 1, "reads stop asking after a failure");
        fake.fail.store(false, Ordering::Relaxed);
        assert!(secrets.save("0000000000000003", "pw").await.is_ok(), "an explicit save tries again");
        assert_eq!(secrets.password("0000000000000004").await, None);
        assert_eq!(fake.calls.load(Ordering::Relaxed), 3, "and reads ask again after it worked");
    }

    #[tokio::test]
    async fn availability_is_probed_once_without_reading_an_entry() {
        let (secrets, fake) = secrets(FakeKeychain::default());
        assert!(secrets.available().await);
        fake.fail.store(true, Ordering::Relaxed);
        assert!(secrets.available().await, "the probe ran once; only real calls change it");
    }
}
