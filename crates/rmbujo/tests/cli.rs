use rmbujo::cli::run;
use rmbujo::config::{self, Config};
use rmbujo::wizard::{assemble, Answers};

fn tmp_dir() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!("rmbujo-cli-{n}"));
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn wizard_assemble() {
    let base = tmp_dir();
    let (config, out_dir, config_path) = assemble(Answers {
        year: 2026,
        base: base.to_string_lossy().into_owned(),
        device: "paper-pro-move".into(),
        week_start: "sun".into(),
        pages_per_day: 2,
        collection_pages: 2,
        spacing_mm: 4.5,
        theme: "library".into(),
        deploy_backend: "rmapi".into(),
        base_folder: "/2026".into(),
        timezone: "America/Toronto".into(),
        ics: vec![],
    });
    assert_eq!(config.year, 2026);
    assert_eq!(config.pages_per_day, 2);
    assert_eq!(config.spacing_mm, 4.5);
    assert_eq!(config.deploy.backend, "rmapi");
    assert_eq!(config.deploy.base_folder, "/2026");
    assert_eq!(out_dir, base.join("2026"));
    assert_eq!(config_path, base.join("2026").join("rmbujo.toml"));
}

#[test]
fn regenerate_from_config() {
    let dir = tmp_dir().join("2026");
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = Config {
        collection_pages: 1,
        ..Config::new(2026)
    };
    config::dump(&cfg, &dir.join("rmbujo.toml")).unwrap();

    run(vec![
        "rmbujo".into(),
        dir.join("rmbujo.toml").to_string_lossy().into_owned(),
    ])
    .unwrap();

    assert!(dir.join("2026.05 May.pdf").exists());
    assert!(dir.join("2026 Reference.pdf").exists());
}

#[test]
fn refresh_feeds_flag_parses() {
    let dir = tmp_dir().join("2026");
    std::fs::create_dir_all(&dir).unwrap();
    let cfg = Config::new(2026);
    config::dump(&cfg, &dir.join("rmbujo.toml")).unwrap();
    run(vec![
        "rmbujo".into(),
        dir.join("rmbujo.toml").to_string_lossy().into_owned(),
        "--refresh-feeds".into(),
    ])
    .unwrap();
    assert!(dir.join("2026.05 May.pdf").exists());
}
