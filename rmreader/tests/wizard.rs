use rmreader::wizard::{assemble, Answers};

#[test]
fn assemble_builds_valid_config() {
    let (cfg, out_dir, cfg_path) = assemble(Answers {
        output_dir: "/tmp/reader".into(),
        device: "paper-pro-move".into(),
        token: "tok".into(),
        library_locations: vec!["new".into(), "later".into(), "shortlist".into()],
        library_max: 100,
        feed_enabled: true,
        feed_max: 100,
        images_enabled: true,
        deploy_backend: "rmapi".into(),
        library_folder: "/Reader".into(),
        feed_folder: "/Reader".into(),
    });
    assert!(cfg.validate().is_ok());
    assert_eq!(out_dir.to_str().unwrap(), "/tmp/reader");
    assert_eq!(cfg_path.to_str().unwrap(), "/tmp/reader/rmreader.toml");
}
