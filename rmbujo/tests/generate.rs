use rmbujo::config::{Config, DeployConfig};
use rmbujo::deploy::{get_deployer, local::LocalDeployer};
use rmbujo::generate::generate_year;

fn tmp_dir() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    p.push(format!("rmbujo-gen-{n}"));
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn writes_15_named_pdfs() {
    let cfg = Config { daily_pages: 1, collection_pages: 1, ..Config::new(2026) };
    let dir = tmp_dir();
    let paths = generate_year(&cfg, &dir).unwrap();
    assert_eq!(paths.len(), 15);
    for f in [
        "2026 Future Log.pdf",
        "2026.05 May.pdf",
        "2026 Collection Template.pdf",
        "2026 Reference.pdf",
    ] {
        assert!(dir.join(f).exists(), "missing {f}");
    }
}

#[test]
fn deployer_none_ok_unknown_errs() {
    let _: LocalDeployer = LocalDeployer; // type exists
    assert!(get_deployer(&Config::new(2026)).is_ok());
    let bad = Config { deploy: DeployConfig { backend: "rmapi".into(), target_folder: String::new() }, ..Config::new(2026) };
    assert!(get_deployer(&bad).is_err());
}
