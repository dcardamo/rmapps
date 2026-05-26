//! Preflight checklist for an inkapp app. See
//! docs/superpowers/specs/2026-05-25-local-preview-and-doctor-design.md.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use inkapp_config::store::ConfigStore;
use inkapp_config::Config as ConfigTrait;
use inkapp_core::connector::Connector;
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

    /// Add a check that a typed config section resolves without error.
    /// The resolution is done eagerly (at call time) and the outcome is stored
    /// as a `StaticCheck` so it does not require `store` to outlive `self`.
    pub fn config_resolves<T: ConfigTrait + 'static>(
        mut self,
        store: &ConfigStore,
        instance: &str,
        label: &str,
    ) -> Self {
        let res = store.resolve::<T>(instance);
        self.checks.push(Box::new(StaticCheck {
            label: format!("[{}] config resolves", label),
            outcome_status: match &res {
                Ok(_) => Status::Pass,
                Err(_) => Status::Fail,
            },
            detail: res.err().map(|e| e.to_string()).unwrap_or_default(),
        }));
        self
    }

    /// Add a check that calls `connector.refresh()` and reports its result.
    pub fn connector_refresh(mut self, label: &str, c: Arc<dyn Connector>) -> Self {
        self.checks.push(Box::new(ConnectorCheck {
            label: format!("{} connector refresh", label),
            c,
        }));
        self
    }

    /// Add a check that builds the app via `build()`, renders the full document
    /// set, and reports the first non-`_banner` doc's page count and byte size.
    ///
    /// `build` may be an `async` closure — it is called inside a dedicated
    /// single-threaded tokio runtime running in a `spawn_blocking` thread.  This
    /// sidesteps the `!Send` bound on `App::render` (which holds `dyn Component`
    /// trait objects) while keeping `Checklist` itself `Send`.
    ///
    /// Any error returned by `build` or by the render is reported as a Fail
    /// outcome, so missing secrets / config / connector errors surface cleanly.
    pub fn render_probe<F, Fut, M, Msg, Cx>(mut self, build: F) -> Self
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<
                Output = inkapp_core::error::Result<inkapp_core::runtime::App<M, Msg, Cx>>,
            > + 'static,
        M: 'static,
        Msg: 'static,
        Cx: inkapp_core::connector::ConnectorSet + 'static,
    {
        // Wrap `build` in an `Option` so the `FnOnce` can be stored behind `Mutex`
        // and taken exactly once when `Check::run` fires.
        let build_opt: std::sync::Mutex<Option<F>> = std::sync::Mutex::new(Some(build));
        let thunk: RenderThunk = Box::new(move || {
            // Take the FnOnce out of the inner mutex.
            let f = build_opt
                .lock()
                .unwrap()
                .take()
                .expect("builder taken twice");
            // Run the !Send futures in a fresh single-threaded tokio runtime
            // spawned via spawn_blocking, so we don't block the multi-thread executor.
            tokio::task::spawn_blocking(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("probe runtime")
                    .block_on(async move {
                        let mut application = f().await?;
                        let mut set = inkapp_core::runtime::DocSet::default();
                        application.render(&mut set).await
                    })
            })
        });
        self.checks.push(Box::new(RenderProbe {
            build: std::sync::Mutex::new(Some(thunk)),
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

/// A check whose outcome was computed eagerly at construction time (e.g. a
/// config resolution that borrows a `ConfigStore` only at the call site).
struct StaticCheck {
    label: String,
    outcome_status: Status,
    detail: String,
}

#[async_trait]
impl Check for StaticCheck {
    async fn run(&self) -> Outcome {
        Outcome {
            name: self.label.clone(),
            status: self.outcome_status.clone(),
            detail: self.detail.clone(),
        }
    }
}

/// A check that calls `Connector::refresh` and maps the result to Pass/Fail.
struct ConnectorCheck {
    label: String,
    c: Arc<dyn Connector>,
}

#[async_trait]
impl Check for ConnectorCheck {
    async fn run(&self) -> Outcome {
        match self.c.refresh().await {
            Ok(()) => Outcome {
                name: self.label.clone(),
                status: Status::Pass,
                detail: String::new(),
            },
            Err(e) => Outcome {
                name: self.label.clone(),
                status: Status::Fail,
                detail: e.to_string(),
            },
        }
    }
}

// --- RenderProbe ---

/// A thunk that, when called, spawns a single-threaded runtime in a blocking
/// thread and runs the app render there (sidesteps the `!Send` component
/// objects inside `App`). Returns a `JoinHandle` that resolves to the doc list.
type RenderThunk = Box<
    dyn FnOnce() -> tokio::task::JoinHandle<
            inkapp_core::error::Result<Vec<inkapp_core::runtime::RenderedDoc>>,
        > + Send,
>;

struct RenderProbe {
    build: std::sync::Mutex<Option<RenderThunk>>,
}

#[async_trait]
impl Check for RenderProbe {
    async fn run(&self) -> Outcome {
        let label = "render probe".to_string();
        let thunk = self.build.lock().unwrap().take();
        let Some(t) = thunk else {
            return Outcome {
                name: label,
                status: Status::Fail,
                detail: "probe already consumed".into(),
            };
        };
        let join_result = t().await;
        let result = match join_result {
            Ok(r) => r,
            Err(e) => {
                return Outcome {
                    name: label,
                    status: Status::Fail,
                    detail: format!("probe task panicked: {e}"),
                };
            }
        };
        match result {
            Ok(docs) => {
                let probe = docs.iter().find(|d| d.key.0 != "_banner");
                match probe {
                    Some(d) => Outcome {
                        name: label,
                        status: Status::Pass,
                        detail: format!(
                            "first content doc '{}' → {} pages, {} bytes",
                            d.key.0,
                            d.page_count,
                            d.pdf.len()
                        ),
                    },
                    None => Outcome {
                        name: label,
                        status: Status::Fail,
                        detail: "no content docs to render".into(),
                    },
                }
            }
            Err(e) => Outcome {
                name: label,
                status: Status::Fail,
                detail: e.to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inkapp_config::store::ConfigStore;
    use inkapp_core::crypto::Key;
    use inkapp_core::runtime::{app, App as Runtime};
    use inkapp_core::secrets::{Scope, SecretStore};
    use inkapp_readwise_reader::Readwise;
    use reading_queue::{update, view, App as RqApp, Connectors};
    use std::sync::Arc;

    fn build_cassette_app(
    ) -> inkapp_core::error::Result<Runtime<RqApp, reading_queue::Msg, Connectors>> {
        let connectors = Connectors::from_arc(Arc::new(Readwise::from_cassette()));
        Ok(app(RqApp)
            .connector(connectors)
            .update(update)
            .view(view)
            .key(Key::from_bytes([7u8; 32]))
            .build())
    }

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

    #[tokio::test]
    async fn config_resolves_passes_on_valid_section() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        std::fs::write(
            &cfg,
            "[app.reading-queue.default]\ndevice_folder = \"/RQ\"\n",
        )
        .unwrap();
        let store = ConfigStore::open(&cfg).unwrap();
        let outcomes = Checklist::new(dir.path().join("s.json"))
            .config_resolves::<reading_queue::AppConfig>(&store, "default", "app.reading-queue")
            .collect()
            .await;
        assert_eq!(outcomes.len(), 1);
        assert!(
            matches!(outcomes[0].status, Status::Pass),
            "{:?}",
            outcomes[0]
        );
    }

    #[tokio::test]
    async fn connector_refresh_passes_for_cassette() {
        let dir = tempfile::tempdir().unwrap();
        let rw: Arc<dyn inkapp_core::connector::Connector> = Arc::new(Readwise::from_cassette());
        let outcomes = Checklist::new(dir.path().join("s.json"))
            .connector_refresh("readwise", rw)
            .collect()
            .await;
        assert_eq!(outcomes.len(), 1);
        assert!(
            matches!(outcomes[0].status, Status::Pass),
            "{:?}",
            outcomes[0]
        );
    }

    #[tokio::test]
    async fn render_probe_passes_on_cassette_app() {
        let dir = tempfile::tempdir().unwrap();
        let outcomes = Checklist::new(dir.path().join("s.json"))
            .render_probe(|| async { build_cassette_app() })
            .collect()
            .await;
        assert_eq!(outcomes.len(), 1);
        assert!(
            matches!(outcomes[0].status, Status::Pass),
            "{:?}",
            outcomes[0]
        );
        assert!(
            outcomes[0].detail.contains("pages"),
            "detail mentions pages: {:?}",
            outcomes[0]
        );
        assert!(
            outcomes[0].detail.contains("bytes"),
            "detail mentions bytes: {:?}",
            outcomes[0]
        );
    }

    #[tokio::test]
    async fn doctor_populated_returns_zero_end_to_end() {
        let dir = tempfile::tempdir().unwrap();
        let secrets_path = dir.path().join("secrets.json");
        {
            let mut s = SecretStore::open(&secrets_path).unwrap();
            s.set(Scope::UserKey, "default", &[0u8; 32]).unwrap();
            s.set(Scope::ConnectorCred, "readwise", b"tok").unwrap();
            s.set(Scope::DeviceAuth, "remarkable", b"auth").unwrap();
        }
        let cfg = dir.path().join("config.toml");
        // DeviceConfig and PageConfig have namespace = "framework", so they
        // live at [device] / [page] directly (no instance sub-key).
        std::fs::write(
            &cfg,
            "[app.reading-queue.default]\ndevice_folder = \"/RQ\"\n\
             [page]\n\
             [device]\nbackend = \"remarkable\"\n",
        )
        .unwrap();
        let store = ConfigStore::open(&cfg).unwrap();
        let rw: Arc<dyn inkapp_core::connector::Connector> = Arc::new(Readwise::from_cassette());
        let code = Checklist::new(&secrets_path)
            .user_key()
            .secret(Scope::ConnectorCred, "readwise")
            .secret(Scope::DeviceAuth, "remarkable")
            .config_resolves::<reading_queue::AppConfig>(&store, "default", "app.reading-queue")
            .config_resolves::<inkapp_core::geometry::PageConfig>(&store, "default", "page")
            .config_resolves::<inkapp_core::geometry::DeviceConfig>(&store, "default", "device")
            .connector_refresh("readwise", rw)
            .render_probe(|| async { build_cassette_app() })
            .run()
            .await;
        assert_eq!(code, 0);
    }
}
