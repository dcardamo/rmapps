//! The per-user secrets/config store. Three scopes — connector credentials,
//! per-device auth, and the per-user encryption key — persisted to a single
//! `0600` JSON file. Single-user and plaintext-on-disk for now; at-rest
//! protection, KMS, rotation, and tenant isolation are future (see appdx).

use std::collections::BTreeMap;
use std::path::PathBuf;

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::crypto::Key;
use crate::error::{Error, Result};

/// Where a secret lives. Each maps to a top-level section in the store file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    ConnectorCred,
    DeviceAuth,
    UserKey,
}

/// The fixed name under which the per-user key is stored in `UserKey`.
const USER_KEY_NAME: &str = "default";

/// On-disk shape: section -> name -> base64(bytes).
#[derive(Default, Serialize, Deserialize)]
struct Data {
    #[serde(default)]
    connector_cred: BTreeMap<String, String>,
    #[serde(default)]
    device_auth: BTreeMap<String, String>,
    #[serde(default)]
    user_key: BTreeMap<String, String>,
}

impl Data {
    fn section_mut(&mut self, scope: Scope) -> &mut BTreeMap<String, String> {
        match scope {
            Scope::ConnectorCred => &mut self.connector_cred,
            Scope::DeviceAuth => &mut self.device_auth,
            Scope::UserKey => &mut self.user_key,
        }
    }
    fn section(&self, scope: Scope) -> &BTreeMap<String, String> {
        match scope {
            Scope::ConnectorCred => &self.connector_cred,
            Scope::DeviceAuth => &self.device_auth,
            Scope::UserKey => &self.user_key,
        }
    }
}

/// A file-backed secret store.
pub struct SecretStore {
    path: PathBuf,
    data: Data,
}

