//! Saved passwords (SSH passwords / key passphrases by configuration id, git HTTPS
//! credentials, the HTTP proxy's) in the OS credential store: Windows Credential
//! Manager, the macOS Keychain, the Secret Service on Linux. The file at `path` only
//! indexes them (an id and the keys, never a secret); owner-only on Unix.
//!
//! Older versions kept the secrets themselves in that file: they are moved to the
//! credential store on first use. Without a credential store (a headless Linux
//! host) such a file is still read and forgetting a secret rewrites it without
//! that one, but no new secret is written to disk: saving fails and callers keep
//! the secret for the session only.
//!
//! A process whose data lives in a home of its own (tests, measurements) calls
//! [`keep_secrets_in_files`]: from then on secrets go to an owner-only file beside
//! their index and the OS store is never opened.

use ide_core::{IdeError, IdeResult};
pub use keyring_core::{Error as StoreError, Result as StoreResult};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const SERVICE: &str = "ai.l0ng.openidea";

/// Serialises read-modify-write of the index files (several stores may share one).
static LOCK: Mutex<()> = Mutex::new(());

/// Set by [`keep_secrets_in_files`].
static IN_FILES: AtomicBool = AtomicBool::new(false);

/// From now on every [`SecretFile`] keeps its secrets in an owner-only file beside its index
/// (`<index>.vault`) instead of the OS credential store, and the OS store refuses to work, so
/// a store made earlier loses saving rather than reaching the user's keychain. For processes
/// whose data lives in a temporary or overridden home. There is no way back.
pub fn keep_secrets_in_files() {
    IN_FILES.store(true, Ordering::SeqCst);
}

/// Whether [`keep_secrets_in_files`] was called in this process.
pub fn secrets_in_files() -> bool {
    IN_FILES.load(Ordering::SeqCst)
}

/// Whether this process has set up a default credential store (the OS one, on its first use).
/// A check for tests that never opens the keychain.
pub fn default_store_installed() -> bool {
    keyring_core::get_default_store().is_some()
}

/// Reads the secrets of an older version's file, by key (`None`: not that format).
pub type Legacy = fn(serde_json::Value) -> Option<BTreeMap<String, String>>;

/// Where the secrets are kept: the default credential store, or a stand-in in tests.
pub trait Vault: Send + Sync {
    fn available(&self) -> bool;
    fn get(&self, service: &str, key: &str) -> StoreResult<String>;
    fn set(&self, service: &str, key: &str, secret: &str) -> StoreResult<()>;
    fn delete(&self, service: &str, key: &str) -> StoreResult<()>;
}

/// `keyring_core`'s default store: the OS's, unless a test installed another one. It refuses to
/// work while `in_files` is set.
struct DefaultStore {
    in_files: &'static AtomicBool,
}

const DEFAULT_STORE: DefaultStore = DefaultStore { in_files: &IN_FILES };

impl DefaultStore {
    fn entry(&self, service: &str, key: &str) -> StoreResult<keyring_core::Entry> {
        if !self.available() {
            return Err(StoreError::NoDefaultStore);
        }
        keyring_core::Entry::new(service, key)
    }
}

impl Vault for DefaultStore {
    fn available(&self) -> bool {
        if self.in_files.load(Ordering::SeqCst) {
            return false;
        }
        if keyring_core::get_default_store().is_some() {
            return true;
        }
        static WARNED: std::sync::Once = std::sync::Once::new();
        match keyring::Entry::store_status() {
            Ok(()) => true,
            Err(e) => {
                WARNED.call_once(|| tracing::warn!("saved passwords: no credential store: {e}"));
                false
            }
        }
    }

    fn get(&self, service: &str, key: &str) -> StoreResult<String> {
        self.entry(service, key)?.get_password()
    }

    fn set(&self, service: &str, key: &str, secret: &str) -> StoreResult<()> {
        self.entry(service, key)?.set_password(secret)
    }

    fn delete(&self, service: &str, key: &str) -> StoreResult<()> {
        self.entry(service, key)?.delete_credential()
    }
}

/// Secrets in an owner-only JSON file (service → key → secret). Only [`SecretFile`] calls it,
/// always while holding [`LOCK`].
struct FileVault(PathBuf);

type Vaulted = BTreeMap<String, BTreeMap<String, String>>;

impl FileVault {
    fn beside(index: &Path) -> Self {
        let mut path = index.as_os_str().to_owned();
        path.push(".vault");
        Self(PathBuf::from(path))
    }

