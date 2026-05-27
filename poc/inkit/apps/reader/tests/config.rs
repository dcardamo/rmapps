//! Reader AppConfig + DeviceConfig + PageConfig resolve from a config file.

use inkapp_config::ConfigStore;
use reader::AppConfig;
use std::fs::write;

const SAMPLE: &str = r#"
[device]
backend = "remarkable"
sync_interval_secs = 30

[page]
width = 420.0
height = 560.0
margin = 16.0

[connector.readwise.main]
token = "readwise"
library_locations = ["new"]
library_max = 10
feed_enabled = false
feed_max = 100

[app.reader.default]
device_folder = "/Reader"
readwise = "readwise.main"
"#;

#[test]
fn config_resolves_with_reader_section() {
    let td = tempfile::tempdir().unwrap();
    let path = td.path().join("config.toml");
    write(&path, SAMPLE).unwrap();
    let store = ConfigStore::open(&path).unwrap();
    let cfg: AppConfig = store.resolve("default").unwrap();
    assert_eq!(cfg.device_folder, "/Reader");
    assert_eq!(cfg.readwise.kind, "readwise");
    assert_eq!(cfg.readwise.instance, "main");
}

#[test]
fn config_uses_defaults_when_section_omitted() {
    // Without [app.reader.<instance>], resolve falls back to derive-default values.
    let td = tempfile::tempdir().unwrap();
    let path = td.path().join("config.toml");
    write(
        &path,
        r#"
[device]
backend = "remarkable"
sync_interval_secs = 30

[page]
width = 420.0
height = 560.0
margin = 16.0

[connector.readwise.main]
token = "readwise"
"#,
    )
    .unwrap();
    let store = ConfigStore::open(&path).unwrap();
    let cfg: AppConfig = store.resolve("default").unwrap_or_default();
    assert_eq!(cfg.device_folder, "/Reader");
}