impl SecretStore {
    /// Open (or initialize) the store at `path`. A missing file is treated as
    /// empty; it is created on the first write.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let data = match std::fs::read(&path) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|e| Error::Secrets(e.to_string()))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Data::default(),
            Err(e) => return Err(Error::Secrets(e.to_string())),
        };
        Ok(Self { path, data })
    }

    /// Open the store at the default location: `$INKAPP_SECRETS_PATH`, else
    /// `$XDG_CONFIG_HOME/inkapp/secrets.json`, else `$HOME/.config/inkapp/secrets.json`.
    pub fn open_default() -> Result<Self> {
        Self::open(Self::default_path()?)
    }

    pub fn default_path() -> Result<PathBuf> {
        if let Ok(p) = std::env::var("INKAPP_SECRETS_PATH") {
            return Ok(PathBuf::from(p));
        }
        let base = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            PathBuf::from(xdg)
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".config")
        } else {
            return Err(Error::Secrets("no HOME or XDG_CONFIG_HOME set".into()));
        };
        Ok(base.join("inkapp").join("secrets.json"))
    }

    /// Fetch a secret's raw bytes. `Ok(None)` means absent; an `Err` means the
    /// stored value is present but not valid base64 (corrupt store) — callers
    /// must NOT treat that as "absent".
    pub fn get(&self, scope: Scope, name: &str) -> Result<Option<Vec<u8>>> {
        match self.data.section(scope).get(name) {
            None => Ok(None),
            Some(b64) => base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map(Some)
                .map_err(|e| Error::Secrets(format!("corrupt base64 for '{name}': {e}"))),
        }
    }

    /// Store a secret and persist the file.
    pub fn set(&mut self, scope: Scope, name: &str, value: &[u8]) -> Result<()> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(value);
        self.data.section_mut(scope).insert(name.to_string(), b64);
        self.persist()
    }

    /// The per-user encryption key, generated and persisted on first call.
    pub fn user_key(&mut self) -> Result<Key> {
        if let Some(bytes) = self.get(Scope::UserKey, USER_KEY_NAME)? {
            let arr: [u8; 32] = bytes
                .as_slice()
                .try_into()
                .map_err(|_| Error::Secrets("stored user key is not 32 bytes".into()))?;
            return Ok(Key::from_bytes(arr));
        }
        let mut bytes = [0u8; 32];
        getrandom::getrandom(&mut bytes).map_err(|e| Error::Secrets(e.to_string()))?;
        self.set(Scope::UserKey, USER_KEY_NAME, &bytes)?;
        Ok(Key::from_bytes(bytes))
    }

    fn persist(&self) -> Result<()> {
        let parent = self.path.parent().filter(|p| !p.as_os_str().is_empty());
        if let Some(parent) = parent {
            std::fs::create_dir_all(parent).map_err(|e| Error::Secrets(e.to_string()))?;
        }
        let json =
            serde_json::to_vec_pretty(&self.data).map_err(|e| Error::Secrets(e.to_string()))?;
        // Write to a sibling temp file, tighten perms, then atomically rename
        // over the target so the key file is never visible with broad perms.
        let dir = parent
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let mut tmp =
            tempfile::NamedTempFile::new_in(&dir).map_err(|e| Error::Secrets(e.to_string()))?;
        use std::io::Write;
        tmp.write_all(&json)
            .map_err(|e| Error::Secrets(e.to_string()))?;
        tmp.flush().map_err(|e| Error::Secrets(e.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tmp.as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|e| Error::Secrets(e.to_string()))?;
        }
        tmp.persist(&self.path)
            .map_err(|e| Error::Secrets(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secrets.json");
        (dir, path)
    }

    #[test]
    fn set_get_round_trips_all_scopes_across_reopen() {
        let (_d, path) = tmp();
        {
            let mut s = SecretStore::open(&path).unwrap();
            s.set(Scope::ConnectorCred, "readwise", b"tok").unwrap();
            s.set(Scope::DeviceAuth, "remarkable", b"auth").unwrap();
            s.set(Scope::UserKey, "default", b"k").unwrap();
        }
        let s = SecretStore::open(&path).unwrap();
        assert_eq!(
            s.get(Scope::ConnectorCred, "readwise").unwrap().unwrap(),
            b"tok"
        );
        assert_eq!(
            s.get(Scope::DeviceAuth, "remarkable").unwrap().unwrap(),
            b"auth"
        );
        assert_eq!(s.get(Scope::UserKey, "default").unwrap().unwrap(), b"k");
    }

    #[test]
    fn user_key_is_stable_across_reopen() {
        let (_d, path) = tmp();
        let first = SecretStore::open(&path).unwrap().user_key().unwrap();
        let second = SecretStore::open(&path).unwrap().user_key().unwrap();
        assert_eq!(first.as_bytes(), second.as_bytes());
    }

    #[test]
    fn user_key_distinct_per_path() {
        let (_d1, p1) = tmp();
        let (_d2, p2) = tmp();
        let a = SecretStore::open(&p1).unwrap().user_key().unwrap();
        let b = SecretStore::open(&p2).unwrap().user_key().unwrap();
        assert_ne!(a.as_bytes(), b.as_bytes());
    }

    #[cfg(unix)]
    #[test]
    fn file_is_0600() {
        use std::os::unix::fs::PermissionsExt;
        let (_d, path) = tmp();
        let mut s = SecretStore::open(&path).unwrap();
        s.set(Scope::ConnectorCred, "x", b"y").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn open_default_honors_env_override() {
        let (_d, path) = tmp();
        std::env::set_var("INKAPP_SECRETS_PATH", &path);
        let mut s = SecretStore::open_default().unwrap();
        s.set(Scope::ConnectorCred, "x", b"y").unwrap();
        std::env::remove_var("INKAPP_SECRETS_PATH");
        assert!(path.exists());
    }

    #[test]
    fn corrupt_base64_is_an_error_not_absent() {
        let (_d, path) = tmp();
        std::fs::write(&path, br#"{"connector_cred":{"x":"!!!not-base64!!!"}}"#).unwrap();
        let s = SecretStore::open(&path).unwrap();
        assert!(matches!(
            s.get(Scope::ConnectorCred, "x"),
            Err(Error::Secrets(_))
        ));
    }
}