    fn read(&self) -> StoreResult<Vaulted> {
        match std::fs::read(&self.0) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| StoreError::BadDataFormat(bytes, Box::new(e))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vaulted::new()),
            Err(e) => Err(StoreError::PlatformFailure(Box::new(e))),
        }
    }

    fn write(&self, all: &Vaulted) -> StoreResult<()> {
        let bytes = serde_json::to_vec_pretty(all).map_err(|e| StoreError::PlatformFailure(Box::new(e)))?;
        write_private(&self.0, &bytes).map_err(|e| StoreError::PlatformFailure(Box::new(e)))
    }
}

impl Vault for FileVault {
    fn available(&self) -> bool {
        true
    }

    fn get(&self, service: &str, key: &str) -> StoreResult<String> {
        self.read()?.get(service).and_then(|keys| keys.get(key)).cloned().ok_or(StoreError::NoEntry)
    }

    fn set(&self, service: &str, key: &str, secret: &str) -> StoreResult<()> {
        let mut all = self.read()?;
        all.entry(service.to_owned()).or_default().insert(key.to_owned(), secret.to_owned());
        self.write(&all)
    }

    fn delete(&self, service: &str, key: &str) -> StoreResult<()> {
        let mut all = self.read()?;
        let keys = all.get_mut(service).ok_or(StoreError::NoEntry)?;
        keys.remove(key).ok_or(StoreError::NoEntry)?;
        if keys.is_empty() {
            all.remove(service);
        }
        self.write(&all)
    }
}

pub struct SecretFile {
    path: PathBuf,
    legacy: Legacy,
    vault: Arc<dyn Vault>,
}

/// The file's contents. `id` keeps the credentials of different data directories
/// (and of tests) apart in the one OS store.
#[derive(Serialize, Deserialize)]
struct Index {
    id: String,
    keys: BTreeSet<String>,
}

enum Contents {
    Index(Index),
    /// An older version's file that could not be moved, and why.
    Plaintext(BTreeMap<String, String>, String),
}

