use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, Table};

use crate::config::{Config, Namespace};
use crate::error::{ConfigError, Result};
use crate::schema::Registry;

/// A file-backed configuration store. Read once at launch; callers hold the
/// resolved typed values as an immutable snapshot.
pub struct ConfigStore {
    path: PathBuf,
    doc: DocumentMut,
}

impl ConfigStore {
    /// Open (or treat as empty) the config at `path`. The file is read fully
    /// into memory here; the on-disk file is not needed again until `save`.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let doc = match std::fs::read_to_string(&path) {
            Ok(s) => s
                .parse::<DocumentMut>()
                .map_err(|e| ConfigError::Parse(e.to_string()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => DocumentMut::new(),
            Err(e) => return Err(ConfigError::Io(e.to_string())),
        };
        Ok(Self { path, doc })
    }

    pub fn open_default() -> Result<Self> {
        Self::open(Self::default_path()?)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Resolve the config file path: `$INKAPP_CONFIG_PATH`, else
    /// `$XDG_CONFIG_HOME/inkapp/config.toml`, else `$HOME/.config/inkapp/config.toml`.
    /// Public API — app launchers and the `config path` CLI subcommand call this.
    pub fn default_path() -> Result<PathBuf> {
        if let Ok(p) = std::env::var("INKAPP_CONFIG_PATH") {
            return Ok(PathBuf::from(p));
        }
        let base = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            PathBuf::from(xdg)
        } else if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".config")
        } else {
            return Err(ConfigError::Io("no HOME or XDG_CONFIG_HOME set".into()));
        };
        Ok(base.join("inkapp").join("config.toml"))
    }

    /// The toml_edit table at this section path, if present.
    ///
    /// `Framework` sections live at the top level (`[<kind>]`, e.g. `[page]`) and
    /// intentionally ignore `instance` — there is one such table per kind, shared
    /// across instances. Callers may pass any instance string for a Framework kind.
    fn section(&self, ns: Namespace, kind: &str, instance: &str) -> Option<&Table> {
        let item = match ns {
            Namespace::Framework => self.doc.get(kind),
            Namespace::Connector => self
                .doc
                .get("connector")
                .and_then(Item::as_table)
                .and_then(|t| t.get(kind))
                .and_then(Item::as_table)
                .and_then(|t| t.get(instance)),
            Namespace::App => self
                .doc
                .get("app")
                .and_then(Item::as_table)
                .and_then(|t| t.get(kind))
                .and_then(Item::as_table)
                .and_then(|t| t.get(instance)),
        };
        item.and_then(Item::as_table)
    }

    /// Resolve a typed config section, applying defaults for absent keys.
    pub fn resolve<T: Config>(&self, instance: &str) -> Result<T> {
        let Some(table) = self.section(T::NAMESPACE, T::KIND, instance) else {
            return Ok(T::default()); // absent section → all defaults
        };
        // Unknown-key guard against the registered schema.
        if let Some(known) = Registry::field_names(T::NAMESPACE, T::KIND) {
            for (key, _) in table.iter() {
                if !known.contains(&key) {
                    return Err(ConfigError::UnknownKey {
                        section: format!("{}.{}.{}", T::NAMESPACE.as_str(), T::KIND, instance),
                        key: key.to_string(),
                        known: known.join(", "),
                    });
                }
            }
        }
        // Deserialize via the toml string of just this table.
        let toml_str = table.to_string();
        toml::from_str::<T>(&toml_str).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    /// Instance names configured under `[<namespace>.<kind>]`.
    pub fn instances(&self, ns: Namespace, kind: &str) -> Vec<String> {
        let prefix = ns.prefix();
        if prefix.is_empty() {
            return Vec::new();
        }
        self.doc
            .get(prefix)
            .and_then(Item::as_table)
            .and_then(|t| t.get(kind))
            .and_then(Item::as_table)
            .map(|t| t.iter().map(|(k, _)| k.to_string()).collect())
            .unwrap_or_default()
    }

    /// Error unless `[<namespace>.<kind>.<instance>]` exists (for ConnectorRef binding).
    pub fn require_instance(&self, ns: Namespace, kind: &str, instance: &str) -> Result<()> {
        if self.section(ns, kind, instance).is_some() {
            Ok(())
        } else {
            Err(ConfigError::NoSuchInstance {
                namespace: ns.as_str().to_string(),
                kind: kind.to_string(),
                instance: instance.to_string(),
                available: self.instances(ns, kind).join(", "),
            })
        }
    }
}

