#[test]
fn help_exits_ok() {
    // run() with --help calls e.exit() (exits process); instead test no-arg help path.
    let r = rmreader::cli::run(vec!["rmreader".into()]);
    assert!(r.is_ok());
}