fn service(id: &str) -> String {
    format!("{SERVICE}/{id}")
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

impl SecretFile {
    pub fn new(path: PathBuf) -> Self {
        Self::with_legacy(path, |v| serde_json::from_value(v).ok())
    }

    pub fn with_legacy(path: PathBuf, legacy: Legacy) -> Self {
        let vault: Arc<dyn Vault> =
            if secrets_in_files() { Arc::new(FileVault::beside(&path)) } else { Arc::new(DEFAULT_STORE) };
        Self::with_vault(path, legacy, vault)
    }

    pub fn with_vault(path: PathBuf, legacy: Legacy, vault: Arc<dyn Vault>) -> Self {
        Self { path, legacy, vault }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let _g = LOCK.lock();
        match self.load() {
            Contents::Index(mut index) if index.keys.contains(key) => match self.vault.get(&service(&index.id), key) {
                Ok(secret) => Some(secret),
                Err(StoreError::NoEntry) => {
                    // Removed outside the app: stop listing it.
                    index.keys.remove(key);
                    self.write_logged(&index);
                    None
                }
                Err(e) => {
                    tracing::warn!("saved passwords: {key}: {e}");
                    None
                }
            },
            Contents::Index(_) => None,
            Contents::Plaintext(mut old, _) => old.remove(key),
        }
    }

    pub fn set(&self, key: &str, secret: &str) -> IdeResult<()> {
        let _g = LOCK.lock();
        let mut index = match self.load() {
            Contents::Index(index) => index,
            Contents::Plaintext(_, why) => return Err(unavailable(&why)),
        };
        let service = service(&index.id);
        self.vault.set(&service, key, secret).map_err(|e| unavailable(&e.to_string()))?;
        if index.keys.insert(key.to_owned())
            && let Err(e) = self.write(&index)
        {
            let _ = self.vault.delete(&service, key);
            return Err(e);
        }
        Ok(())
    }

    pub fn delete(&self, key: &str) {
        let _g = LOCK.lock();
        match self.load() {
            Contents::Index(mut index) => {
                if !index.keys.contains(key) {
                    return;
                }
                match self.vault.delete(&service(&index.id), key) {
                    Ok(()) | Err(StoreError::NoEntry) => {}
                    Err(e) => {
                        // Kept in the index so that the next delete tries again.
                        tracing::warn!("saved passwords: could not forget {key}: {e}");
                        return;
                    }
                }
                index.keys.remove(key);
                self.write_logged(&index);
            }
            Contents::Plaintext(mut old, _) => {
                if old.remove(key).is_none() {
                    return;
                }
                // A move halfway may have copied it already.
                if let Some(id) = self.pending_id()
                    && let Err(e) = self.vault.delete(&service(&id), key)
                    && !matches!(e, StoreError::NoEntry)
                {
                    tracing::warn!("saved passwords: could not forget {key}: {e}");
                    return;
                }
                self.write_logged(&old);
            }
        }
    }

    /// Every stored key, sorted (the secrets themselves stay private).
    pub fn keys(&self) -> Vec<String> {
        let _g = LOCK.lock();
        match self.load() {
            Contents::Index(index) => index.keys.into_iter().collect(),
            Contents::Plaintext(old, _) => old.into_keys().collect(),
        }
    }

    fn load(&self) -> Contents {
        let value = match std::fs::read(&self.path) {
            Err(_) => return Contents::Index(Index { id: new_id(), keys: BTreeSet::new() }),
            Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(v) => v,
                Err(e) => return self.set_aside(&e.to_string()),
            },
        };
        if let Ok(index) = serde_json::from_value::<Index>(value.clone()) {
            return Contents::Index(index);
        }
        match (self.legacy)(value) {
            Some(old) => self.migrate(old),
            None => self.set_aside("unknown format"),
        }
    }

    /// An unreadable file is renamed rather than overwritten (it may index
    /// credentials, or hold secrets); the store starts over.
    fn set_aside(&self, why: &str) -> Contents {
        let aside = self.path.with_extension(format!("json.unreadable-{}", new_id()));
        tracing::warn!("saved passwords: {}: {why}; moved to {}", self.path.display(), aside.display());
        if let Err(e) = std::fs::rename(&self.path, &aside) {
            // Not overwritten either: nothing is saved until it can be moved.
            return Contents::Plaintext(BTreeMap::new(), format!("{}: {e}", self.path.display()));
        }
        Contents::Index(Index { id: new_id(), keys: BTreeSet::new() })
    }

    fn pending_path(&self) -> PathBuf {
        self.path.with_extension("json.migrating")
    }

    /// The id of a move that has not finished (see [`SecretFile::migrate`]).
    fn pending_id(&self) -> Option<String> {
        let id = std::fs::read_to_string(self.pending_path()).ok()?;
        Some(id.trim().to_owned()).filter(|id| !id.is_empty())
    }

    /// Moves an older version's secrets to the credential store, key by key; a secret
    /// the store refuses for good (too long, say) is forgotten, and the user is asked
    /// for it again. The old file is replaced only when everything else was moved.
    /// Until then the id waits in `<file>.migrating`, so a retry after a failure or a
    /// crash overwrites the same credentials instead of copying them again.
    fn migrate(&self, old: BTreeMap<String, String>) -> Contents {
        if !self.vault.available() {
            return Contents::Plaintext(old, "no credential store".into());
        }
        let pending = self.pending_path();
        let id = match self.pending_id() {
            Some(id) => id,
            None => {
                let id = new_id();
                if let Err(e) = std::fs::write(&pending, &id) {
                    return Contents::Plaintext(old, format!("{}: {e}", pending.display()));
                }
                id
            }
        };
        let mut index = Index { id, keys: BTreeSet::new() };
        for (key, secret) in &old {
            match self.vault.set(&service(&index.id), key, secret) {
                Ok(()) => {
                    index.keys.insert(key.clone());
                }
                Err(e @ (StoreError::TooLong(..) | StoreError::Invalid(..))) => {
                    tracing::warn!("saved passwords: {key} does not fit the credential store and is forgotten: {e}");
                }
                Err(e) => {
                    tracing::warn!("saved passwords: could not move {key} to the credential store: {e}");
                    return Contents::Plaintext(old, e.to_string());
                }
            }
        }
        if let Err(e) = self.write(&index) {
            return Contents::Plaintext(old, e.message);
        }
        let _ = std::fs::remove_file(&pending);
        Contents::Index(index)
    }

    fn write_logged(&self, contents: &impl Serialize) {
        if let Err(e) = self.write(contents) {
            tracing::warn!("{}", e.message);
        }
    }

    fn write(&self, contents: &impl Serialize) -> IdeResult<()> {
        let err = |e: std::io::Error| IdeError::unavailable(format!("saved passwords: {}: {e}", self.path.display()));
        let bytes = serde_json::to_vec_pretty(contents).map_err(|e| IdeError::internal(e.to_string()))?;
        write_private(&self.path, &bytes).map_err(err)
    }
}

