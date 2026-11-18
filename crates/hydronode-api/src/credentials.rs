//! Where tokens live: the OS keychain, or a private file when there is none.
//!
//! Refreshing happens under an exclusive lock file per profile. Whoever holds the lock reads the
//! credential again first: if another process refreshed in the meantime, its result is used and
//! no second refresh is sent (a replayed refresh token would end the whole session family).

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const SERVICE: &str = "hydronode-cli";
const FILE_NAME: &str = "credentials.json";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Credential {
    /// A normal HydroNode session (browser or device login): access JWT + rotating refresh token.
    Session {
        access: String,
        refresh: String,
        /// Unix seconds when `access` runs out.
        #[serde(rename = "expiresAt")]
        expires_at: i64,
    },
    /// A personal access token: `hat_` full, `hrt_` read only.
    Pat { token: String },
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Credential::Session { expires_at, .. } => f
                .debug_struct("Session")
                .field("expires_at", expires_at)
                .finish_non_exhaustive(),
            Credential::Pat { token } => f
                .debug_struct("Pat")
                .field("prefix", &token.get(..4).unwrap_or(""))
                .finish_non_exhaustive(),
        }
    }
}

impl Credential {
    pub fn bearer(&self) -> &str {
        match self {
            Credential::Session { access, .. } => access,
            Credential::Pat { token } => token,
        }
    }

    pub fn is_read_only(&self) -> bool {
        matches!(self, Credential::Pat { token } if token.starts_with("hrt_"))
    }

    pub fn is_session(&self) -> bool {
        matches!(self, Credential::Session { .. })
    }

    /// `session`, `pat` or `read_pat`, as `GET /api/agent/v1/me` names them.
    pub fn kind_name(&self) -> &'static str {
        match self {
            Credential::Session { .. } => "session",
            Credential::Pat { .. } if self.is_read_only() => "read_pat",
            Credential::Pat { .. } => "pat",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreKind {
    /// OS keychain, falls back to the file with a warning.
    Keyring,
    /// Only the private file (0600). Set with `HYDRONODE_CREDENTIAL_STORE=file`.
    File,
}

impl StoreKind {
    pub fn from_env() -> StoreKind {
        match std::env::var("HYDRONODE_CREDENTIAL_STORE").as_deref() {
            Ok("file") => StoreKind::File,
            _ => StoreKind::Keyring,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CredentialStore {
    dir: PathBuf,
    kind: StoreKind,
}

static WARNED: AtomicBool = AtomicBool::new(false);

fn warn_fallback(reason: &str) {
    if !WARNED.swap(true, Ordering::Relaxed) {
        eprintln!(
            "warning: no usable system keychain ({reason}), keeping tokens in a private file (0600)"
        );
    }
}

impl CredentialStore {
    pub fn new(dir: &Path, kind: StoreKind) -> CredentialStore {
        CredentialStore {
            dir: dir.to_path_buf(),
            kind,
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn get(&self, profile: &str) -> Result<Option<Credential>> {
        if self.kind == StoreKind::Keyring {
            match keyring_get(profile) {
                Ok(Some(c)) => return Ok(Some(c)),
                Ok(None) => {}
                Err(reason) => warn_fallback(&reason),
            }
        }
        Ok(self.read_file()?.remove(profile))
    }

    pub fn set(&self, profile: &str, credential: &Credential) -> Result<()> {
        if self.kind == StoreKind::Keyring {
            match keyring_set(profile, credential) {
                Ok(()) => {
                    // a copy from an earlier fallback must not shadow the keychain later
                    self.remove_from_file(profile)?;
                    return Ok(());
                }
                Err(reason) => warn_fallback(&reason),
            }
        }
        let mut all = self.read_file()?;
        all.insert(profile.to_string(), credential.clone());
        self.write_file(&all)
    }

    pub fn delete(&self, profile: &str) -> Result<()> {
        if self.kind == StoreKind::Keyring {
            let _ = keyring_delete(profile);
        }
        self.remove_from_file(profile)
    }

    fn remove_from_file(&self, profile: &str) -> Result<()> {
        let mut all = self.read_file()?;
        if all.remove(profile).is_some() {
            self.write_file(&all)?;
        }
        Ok(())
    }

    fn file_path(&self) -> PathBuf {
        self.dir.join(FILE_NAME)
    }

    fn read_file(&self) -> Result<BTreeMap<String, Credential>> {
        match std::fs::read_to_string(self.file_path()) {
            Ok(text) if text.trim().is_empty() => Ok(BTreeMap::new()),
            Ok(text) => serde_json::from_str(&text)
                .map_err(|e| Error::Config(format!("{FILE_NAME} is damaged: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e.into()),
        }
    }

    fn write_file(&self, all: &BTreeMap<String, Credential>) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let tmp = self.dir.join(format!("{FILE_NAME}.tmp"));
        let text = serde_json::to_string_pretty(all).map_err(|e| Error::Other(e.to_string()))?;
        {
            use std::io::Write;
            let mut file = private_file(&tmp)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp, self.file_path())?;
        Ok(())
    }

    /// Run `f` while holding the profile's exclusive lock (blocks until it is free).
    pub fn with_lock_blocking<T>(&self, profile: &str, f: impl FnOnce() -> T) -> Result<T> {
        let lock = self.lock(profile)?;
        let out = f();
        drop(lock);
        Ok(out)
    }

    /// Take the profile's exclusive lock. Released when the guard drops.
    pub fn lock(&self, profile: &str) -> Result<LockGuard> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(format!("{}.lock", sanitize(profile)));
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        file.lock()?;
        Ok(LockGuard { file })
    }
}

pub struct LockGuard {
    file: File,
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

fn sanitize(profile: &str) -> String {
    profile
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(unix)]
fn private_file(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::fs::PermissionsExt;
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

/// Windows: the file sits in the user's profile, which is already private by default. On top,
/// drop inherited rights and grant only the current user (`icacls`, part of every Windows),
/// before anything is written into it. If that fails, the profile's own rights still apply.
#[cfg(windows)]
fn private_file(path: &Path) -> std::io::Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    let user = match (std::env::var("USERDOMAIN"), std::env::var("USERNAME")) {
        (Ok(d), Ok(u)) if !d.is_empty() => format!("{d}\\{u}"),
        (_, Ok(u)) => u,
        _ => String::new(),
    };
    let ok = !user.is_empty()
        && std::process::Command::new("icacls")
            .arg(path)
            .args(["/inheritance:r", "/grant:r"])
            .arg(format!("{user}:F"))
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
    if !ok {
        eprintln!(
            "warning: could not limit {} to your user, it keeps the rights of your profile folder",
            path.display()
        );
    }
    Ok(file)
}

#[cfg(not(any(unix, windows)))]
fn private_file(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
}

fn entry(profile: &str) -> std::result::Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, profile).map_err(|e| e.to_string())
}

fn keyring_get(profile: &str) -> std::result::Result<Option<Credential>, String> {
    match entry(profile)?.get_password() {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| format!("keychain entry is damaged: {e}")),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn keyring_set(profile: &str, credential: &Credential) -> std::result::Result<(), String> {
    let text = serde_json::to_string(credential).map_err(|e| e.to_string())?;
    entry(profile)?
        .set_password(&text)
        .map_err(|e| e.to_string())
}

fn keyring_delete(profile: &str) -> std::result::Result<(), String> {
    match entry(profile)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
