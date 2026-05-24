use inkapp_core::calendar::EventRow;
use inkapp_core::mode::Mode;

#[test]
fn mode_is_copy_and_comparable() {
    let m = Mode::Editable;
    let n = m; // requires Copy
    assert_eq!(m, n);
    assert_ne!(Mode::ReadOnly, Mode::Editable);
}

#[test]
fn event_row_constructs_and_compares() {
    let a = EventRow {
        uid: "e1".into(),
        summary: "Standup".into(),
        start: "20260525T090000Z".into(),
        end: "20260525T091500Z".into(),
        cancelled: false,
    };
    let b = a.clone();
    assert_eq!(a, b);
    assert!(!a.cancelled);
}