/// Replaces `path` with `bytes` through a sibling `<path>.tmp`, owner-only on Unix.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    std::io::Write::write_all(&mut options.open(&tmp)?, bytes)?;
    std::fs::rename(&tmp, path)
}

fn unavailable(why: &str) -> IdeError {
    IdeError::unavailable(format!("saved passwords: {why}"))
}

/// Helpers for tests of crates that save secrets (feature `testing`).
#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use std::path::{Path, PathBuf};

    /// Makes an in-memory credential store the default, so that the test never
    /// touches the user's keychain. Call it before the first secret is saved.
    pub fn use_memory_store() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| keyring_core::set_default_store(keyring_core::sample::Store::new().expect("memory store")));
    }

    /// Every file under `dir` whose bytes contain `needle`.
    pub fn files_containing(dir: &Path, needle: &str) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut pending = vec![dir.to_path_buf()];
        while let Some(d) = pending.pop() {
            for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                } else if std::fs::read(&path).is_ok_and(|b| b.windows(needle.len()).any(|w| w == needle.as_bytes())) {
                    found.push(path);
                }
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{files_containing, use_memory_store};
    use super::*;
    use std::collections::HashMap;

    /// A credential store that can be absent, or fail on chosen keys.
    #[derive(Default)]
    struct FakeVault {
        absent: bool,
        entries: Mutex<HashMap<(String, String), String>>,
        failing: Mutex<HashMap<String, fn() -> StoreError>>,
    }

    impl FakeVault {
        fn fail(&self, key: &str, error: fn() -> StoreError) {
            self.failing.lock().insert(key.to_owned(), error);
        }

        fn heal(&self) {
            self.failing.lock().clear();
        }

        fn check(&self, key: &str) -> StoreResult<()> {
            if self.absent {
                return Err(StoreError::NoDefaultStore);
            }
            self.failing.lock().get(key).map_or(Ok(()), |e| Err(e()))
        }

        fn secrets(&self) -> Vec<String> {
            let mut all: Vec<_> = self.entries.lock().values().cloned().collect();
            all.sort();
            all
        }
    }

    impl Vault for FakeVault {
        fn available(&self) -> bool {
            !self.absent
        }
        fn get(&self, service: &str, key: &str) -> StoreResult<String> {
            self.check(key)?;
            self.entries.lock().get(&(service.to_owned(), key.to_owned())).cloned().ok_or(StoreError::NoEntry)
        }
        fn set(&self, service: &str, key: &str, secret: &str) -> StoreResult<()> {
            self.check(key)?;
            self.entries.lock().insert((service.to_owned(), key.to_owned()), secret.to_owned());
            Ok(())
        }
        fn delete(&self, service: &str, key: &str) -> StoreResult<()> {
            self.check(key)?;
            self.entries.lock().remove(&(service.to_owned(), key.to_owned())).map(|_| ()).ok_or(StoreError::NoEntry)
        }
    }

    fn platform() -> StoreError {
        StoreError::PlatformFailure("locked".into())
    }

    fn with_fake(path: &std::path::Path, vault: &Arc<FakeVault>) -> SecretFile {
        SecretFile::with_vault(path.to_path_buf(), |v| serde_json::from_value(v).ok(), vault.clone())
    }

    const OLD: &str = r#"{"https://a.example": "me\nold-a", "https://b.example": "me\nold-b"}"#;

    #[test]
    fn saved_secrets_never_reach_disk() {
        use_memory_store();
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ssh-passwords.json");
        SecretFile::new(path.clone()).set("a", "hunter2-ssh").unwrap();
        assert_eq!(files_containing(tmp.path(), "hunter2-ssh"), Vec::<PathBuf>::new());
        let reopened = SecretFile::new(path);
        assert_eq!(reopened.get("a").as_deref(), Some("hunter2-ssh"));
        assert_eq!(reopened.keys(), ["a"]);
    }

    #[test]
    fn moves_plaintext_files_of_older_versions_off_disk() {
        use_memory_store();
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("git-credentials.json");
        std::fs::write(&path, r#"{"https://example.com": "me\nold-pw"}"#).unwrap();
        let file = SecretFile::new(path.clone());
        assert_eq!(file.keys(), ["https://example.com"]);
        assert_eq!(files_containing(tmp.path(), "old-pw"), Vec::<PathBuf>::new());
        assert_eq!(SecretFile::new(path).get("https://example.com").as_deref(), Some("me\nold-pw"));
    }

    #[test]
    fn a_failed_move_keeps_the_old_file_and_the_retry_converges() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("git-credentials.json");
        std::fs::write(&path, OLD).unwrap();
        let vault = Arc::new(FakeVault::default());
        vault.fail("https://b.example", platform);

        let file = with_fake(&path, &vault);
        assert_eq!(file.get("https://a.example").as_deref(), Some("me\nold-a"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), OLD, "the old file is untouched");
        assert!(file.set("https://c.example", "me\nnew").unwrap_err().message.contains("locked"));

        vault.heal();
        assert_eq!(file.keys(), ["https://a.example", "https://b.example"]);
        assert_eq!(files_containing(tmp.path(), "old-a"), Vec::<PathBuf>::new());
        assert_eq!(vault.secrets(), ["me\nold-a", "me\nold-b"], "the retry overwrote, not copied");
        assert!(!path.with_extension("json.migrating").exists());
        assert_eq!(with_fake(&path, &vault).get("https://b.example").as_deref(), Some("me\nold-b"));
    }

    #[test]
    fn forgetting_during_a_pending_move_forgets_the_copy_too() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("git-credentials.json");
        std::fs::write(&path, OLD).unwrap();
        let vault = Arc::new(FakeVault::default());
        vault.fail("https://b.example", platform);

        let file = with_fake(&path, &vault);
        file.delete("https://a.example");
        vault.heal();
        assert_eq!(file.keys(), ["https://b.example"]);
        assert_eq!(vault.secrets(), ["me\nold-b"]);
    }

    #[test]
    fn a_secret_the_store_refuses_for_good_is_forgotten_and_does_not_block_saving() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("git-credentials.json");
        std::fs::write(&path, OLD).unwrap();
        let vault = Arc::new(FakeVault::default());
        vault.fail("https://b.example", || StoreError::TooLong("password".into(), 2560));

        let file = with_fake(&path, &vault);
        assert_eq!(file.keys(), ["https://a.example"]);
        assert_eq!(files_containing(tmp.path(), "old-b"), Vec::<PathBuf>::new());
        file.set("https://c.example", "me\nnew").unwrap();
        assert_eq!(file.get("https://c.example").as_deref(), Some("me\nnew"));
    }

    #[test]
    fn without_a_credential_store_the_old_file_is_read_and_nothing_new_is_saved() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("git-credentials.json");
        std::fs::write(&path, OLD).unwrap();
        let vault = Arc::new(FakeVault { absent: true, ..Default::default() });

        let file = with_fake(&path, &vault);
        assert_eq!(file.get("https://a.example").as_deref(), Some("me\nold-a"));
        assert!(file.set("https://c.example", "me\nnew").unwrap_err().message.contains("no credential store"));
        assert_eq!(files_containing(tmp.path(), "me\nnew"), Vec::<PathBuf>::new());
        assert!(!path.with_extension("json.migrating").exists());

        file.delete("https://a.example");
        assert_eq!(file.keys(), ["https://b.example"]);
        assert_eq!(files_containing(tmp.path(), "old-a"), Vec::<PathBuf>::new());
    }

    #[test]
    fn forgets_only_what_the_store_forgot() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = Arc::new(FakeVault::default());
        let file = with_fake(&tmp.path().join("ssh-passwords.json"), &vault);
        file.set("a", "pw").unwrap();
        vault.fail("a", platform);
        file.delete("a");
        assert_eq!(file.keys(), ["a"], "still listed, so that forgetting can be retried");
        vault.heal();
        file.delete("a");
        assert_eq!(file.keys(), Vec::<String>::new());
        assert_eq!(vault.secrets(), Vec::<String>::new());
    }

    #[test]
    fn a_secret_removed_outside_the_app_is_no_longer_listed() {
        let tmp = tempfile::tempdir().unwrap();
        let vault = Arc::new(FakeVault::default());
        let file = with_fake(&tmp.path().join("ssh-passwords.json"), &vault);
        file.set("a", "pw").unwrap();
        vault.entries.lock().clear();
        assert_eq!(file.get("a"), None);
        assert_eq!(file.keys(), Vec::<String>::new());
    }

    #[test]
    fn a_failed_index_write_takes_the_secret_back_out_of_the_store() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ssh-passwords.json");
        std::fs::create_dir(&path).unwrap();
        let vault = Arc::new(FakeVault::default());
        assert!(with_fake(&path, &vault).set("a", "pw").is_err());
        assert_eq!(vault.secrets(), Vec::<String>::new());
    }

    #[test]
    fn an_unreadable_file_is_set_aside_not_overwritten() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ssh-passwords.json");
        std::fs::write(&path, "{ not json").unwrap();
        let vault = Arc::new(FakeVault::default());
        let file = with_fake(&path, &vault);
        assert_eq!(file.keys(), Vec::<String>::new());
        file.set("a", "pw").unwrap();
        std::fs::write(&path, "{ not json either").unwrap();
        assert_eq!(file.keys(), Vec::<String>::new());
        assert_eq!(files_containing(tmp.path(), "{ not json").len(), 2, "both kept, neither overwritten");
    }

    fn with_file_vault(path: &Path) -> SecretFile {
        SecretFile::with_vault(
            path.to_path_buf(),
            |v| serde_json::from_value(v).ok(),
            Arc::new(FileVault::beside(path)),
        )
    }

    #[cfg(unix)]
    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_file_vault_keeps_secrets_beside_the_index_owner_only_and_reads_them_back() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ssh-passwords.json");
        with_file_vault(&path).set("a", "hunter2-file").unwrap();
        let vault = tmp.path().join("ssh-passwords.json.vault");
        assert_eq!(files_containing(tmp.path(), "hunter2-file"), [vault.clone()], "only the vault holds it");
        #[cfg(unix)]
        {
            assert_eq!(mode(&vault), 0o600);
            assert_eq!(mode(&path), 0o600);
        }
        let reopened = with_file_vault(&path);
        assert_eq!(reopened.get("a").as_deref(), Some("hunter2-file"));
        assert_eq!(reopened.keys(), ["a"]);
    }

    #[test]
    fn a_file_vault_forgets_a_deleted_secret_and_keeps_the_others() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("db-passwords.json");
        let file = with_file_vault(&path);
        file.set("a", "first-secret").unwrap();
        file.set("b", "second-secret").unwrap();
        file.delete("a");
        assert_eq!(file.get("a"), None);
        assert_eq!(file.keys(), ["b"]);
        assert_eq!(files_containing(tmp.path(), "first-secret"), Vec::<PathBuf>::new());
        assert_eq!(with_file_vault(&path).get("b").as_deref(), Some("second-secret"));
    }

    #[test]
    fn a_corrupt_file_vault_fails_the_save_instead_of_overwriting_it() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ssh-passwords.json");
        let vault = tmp.path().join("ssh-passwords.json.vault");
        std::fs::write(&vault, "{ not json").unwrap();
        assert!(with_file_vault(&path).set("a", "pw").is_err());
        assert_eq!(std::fs::read_to_string(&vault).unwrap(), "{ not json");
        assert_eq!(with_file_vault(&path).keys(), Vec::<String>::new(), "the index does not list it");
    }

    #[test]
    fn the_os_store_refuses_to_work_once_secrets_are_kept_in_files() {
        static ON: AtomicBool = AtomicBool::new(true);
        let store = DefaultStore { in_files: &ON };
        assert!(!store.available());
        assert!(matches!(store.get("s", "k"), Err(StoreError::NoDefaultStore)));
        assert!(matches!(store.set("s", "k", "pw"), Err(StoreError::NoDefaultStore)));
        assert!(matches!(store.delete("s", "k"), Err(StoreError::NoDefaultStore)));

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ssh-passwords.json");
        let file = SecretFile::with_vault(path.clone(), |v| serde_json::from_value(v).ok(), Arc::new(store));
        assert!(file.set("a", "pw").unwrap_err().message.contains("saved passwords"));
        assert_eq!(file.keys(), Vec::<String>::new());
        std::fs::write(&path, r#"{"a": "old-pw"}"#).unwrap();
        assert_eq!(file.get("a").as_deref(), Some("old-pw"), "an older file is read without the store");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"a": "old-pw"}"#, "and not moved anywhere");
    }

    #[test]
    fn stores_and_forgets_secrets() {
        use_memory_store();
        let tmp = tempfile::tempdir().unwrap();
        let file = SecretFile::new(tmp.path().join("ssh-passwords.json"));
        file.set("a", "pw").unwrap();
        assert_eq!(file.get("a").as_deref(), Some("pw"));
        file.delete("a");
        assert_eq!(file.get("a"), None);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(tmp.path().join("ssh-passwords.json")).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}
