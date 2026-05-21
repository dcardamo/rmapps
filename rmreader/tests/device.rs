use rmreader::device::get_device;

#[test]
fn move_geometry_points() {
    let d = get_device("paper-pro-move").unwrap();
    // 954/264*72 ≈ 260.18, 1696/264*72 ≈ 462.55
    assert!((d.width_pt() - 260.18).abs() < 0.1);
    assert!((d.height_pt() - 462.55).abs() < 0.1);
}

#[test]
fn unknown_device_errs() {
    assert!(get_device("nope").is_err());
}
