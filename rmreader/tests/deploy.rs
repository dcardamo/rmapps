use rmreader::deploy::rmapi::{RmapiDeployer, RmapiRunner};
use rmreader::deploy::Deployer;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Debug, Default)]
struct SharedRunner {
    calls: Rc<RefCell<Vec<Vec<String>>>>,
}

impl RmapiRunner for SharedRunner {
    fn run(&self, args: &[&str]) -> anyhow::Result<()> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        Ok(())
    }
}

#[test]
fn deploy_then_refresh_sequences() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let d = RmapiDeployer::new(SharedRunner {
        calls: calls.clone(),
    });
    let targets = vec![
        (PathBuf::from("/o/Library.pdf"), "/Reader".to_string()),
        (PathBuf::from("/o/Feed.pdf"), "/Feeds".to_string()),
    ];
    d.deploy(&targets).unwrap();
    d.refresh(&targets).unwrap();
    let c = calls.borrow();
    assert_eq!(c[0], vec!["-ni", "mkdir", "/Reader"]);
    assert_eq!(c[1], vec!["-ni", "put", "/o/Library.pdf", "/Reader"]);
    assert_eq!(c[2], vec!["-ni", "mkdir", "/Feeds"]);
    assert_eq!(c[3], vec!["-ni", "put", "/o/Feed.pdf", "/Feeds"]);
    assert_eq!(
        c[4],
        vec!["-ni", "put", "--content-only", "/o/Library.pdf", "/Reader"]
    );
    assert_eq!(
        c[5],
        vec!["-ni", "put", "--content-only", "/o/Feed.pdf", "/Feeds"]
    );
}