impl ConfigStore {
    /// Validate every present section whose kind is in the registry; unknown keys
    /// within a known section → error. Unknown sections (kinds not in this
    /// binary's registry) are ignored — another app may own them in the shared file.
    pub fn validate_known_sections(&self) -> Result<()> {
        for schema in Registry::all() {
            let ns = schema.namespace;
            let known: Vec<&str> = schema.fields.iter().map(|f| f.name).collect();
            let instances = if ns == Namespace::Framework {
                vec![String::new()]
            } else {
                self.instances(ns, schema.kind)
            };
            for inst in instances {
                if let Some(table) = self.section(ns, schema.kind, &inst) {
                    for (key, _) in table.iter() {
                        if !known.contains(&key) {
                            return Err(ConfigError::UnknownKey {
                                section: if inst.is_empty() {
                                    format!("{}.{}", ns.as_str(), schema.kind)
                                } else {
                                    format!("{}.{}.{}", ns.as_str(), schema.kind, inst)
                                },
                                key: key.to_string(),
                                known: known.join(", "),
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Read a dotted path (e.g. "connector.readwise.main.feed_enabled") as a string.
    pub fn get_raw(&self, dotted: &str) -> Option<String> {
        if dotted.is_empty() || dotted.split('.').any(|s| s.is_empty()) {
            return None;
        }
        let mut item = self.doc.as_item();
        for seg in dotted.split('.') {
            item = item.as_table_like()?.get(seg)?;
        }
        item.as_value().map(|v| v.to_string().trim().to_string())
    }

    /// Set a dotted path to a TOML-parsed value (falling back to a string),
    /// creating intermediate tables. Preserves surrounding formatting/comments.
    pub fn set_raw(&mut self, dotted: &str, value: &str) -> Result<()> {
        if dotted.is_empty() || dotted.split('.').any(|s| s.is_empty()) {
            return Err(ConfigError::Parse(format!(
                "invalid key path {dotted:?} (segments must be non-empty)"
            )));
        }
        let parsed: toml_edit::Value = value
            .parse()
            .or_else(|_| format!("\"{value}\"").parse())
            .map_err(|e: toml_edit::TomlError| ConfigError::Parse(e.to_string()))?;
        let segs: Vec<&str> = dotted.split('.').collect();
        let (last, parents) = segs
            .split_last()
            .ok_or_else(|| ConfigError::Parse("empty key".into()))?;
        let mut tbl = self.doc.as_table_mut();
        for seg in parents {
            tbl = tbl
                .entry(seg)
                .or_insert(Item::Table(Table::new()))
                .as_table_mut()
                .ok_or_else(|| ConfigError::Parse(format!("{seg} is not a table")))?;
        }
        tbl.insert(last, Item::Value(parsed));
        Ok(())
    }

    /// Persist the (possibly edited) document, creating the parent dir.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| ConfigError::Io(e.to_string()))?;
        }
        std::fs::write(&self.path, self.doc.to_string()).map_err(|e| ConfigError::Io(e.to_string()))
    }
}

/// Pick the launch instance: explicit arg > $INKAPP_INSTANCE > "default".
pub fn select_instance(explicit: Option<&str>) -> String {
    explicit
        .map(str::to_string)
        .or_else(|| std::env::var("INKAPP_INSTANCE").ok())
        .unwrap_or_else(|| "default".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Config;

    #[derive(serde::Deserialize, Config)]
    #[serde(default)]
    #[config(kind = "reader", namespace = "connector")]
    struct Reader {
        #[config(default = 100)]
        max: usize,
        enabled: bool,
    }

    /// Build a store from inline TOML. `ConfigStore::open` reads the file fully
    /// into memory, so the temp dir can drop immediately afterward (these tests
    /// never call a write-back path).
    fn store_from(toml: &str) -> ConfigStore {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, toml).unwrap();
        ConfigStore::open(path).unwrap()
    }

    #[test]
    fn missing_file_yields_defaults() {
        let s = ConfigStore::open("/nonexistent/inkapp/config.toml").unwrap();
        let r: Reader = s.resolve("main").unwrap();
        assert_eq!(r.max, 100);
        assert!(!r.enabled);
    }

    #[test]
    fn resolves_named_instance_with_defaults() {
        let s = store_from("[connector.reader.main]\nenabled = true\n");
        let r: Reader = s.resolve("main").unwrap();
        assert!(r.enabled);
        assert_eq!(r.max, 100);
    }

    #[test]
    fn unknown_key_in_known_section_errors() {
        let s = store_from("[connector.reader.main]\nbogus = 1\n");
        assert!(matches!(
            s.resolve::<Reader>("main"),
            Err(ConfigError::UnknownKey { .. })
        ));
    }

    #[test]
    fn unknown_section_is_ignored() {
        let s = store_from("[connector.other.main]\nx = 1\n");
        let r: Reader = s.resolve("main").unwrap(); // our section absent → defaults
        assert_eq!(r.max, 100);
    }

    #[test]
    fn instances_and_require() {
        let s = store_from("[connector.reader.work]\n[connector.reader.home]\n");
        let mut got = s.instances(Namespace::Connector, "reader");
        got.sort();
        assert_eq!(got, vec!["home", "work"]);
        assert!(s
            .require_instance(Namespace::Connector, "reader", "work")
            .is_ok());
        assert!(matches!(
            s.require_instance(Namespace::Connector, "reader", "nope"),
            Err(ConfigError::NoSuchInstance { .. })
        ));
    }

    #[test]
    fn select_instance_precedence() {
        std::env::remove_var("INKAPP_INSTANCE");
        assert_eq!(select_instance(Some("x")), "x");
        assert_eq!(select_instance(None), "default");
    }

    #[test]
    fn default_path_honors_env() {
        std::env::set_var("INKAPP_CONFIG_PATH", "/tmp/x/config.toml");
        assert_eq!(
            ConfigStore::default_path().unwrap(),
            std::path::PathBuf::from("/tmp/x/config.toml")
        );
        std::env::remove_var("INKAPP_CONFIG_PATH");
    }
}
