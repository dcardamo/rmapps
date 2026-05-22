use rmreader::deploy::rmapi::{RmapiDeployer, RmapiRunner};
use rmreader::deploy::Deployer;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

// ---------------------------------------------------------------------------
// SharedRunner: basic fake that records all `run` calls and returns Ok(()).
// `try_run_in` records args (prefixed with the dir) and returns `get_succeeds`.
// ---------------------------------------------------------------------------

#[derive(Debug, Default)]
struct SharedRunner {
    calls: Rc<RefCell<Vec<Vec<String>>>>,
    /// Returned by `try_run_in` to simulate found / not-found.
    get_succeeds: bool,
}

impl RmapiRunner for SharedRunner {
    fn run(&self, args: &[&str]) -> anyhow::Result<()> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        Ok(())
    }

    fn try_run_in(&self, _dir: &Path, args: &[&str]) -> anyhow::Result<bool> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        Ok(self.get_succeeds)
    }
}

// ---------------------------------------------------------------------------
// WritingRunner: fake whose `try_run_in` actually writes the expected .rmdoc
// file into `dir` so that `fetch` can find it.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct WritingRunner {
    calls: Rc<RefCell<Vec<Vec<String>>>>,
}

impl RmapiRunner for WritingRunner {
    fn run(&self, args: &[&str]) -> anyhow::Result<()> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        Ok(())
    }

    fn try_run_in(&self, dir: &Path, args: &[&str]) -> anyhow::Result<bool> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        // Derive the document name from the last arg (e.g. "/RMDev/Reader/Library").
        if let Some(remote) = args.last() {
            let name = remote.split('/').next_back().unwrap_or("doc");
            std::fs::write(dir.join(format!("{name}.rmdoc")), b"fake bundle")?;
        }
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn deploy_then_refresh_sequences() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let d = RmapiDeployer::new(SharedRunner {
        calls: calls.clone(),
        get_succeeds: true,
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

#[test]
fn replace_removes_then_puts() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let d = RmapiDeployer::new(SharedRunner {
        calls: calls.clone(),
        get_succeeds: true,
    });
    d.replace(Path::new("/o/Library.pdf"), "/RMDev/Reader")
        .unwrap();
    let c = calls.borrow();
    assert_eq!(c[0], vec!["-ni", "rm", "/RMDev/Reader/Library"]);
    assert_eq!(c[1], vec!["-ni", "put", "/o/Library.pdf", "/RMDev/Reader"]);
}

#[test]
fn fetch_missing_returns_none() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let d = RmapiDeployer::new(SharedRunner {
        calls: calls.clone(),
        get_succeeds: false,
    });
    let result = d.fetch("/RMDev/Reader", "Library").unwrap();
    assert!(result.is_none());
    let c = calls.borrow();
    assert_eq!(c[0], vec!["-ni", "get", "/RMDev/Reader/Library"]);
}

#[test]
fn fetch_success_returns_path() {
    let calls = Rc::new(RefCell::new(Vec::new()));
    let d = RmapiDeployer::new(WritingRunner {
        calls: calls.clone(),
    });
    let result = d.fetch("/RMDev/Reader", "Library").unwrap();
    assert!(result.is_some(), "expected Some(path), got None");
    let path = result.unwrap();
    assert!(
        path.exists(),
        "returned path does not exist: {}",
        path.display()
    );
    assert!(
        path.file_name().and_then(|n| n.to_str()).unwrap_or("") == "rmreader-Library.rmdoc",
        "unexpected filename: {}",
        path.display()
    );
    let c = calls.borrow();
    assert_eq!(c[0], vec!["-ni", "get", "/RMDev/Reader/Library"]);
}
