use inkapp::DeployConfig;

#[test]
fn parses_explicit_backend_and_folder() {
    let cfg =
        DeployConfig::from_toml("backend = \"remarkable\"\nfolder = \"/ReadingQueue\"").unwrap();
    assert_eq!(cfg.backend, "remarkable");
    assert_eq!(cfg.folder, "/ReadingQueue");
}

#[test]
fn backend_defaults_to_remarkable() {
    let cfg = DeployConfig::from_toml("folder = \"/Agenda\"").unwrap();
    assert_eq!(cfg.backend, "remarkable");
}

#[test]
fn missing_folder_is_an_error() {
    assert!(DeployConfig::from_toml("backend = \"remarkable\"").is_err());
}
