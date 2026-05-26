//! Preflight checklist for an inkapp app. See
//! docs/superpowers/specs/2026-05-25-local-preview-and-doctor-design.md.

use std::path::PathBuf;

use async_trait::async_trait;

use inkapp_core::secrets::{Scope, SecretStore};

/// Per-check result status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Pass,
    Fail,
    Skip,
}

/// The result of running a single check.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub name: String,
    pub status: Status,
    pub detail: String,
}

#[async_trait]
trait Check: Send {
    async fn run(&self) -> Outcome;
}

/// Doctor builder. Bind to a secrets-file path once; secret checks inspect it.
pub struct Checklist {
    secrets_path: PathBuf,
    checks: Vec<Box<dyn Check>>,
}

impl Checklist {
    pub fn new(secrets_path: impl Into<PathBuf>) -> Self {
        Self {
            secrets_path: secrets_path.into(),
            checks: Vec::new(),
        }
    }

    /// Add a check that the per-user encryption key is present and 32 bytes.
    pub fn user_key(mut self) -> Self {
        self.checks.push(Box::new(SecretCheck {
            path: self.secrets_path.clone(),
            scope: Scope::UserKey,
            name: "default".to_string(),
            label: "user key present".to_string(),
            expect_len: Some(32),
        }));
        self
    }

    /// Add a check that a named secret is present in the given scope.
    pub fn secret(mut self, scope: Scope, name: impl Into<String>) -> Self {
        let name = name.into();
        let label = match scope {
            Scope::ConnectorCred => format!("{} connector token present", name),
            Scope::DeviceAuth => format!("device auth '{}' present", name),
            Scope::UserKey => format!("user key '{}' present", name),
        };
        self.checks.push(Box::new(SecretCheck {
            path: self.secrets_path.clone(),
            scope,
            name,
            label,
            expect_len: None,
        }));
        self
    }

    /// Run every check, returning the outcomes. Used by tests.
    pub async fn collect(self) -> Vec<Outcome> {
        let mut out = Vec::with_capacity(self.checks.len());
        for c in &self.checks {
            out.push(c.run().await);
        }
        out
    }

    /// Run every check, print rows, return exit code (0 if all Pass/Skip; 1 otherwise).
    pub async fn run(self) -> i32 {
        let outcomes = self.collect().await;
        let mut fail = false;
        for o in &outcomes {
            let tag = match o.status {
                Status::Pass => "[PASS]",
                Status::Fail => {
                    fail = true;
                    "[FAIL]"
                }
                Status::Skip => "[SKIP]",
            };
            if o.detail.is_empty() {
                println!("{tag} {}", o.name);
            } else {
                println!("{tag} {:<42} — {}", o.name, o.detail);
            }
        }
        if fail {
            1
        } else {
            0
        }
    }
}

// --- Internal check types ---

struct SecretCheck {
    path: PathBuf,
    scope: Scope,
    name: String,
    label: String,
    /// If set, the stored value must be exactly this many bytes.
    expect_len: Option<usize>,
}

#[async_trait]
impl Check for SecretCheck {
    async fn run(&self) -> Outcome {
        let store = match SecretStore::open(&self.path) {
            Ok(s) => s,
            Err(e) => {
                return Outcome {
                    name: self.label.clone(),
                    status: Status::Fail,
                    detail: format!("open secrets failed: {e}"),
                }
            }
        };
        match store.get(self.scope, &self.name) {
            Ok(Some(bytes)) => {
                if let Some(n) = self.expect_len {
                    if bytes.len() != n {
                        return Outcome {
                            name: self.label.clone(),
                            status: Status::Fail,
                            detail: format!(
                                "stored value is {} bytes, expected {}",
                                bytes.len(),
                                n
                            ),
                        };
                    }
                }
                Outcome {
                    name: self.label.clone(),
                    status: Status::Pass,
                    detail: String::new(),
                }
            }
            Ok(None) => Outcome {
                name: self.label.clone(),
                status: Status::Fail,
                detail: format!("{:?} name='{}' not in store", self.scope, self.name),
            },
            Err(e) => Outcome {
                name: self.label.clone(),
                status: Status::Fail,
                detail: format!("get failed: {e}"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkapp_core::secrets::{Scope, SecretStore};

    #[tokio::test]
    async fn secrets_empty_store_fails_each_check() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("secrets.json"); // file does not exist
        let outcomes = Checklist::new(&path)
            .user_key()
            .secret(Scope::ConnectorCred, "readwise")
            .secret(Scope::DeviceAuth, "remarkable")
            .collect()
            .await;
        assert_eq!(outcomes.len(), 3);
        for o in &outcomes {
            assert!(
                matches!(o.status, Status::Fail),
                "{} should fail: {:?}",
                o.name,
                o.status
            );
        }
        // Names mention each scope/name so the user can recognize what's missing.
        assert!(outcomes.iter().any(|o| o.name.contains("user key")));
        assert!(outcomes.iter().any(|o| o.name.contains("readwise")));
        assert!(outcomes.iter().any(|o| o.name.contains("remarkable")));
    }

    #[tokio::test]
    async fn secrets_populated_store_passes_each_check() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("secrets.json");
        {
            let mut s = SecretStore::open(&path).unwrap();
            s.set(Scope::UserKey, "default", &[0u8; 32]).unwrap();
            s.set(Scope::ConnectorCred, "readwise", b"tok").unwrap();
            s.set(Scope::DeviceAuth, "remarkable", b"auth").unwrap();
        }
        let outcomes = Checklist::new(&path)
            .user_key()
            .secret(Scope::ConnectorCred, "readwise")
            .secret(Scope::DeviceAuth, "remarkable")
            .collect()
            .await;
        for o in &outcomes {
            assert!(
                matches!(o.status, Status::Pass),
                "{} should pass: {:?}",
                o.name,
                o
            );
        }
    }

    #[tokio::test]
    async fn run_returns_exit_codes() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("secrets.json");
        let code_empty = Checklist::new(&path).user_key().run().await;
        assert_eq!(code_empty, 1, "missing user_key => exit 1");

        {
            let mut s = SecretStore::open(&path).unwrap();
            s.set(Scope::UserKey, "default", &[0u8; 32]).unwrap();
        }
        let code_ok = Checklist::new(&path).user_key().run().await;
        assert_eq!(code_ok, 0, "present user_key => exit 0");
    }
}
