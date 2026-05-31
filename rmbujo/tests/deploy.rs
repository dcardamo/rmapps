use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rmbujo::config::Config;
use rmbujo::deploy::get_deployer;
use rmbujo::deploy::rmapi::{cloud_target, ProcessRmapi, RmapiDeployer, RmapiRunner};
use rmbujo::deploy::Deployer;

/// Records the args of every rmapi call so tests can assert the sequence.
#[derive(Clone, Default, Debug)]
struct Recorder {
    calls: Rc<RefCell<Vec<Vec<String>>>>,
    /// Cloud paths that the probe should report as already existing. Lets
    /// upsert tests exercise both the create and refresh branches without
    /// changing the run() behavior.
    existing: Rc<RefCell<std::collections::HashSet<String>>>,
}
impl RmapiRunner for Recorder {
    fn run(&self, args: &[&str]) -> anyhow::Result<()> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        Ok(())
    }
    fn exists(&self, path: &str) -> anyhow::Result<bool> {
        Ok(self.existing.borrow().contains(path))
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
fn deploy_creates_each_ancestor_folder() {
    let rec = Recorder::default();
    let d = RmapiDeployer::new("/rmbujo/2027".into(), rec.clone());
    d.deploy(&[PathBuf::from("/out/a.pdf")]).unwrap();
    let c = rec.calls.borrow();
    assert_eq!(c[0], vec!["-ni", "mkdir", "/rmbujo"]);
    assert_eq!(c[1], vec!["-ni", "mkdir", "/rmbujo/2027"]);
    assert_eq!(c[2], vec!["-ni", "put", "/out/a.pdf", "/rmbujo/2027"]);
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

// Unique temp dir without an extra crate (matches the project's test style).
fn tmp_dir() -> PathBuf {
    // Unique per call: a nanosecond timestamp can collide when two tests run in
    // the same instant under load, and a shared dir cross-contaminates the shim's
    // calls.log / clobber-trigger. A process-wide atomic counter guarantees no
    // two dirs collide regardless of clock resolution.
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let mut p = std::env::temp_dir();
    p.push(format!("rmbujo-deploy-{n}-{}-{seq}", std::process::id()));
    std::fs::create_dir_all(&p).unwrap();
    p
}

const GOOD_CONF: &str = "devicetoken: \"dev-abc\"\nusertoken: \"usr-xyz\"\n";

/// Write an executable `rmapi` shim into `dir`. It logs each call to
/// `dir/calls.log`. If `dir/clobber-trigger` exists, it truncates the conf named
/// in `dir/conf-path`, deletes the trigger, and exits 1 (simulating rmapi's
/// token-clobber-on-failure bug). Otherwise it exits 0.
fn write_shim(dir: &Path) -> PathBuf {
    let shim = dir.join("rmapi");
    std::fs::write(
        &shim,
        "#!/bin/sh\n\
         d=$(dirname \"$0\")\n\
         echo \"$*\" >> \"$d/calls.log\"\n\
         if [ -f \"$d/clobber-trigger\" ]; then\n\
         : > \"$(cat \"$d/conf-path\")\"\n\
         rm -f \"$d/clobber-trigger\"\n\
         exit 1\n\
         fi\n\
         exit 0\n",
    )
    .unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    shim
}

#[test]
fn process_rmapi_rejects_missing_binary() {
    let dir = tmp_dir();
    let conf = dir.join("rmapi.conf");
    std::fs::write(&conf, GOOD_CONF).unwrap();
    let err = ProcessRmapi::with(dir.join("does-not-exist"), conf).unwrap_err();
    assert!(err.to_string().contains("not"), "got: {err}");
}

#[test]
fn process_rmapi_rejects_unpaired_conf() {
    let dir = tmp_dir();
    let shim = write_shim(&dir);
    let conf = dir.join("rmapi.conf");
    std::fs::write(&conf, "devicetoken: \"\"\nusertoken: \"\"\n").unwrap();
    let err = ProcessRmapi::with(shim, conf).unwrap_err();
    assert!(err.to_string().contains("pair"), "got: {err}");
}

/// Run the shim, retrying on ETXTBSY ("Text file busy", os error 26): under
/// parallel test load another thread's fork can transiently hold a write fd to a
/// just-written shim, making exec fail. Retry until it clears.
fn run_ok(r: &ProcessRmapi, args: &[&str]) {
    for _ in 0..200 {
        match r.run(args) {
            Ok(()) => return,
            Err(e) => {
                if e.downcast_ref::<std::io::Error>()
                    .and_then(|io| io.raw_os_error())
                    == Some(26)
                {
                    std::thread::yield_now();
                    continue;
                }
                panic!("rmapi run failed: {e:?}");
            }
        }
    }
    panic!("rmapi run kept hitting ETXTBSY (os error 26)");
}

#[test]
fn process_rmapi_runs_and_logs() {
    let dir = tmp_dir();
    let shim = write_shim(&dir);
    let conf = dir.join("rmapi.conf");
    std::fs::write(&conf, GOOD_CONF).unwrap();
    let r = ProcessRmapi::with(shim, conf).unwrap();
    run_ok(&r, &["-ni", "put", "/out/a.pdf", "/2026"]);
    let log = std::fs::read_to_string(dir.join("calls.log")).unwrap();
    assert_eq!(log.trim(), "-ni put /out/a.pdf /2026");
}

#[test]
fn process_rmapi_restores_clobbered_conf_and_retries() {
    let dir = tmp_dir();
    let shim = write_shim(&dir);
    let conf = dir.join("rmapi.conf");
    std::fs::write(&conf, GOOD_CONF).unwrap();
    // Arm the shim to clobber the conf + fail on its first call.
    std::fs::write(dir.join("conf-path"), conf.to_str().unwrap()).unwrap();
    std::fs::write(dir.join("clobber-trigger"), "").unwrap();

    let r = ProcessRmapi::with(shim, conf.clone()).unwrap();
    run_ok(&r, &["-ni", "put", "/out/a.pdf", "/2026"]);

    // Conf was restored to the good snapshot, and the call was retried (2 lines).
    assert_eq!(std::fs::read_to_string(&conf).unwrap(), GOOD_CONF);
    let log = std::fs::read_to_string(dir.join("calls.log")).unwrap();
    assert_eq!(log.lines().count(), 2);
}

#[test]
fn get_deployer_routes_backends() {
    // none → ok
    assert!(get_deployer(&Config::new(2026)).is_ok());
    // unknown → err
    let bogus = Config {
        deploy: rmbujo::config::DeployConfig {
            backend: "bogus".into(),
            base_folder: "/rmbujo".into(),
        },
        ..Config::new(2026)
    };
    assert!(get_deployer(&bogus).is_err());
    // rmapi with empty base_folder → err before any rmapi preflight
    let no_folder = Config {
        deploy: rmbujo::config::DeployConfig {
            backend: "rmapi".into(),
            base_folder: "  ".into(),
        },
        ..Config::new(2026)
    };
    let err = get_deployer(&no_folder).unwrap_err();
    assert!(err.to_string().contains("base_folder"), "got: {err}");
}

#[test]
fn upsert_creates_when_missing() {
    let rec = Recorder::default();
    let d = RmapiDeployer::new("/".into(), rec.clone());
    d.upsert(&[PathBuf::from("/out/2026.05 May.pdf")]).unwrap();
    let c = rec.calls.borrow();
    // No mkdir for "/" (folder_chain is empty), then a plain put.
    assert_eq!(c[0], vec!["-ni", "put", "/out/2026.05 May.pdf", "/"]);
    assert_eq!(c.len(), 1);
}

#[test]
fn upsert_refreshes_when_present() {
    let rec = Recorder::default();
    rec.existing.borrow_mut().insert("/2026.05 May".to_string());
    let d = RmapiDeployer::new("/".into(), rec.clone());
    d.upsert(&[PathBuf::from("/out/2026.05 May.pdf")]).unwrap();
    let c = rec.calls.borrow();
    assert_eq!(
        c[0],
        vec!["-ni", "put", "--content-only", "/out/2026.05 May.pdf", "/"]
    );
    assert_eq!(c.len(), 1, "mkdir must not run when refreshing");
}

#[test]
fn upsert_nested_target_mkdir_then_put_when_missing() {
    let rec = Recorder::default();
    let d = RmapiDeployer::new("/rmbujo/2026".into(), rec.clone());
    d.upsert(&[PathBuf::from("/out/2026.05 May.pdf")]).unwrap();
    let c = rec.calls.borrow();
    assert_eq!(c[0], vec!["-ni", "mkdir", "/rmbujo"]);
    assert_eq!(c[1], vec!["-ni", "mkdir", "/rmbujo/2026"]);
    assert_eq!(
        c[2],
        vec!["-ni", "put", "/out/2026.05 May.pdf", "/rmbujo/2026"]
    );
    assert_eq!(c.len(), 3);
}

#[test]
fn upsert_mkdir_runs_only_once_across_mixed_paths() {
    let rec = Recorder::default();
    // First doc exists (refresh, no mkdir), second doesn't (mkdir, then put).
    rec.existing
        .borrow_mut()
        .insert("/rmbujo/2026/2026.05 May".to_string());
    let d = RmapiDeployer::new("/rmbujo/2026".into(), rec.clone());
    d.upsert(&[
        PathBuf::from("/out/2026.05 May.pdf"),
        PathBuf::from("/out/2026.06 June.pdf"),
    ])
    .unwrap();
    let c = rec.calls.borrow();
    // Refresh of May — no mkdir yet.
    assert_eq!(
        c[0],
        vec![
            "-ni",
            "put",
            "--content-only",
            "/out/2026.05 May.pdf",
            "/rmbujo/2026"
        ]
    );
    // Now June is missing → mkdir chain runs, then plain put.
    assert_eq!(c[1], vec!["-ni", "mkdir", "/rmbujo"]);
    assert_eq!(c[2], vec!["-ni", "mkdir", "/rmbujo/2026"]);
    assert_eq!(
        c[3],
        vec!["-ni", "put", "/out/2026.06 June.pdf", "/rmbujo/2026"]
    );
    assert_eq!(c.len(), 4);
}

#[test]
fn create_if_missing_skips_existing_and_puts_only_absent() {
    // May already exists; the two extras do not. create_if_missing must NOT
    // touch May at all, and must `put` (plain, never content-only) only the
    // absent extras — so existing on-device docs are never re-pushed.
    let rec = Recorder::default();
    rec.existing
        .borrow_mut()
        .insert("/2026/2026.05 May".to_string());
    let d = RmapiDeployer::new("/2026".into(), rec.clone());
    d.create_if_missing(&[
        PathBuf::from("/out/2026.05 May.pdf"),
        PathBuf::from("/out/2026 Future Log.pdf"),
        PathBuf::from("/out/2026 Reference.pdf"),
    ])
    .unwrap();
    let c = rec.calls.borrow();
    let puts: Vec<&Vec<String>> = c
        .iter()
        .filter(|a| a.contains(&"put".to_string()))
        .collect();
    // Exactly the two missing extras get a plain put; none is content-only.
    assert_eq!(puts.len(), 2, "calls: {c:?}");
    assert!(puts
        .iter()
        .all(|a| !a.contains(&"--content-only".to_string())));
    assert!(puts
        .iter()
        .any(|a| a.iter().any(|s| s.contains("Future Log"))));
    assert!(puts
        .iter()
        .any(|a| a.iter().any(|s| s.contains("Reference"))));
    // The existing May is never uploaded (no put referencing it).
    assert!(
        !c.iter()
            .any(|a| a.contains(&"put".to_string()) && a.iter().any(|s| s.contains("2026.05 May"))),
        "existing May must not be re-pushed; calls: {c:?}"
    );
}

#[test]
fn create_if_missing_all_present_does_nothing() {
    let rec = Recorder::default();
    rec.existing
        .borrow_mut()
        .insert("/2026/2026 Future Log".to_string());
    let d = RmapiDeployer::new("/2026".into(), rec.clone());
    d.create_if_missing(&[PathBuf::from("/out/2026 Future Log.pdf")])
        .unwrap();
    // Nothing absent → no put, no mkdir.
    assert!(
        rec.calls.borrow().is_empty(),
        "calls: {:?}",
        rec.calls.borrow()
    );
}

#[test]
fn cloud_target_normalizes_base() {
    assert_eq!(cloud_target("/rmbujo", 2026), "/rmbujo/2026");
    assert_eq!(cloud_target("rmbujo", 2026), "/rmbujo/2026"); // no leading slash
    assert_eq!(cloud_target("rmbujo/", 2026), "/rmbujo/2026"); // trailing slash
    assert_eq!(cloud_target("  /rmbujo  ", 2026), "/rmbujo/2026"); // whitespace
    assert_eq!(cloud_target("/journals/bujo", 2026), "/journals/bujo/2026"); // nested
    assert_eq!(cloud_target("/", 2026), "/2026");
}
