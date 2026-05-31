use rmbujo::device::get_device;
use rmbujo::geometry::{default_grid, monthly_row_center, TOOLBAR_SAFE_PT};

#[test]
fn move_grid() {
    let g = default_grid(&get_device("paper-pro-move").unwrap());
    // 4.756 mm at 72pt/inch → 13.48 pt; margin 6.0 mm → 17.01 pt (unchanged).
    assert!((g.spacing_pt - 13.48).abs() < 0.01);
    assert!((g.margin_pt - 17.01).abs() < 0.01);
    assert_eq!(g.cols, 17);
    assert_eq!(g.rows, 32);
}

#[test]
fn monthly_rows_sit_on_grid_and_fit() {
    // Each day is centred on a real dot-row centre (DOT_CENTER_Y0 + k·sp), so
    // consecutive days are exactly one dot pitch apart — never sub-pitch.
    let dev = get_device("paper-pro-move").unwrap();
    let g = default_grid(&dev);
    let first = monthly_row_center(g.spacing_pt, 0);
    let second = monthly_row_center(g.spacing_pt, 1);
    assert!((second - first - g.spacing_pt).abs() < 1e-3);

    // Day 1 clears the toolbar-safe band and day 31's row stays on the page.
    assert!(first >= TOOLBAR_SAFE_PT);
    let last = monthly_row_center(g.spacing_pt, 30);
    assert!(last + g.spacing_pt / 2.0 <= dev.height_pt() + 1e-3);
}
