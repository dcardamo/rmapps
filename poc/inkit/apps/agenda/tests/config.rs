//! Config-driven wiring tests for the agenda app: both connector bindings are
//! resolved from `config.toml`, and a missing binding errors with attribution.

use agenda::{AppConfig, Connectors};
use inkapp::ConfigStore;

#[test]
fn wires_both_connectors_from_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let cal_path = dir.path().join("cal.json");
    std::fs::write(
        &path,
        format!(
            "[app.agenda.default]\ndevice_folder=\"/A\"\nfeed=\"ics.work\"\ncal=\"localcal.work\"\n\
             [connector.ics.work]\nurl=\"https://example.com/x.ics\"\n\
             [connector.localcal.work]\nstore_path=\"{}\"\n",
            cal_path.display()
        ),
    )
    .unwrap();
    let store = ConfigStore::open(&path).unwrap();
    let app_cfg: AppConfig = store.resolve("default").unwrap();
    assert_eq!(app_cfg.feed.instance, "work");
    let conn = Connectors::from_config(&store, &app_cfg).expect("wire");
    // LocalCal::from_config seeds sample events at construction; the ICS connector
    // starts with an empty cache (its URL is only fetched on refresh).
    assert!(!conn.cal.events().is_empty());
    assert!(conn.feed.events().is_empty());
}

#[test]
fn missing_bound_instance_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[app.agenda.default]\nfeed=\"ics.nope\"\ncal=\"localcal.nope\"\n",
    )
    .unwrap();
    let store = ConfigStore::open(&path).unwrap();
    let app_cfg: AppConfig = store.resolve("default").unwrap();
    // `Connectors` is not `Debug`, so use `.err().unwrap()` rather than `.unwrap_err()`.
    let err = Connectors::from_config(&store, &app_cfg)
        .err()
        .expect("expected an error");
    assert!(
        matches!(err, inkapp_config::ConfigError::NoSuchInstance { .. }),
        "expected NoSuchInstance, got: {err}"
    );
}

#[test]
fn missing_feed_instance_errors_with_attribution() {
    // Only the feed binding is missing; cal is valid. The error must name ics.nope.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let cal_path = dir.path().join("cal.json");
    std::fs::write(
        &path,
        format!(
            "[app.agenda.default]\nfeed=\"ics.nope\"\ncal=\"localcal.work\"\n\
             [connector.localcal.work]\nstore_path=\"{}\"\n",
            cal_path.display()
        ),
    )
    .unwrap();
    let store = ConfigStore::open(&path).unwrap();
    let app_cfg: AppConfig = store.resolve("default").unwrap();
    // `Connectors` is not `Debug`, so use `.err().unwrap()` rather than `.unwrap_err()`.
    let err = Connectors::from_config(&store, &app_cfg)
        .err()
        .expect("expected an error");
    assert!(
        matches!(
            err,
            inkapp_config::ConfigError::NoSuchInstance { ref kind, ref instance, .. }
                if kind == "ics" && instance == "nope"
        ),
        "expected NoSuchInstance for ics.nope, got: {err}"
    );
}
