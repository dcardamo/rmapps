use rmbujo::device::get_device;
use rmbujo::geometry::default_grid;

#[test]
fn move_grid() {
    let g = default_grid(&get_device("paper-pro-move").unwrap());
    assert!((g.spacing_pt - 12.76).abs() < 0.01);
    assert!((g.margin_pt - 17.01).abs() < 0.01);
    assert_eq!(g.cols, 18);
    assert_eq!(g.rows, 34);
}
