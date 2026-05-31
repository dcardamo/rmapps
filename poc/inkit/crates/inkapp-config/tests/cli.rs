#![cfg(feature = "cli")]
use inkapp_config::cli::{run, ConfigCmd};
use inkapp_config::Config;

#[derive(serde::Deserialize, Config)]
#[serde(default)]
#[config(kind = "demo", namespace = "connector")]
#[allow(dead_code)]
struct Demo {
    /// the cap
    #[config(default = 5)]
    max: usize,
    token: inkapp_config::SecretRef,
}

#[test]
fn template_contains_section_and_defaults() {
    let tmpl = inkapp_config::cli::render_template_for_test();
    assert!(
        tmpl.contains("[connector.demo.<instance>]"),
        "tmpl was: {tmpl}"
    );
    assert!(tmpl.contains("max = 5"));
    assert!(tmpl.contains("# the cap"));
}

#[test]
fn set_then_get_roundtrips_and_preserves_comments() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "# keep me\n[connector.demo.main]\nmax = 5\n").unwrap();
    assert_eq!(
        run(
            ConfigCmd::Set {
                key: "connector.demo.main.max".into(),
                value: "9".into()
            },
            path.clone()
        )
        .unwrap(),
        0
    );
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(
        body.contains("# keep me"),
        "comment preserved; body: {body}"
    );
    assert!(body.contains("max = 9"), "body: {body}");
    // and Get reads it back
    assert_eq!(
        run(
            ConfigCmd::Get {
                key: "connector.demo.main.max".into()
            },
            path
        )
        .unwrap(),
        0
    );
}

#[test]
fn validate_flags_unknown_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[connector.demo.main]\nbogus = 1\n").unwrap();
    assert_eq!(run(ConfigCmd::Validate, path).unwrap(), 1);
}

#[test]
fn validate_passes_clean_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[connector.demo.main]\nmax = 7\n").unwrap();
    assert_eq!(run(ConfigCmd::Validate, path).unwrap(), 0);
}

#[test]
fn get_missing_key_returns_exit_1() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[connector.demo.main]\nmax = 5\n").unwrap();
    assert_eq!(
        run(
            ConfigCmd::Get {
                key: "connector.demo.main.nonexistent".into()
            },
            path
        )
        .unwrap(),
        1
    );
}
