use rmbujo::device::get_device;
use rmbujo::geometry::default_grid;

#[test]
fn move_grid() {
    let g = default_grid(&get_device("paper-pro-move").unwrap());
    // 4.756 mm at 72pt/inch → 13.48 pt; margin 6.0 mm → 17.01 pt (unchanged).
    assert!((g.spacing_pt - 13.48).abs() < 0.01);
    assert!((g.margin_pt - 17.01).abs() < 0.01);
    assert_eq!(g.cols, 17);
    assert_eq!(g.rows, 32);
}
