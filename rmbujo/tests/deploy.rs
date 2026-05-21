use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use rmbujo::deploy::rmapi::{RmapiDeployer, RmapiRunner};
use rmbujo::deploy::Deployer;

/// Records the args of every rmapi call so tests can assert the sequence.
#[derive(Clone, Default)]
struct Recorder {
    calls: Rc<RefCell<Vec<Vec<String>>>>,
}
impl RmapiRunner for Recorder {
    fn run(&self, args: &[&str]) -> anyhow::Result<()> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        Ok(())
    }
}

#[test]
fn deploy_mkdirs_then_puts_each_pdf() {
    let rec = Recorder::default();
    let d = RmapiDeployer::new("/2026".into(), rec.clone());
    d.deploy(&[
        PathBuf::from("/out/2026.05 May.pdf"),
        PathBuf::from("/out/2026 Reference.pdf"),
    ])
    .unwrap();
    let c = rec.calls.borrow();
    assert_eq!(c[0], vec!["-ni", "mkdir", "/2026"]);
    assert_eq!(c[1], vec!["-ni", "put", "/out/2026.05 May.pdf", "/2026"]);
    assert_eq!(c[2], vec!["-ni", "put", "/out/2026 Reference.pdf", "/2026"]);
    assert_eq!(c.len(), 3);
}

#[test]
fn refresh_uses_content_only() {
    let rec = Recorder::default();
    let d = RmapiDeployer::new("/2026".into(), rec.clone());
    d.refresh(&[PathBuf::from("/out/2026.05 May.pdf")]).unwrap();
    let c = rec.calls.borrow();
    assert_eq!(
        c[0],
        vec![
            "-ni",
            "put",
            "--content-only",
            "/out/2026.05 May.pdf",
            "/2026"
        ]
    );
    assert_eq!(c.len(), 1);
}

#[test]
fn every_call_is_non_interactive() {
    let rec = Recorder::default();
    let d = RmapiDeployer::new("/2026".into(), rec.clone());
    d.deploy(&[PathBuf::from("/out/a.pdf")]).unwrap();
    d.refresh(&[PathBuf::from("/out/a.pdf")]).unwrap();
    for call in rec.calls.borrow().iter() {
        assert_eq!(call[0], "-ni", "every rmapi call must pass -ni");
    }
}
