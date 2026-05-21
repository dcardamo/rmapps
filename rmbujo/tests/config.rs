use rmbujo::config::{self, Config, IcsFeed};

#[test]
fn validate_accepts_defaults() {
    assert!(Config::new(2026).validate().is_ok());
}

#[test]
fn validate_rejects_bad_fields() {
    assert!(Config {
        device: "nope".into(),
        ..Config::new(2026)
    }
    .validate()
    .is_err());
    assert!(Config {
        week_start: "xyz".into(),
        ..Config::new(2026)
    }
    .validate()
    .is_err());
    assert!(Config {
        theme: "nope".into(),
        ..Config::new(2026)
    }
    .validate()
    .is_err());
    assert!(Config {
        spacing_mm: 0.0,
        ..Config::new(2026)
    }
    .validate()
    .is_err());
    assert!(Config {
        spacing_mm: 50.0,
        ..Config::new(2026)
    }
    .validate()
    .is_err());
}

#[test]
fn round_trip() {
    let dir = tempdir();
    let cfg = Config {
        ics: vec![IcsFeed {
            name: "Holidays".into(),
            url: "https://x/h.ics".into(),
            color: "brick".into(),
        }],
        ..Config::new(2026)
    };
    let p = dir.join("rmbujo.toml");
    config::dump(&cfg, &p).unwrap();
    assert_eq!(config::load(&p).unwrap(), cfg);
}

#[test]
fn minimal_defaults() {
    let dir = tempdir();
    let p = dir.join("rmbujo.toml");
    std::fs::write(&p, "year = 2026\n").unwrap();
    let c = config::load(&p).unwrap();
    assert_eq!(c.device, "paper-pro-move");
    assert_eq!(c.week_start, "sun");
    assert_eq!(c.daily_pages, 60);
    assert_eq!(c.collection_pages, 20);
    assert_eq!(c.spacing_mm, 4.5);
    assert_eq!(c.theme, "library");
    assert!(c.ics.is_empty());
    assert_eq!(c.deploy.backend, "none");
}

#[test]
fn missing_year_errors() {
    let dir = tempdir();
    let p = dir.join("rmbujo.toml");
    std::fs::write(&p, "device = \"paper-pro\"\n").unwrap();
    assert!(config::load(&p).is_err());
}

#[test]
fn unknown_keys_ignored() {
    let dir = tempdir();
    let p = dir.join("rmbujo.toml");
    std::fs::write(&p, "year = 2026\nbogus = 1\n").unwrap();
    assert_eq!(config::load(&p).unwrap().year, 2026);
}

#[test]
fn validate_deploy_backend() {
    assert!(Config {
        deploy: config::DeployConfig {
            backend: "rmapi".into(),
            target_folder: "/2026".into(),
        },
        ..Config::new(2026)
    }
    .validate()
    .is_ok());
    assert!(Config {
        deploy: config::DeployConfig {
            backend: "ftp".into(),
            target_folder: "/2026".into(),
        },
        ..Config::new(2026)
    }
    .validate()
    .is_err());
}

// Minimal unique temp dir without an extra crate dependency.
fn tempdir() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!("rmbujo-test-{n}"));
    std::fs::create_dir_all(&p).unwrap();
    p
}
