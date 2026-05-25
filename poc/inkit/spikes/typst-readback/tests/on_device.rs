use std::path::Path;

#[test]
#[ignore = "requires a paired reMarkable; run manually: cargo test -p typst-readback --test on_device -- --ignored --nocapture"]
fn pushes_sample_doc_for_visual_check() {
    let pdf = typst_readback::compile_pdf("= inkapp spike\n\nWrite on me with the pen.").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("inkapp-spike.pdf");
    std::fs::write(&path, &pdf).unwrap();
    typst_readback::cloud::push_content_only(Path::new(&path), "/inkapp-spike").unwrap();
    eprintln!("pushed to /inkapp-spike — inspect quality on the tablet");
}
