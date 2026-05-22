use rmbujo::device::get_device;
use rmbujo::geometry::{default_grid, monthly_row_pt, TOOLBAR_SAFE_PT};

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
fn monthly_row_fits_under_reserve() {
    let dev = get_device("paper-pro-move").unwrap();
    let grid = default_grid(&dev);
    let header = 22.0;
    let row = monthly_row_pt(&dev, TOOLBAR_SAFE_PT, header, grid.margin_pt, 31);
    assert!(row > 0.0 && row <= grid.spacing_pt + 0.001);
    assert!(31.0 * row + header + TOOLBAR_SAFE_PT + grid.margin_pt <= dev.height_pt() + 0.001);
}
