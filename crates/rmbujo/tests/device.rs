use rmbujo::device::{get_device, MOVE, PRO};

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn move_page_size() {
    let d = get_device("paper-pro-move").unwrap();
    assert!(approx(d.width_pt(), 260.18));
    assert!(approx(d.height_pt(), 462.55));
}

#[test]
fn pro_page_size() {
    let d = get_device("paper-pro").unwrap();
    assert!(approx(d.width_pt(), 509.34));
    assert!(approx(d.height_pt(), 679.13));
}

#[test]
fn unknown_device_errors() {
    assert!(get_device("nope").is_err());
}

#[test]
fn constants_present() {
    assert_eq!(MOVE.key, "paper-pro-move");
    assert_eq!(PRO.key, "paper-pro");
}
