//! Deploy backend: list cloud docs, fetch bundles, upload digest PDFs.
//!
//! Two backends:
//! - [`RmapiBackend`]: talks to the reMarkable cloud via `rmapi`.
//! - [`LocalBackend`]: writes PDFs to a local directory (used by `--local` and tests).
//!
//! # rmapi version / cheap-skip
//!
//! `rmapi stat` returns a JSON blob with a `Version` field, but in practice it
//! is always `0` (rmapi's Go client does not populate it from the v1.5 API).
//! `ModifiedClient` is the only reliable per-doc token, but obtaining it requires
//! a separate `stat` call per document — making it as expensive as a fetch.  For
//! now `CloudDoc::version` is always `None`; the orchestration layer falls back to
//! always-fetch (correctness preserved, just no cheap skip).  If rmapi gains a
//! working version field, wire it through `run_capture` + `stat` parsing here.

use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A document discovered in a watched cloud path.
#[derive(Debug, Clone)]
pub struct CloudDoc {
    /// Full cloud path, e.g. `"/Books/Purchased/kobo/Author/Title"`.
    pub path: String,
    /// Visible name (leaf).
    pub name: String,
    /// Parent folder, e.g. `"/Books/Purchased/kobo/Author"`.
    pub folder: String,
    /// Opaque version/hash for the cheap skip; `None` if rmapi can't provide one.
    /// Currently always `None` — see module-level doc comment.
    pub version: Option<String>,
}

/// Backend-agnostic interface for listing, fetching, and uploading docs.
pub trait Backend {
    /// Recursively list candidate source docs under `root`, excluding any whose
    /// visible name ends with one of `exclude_suffixes` (the digest suffixes).
    fn list(&self, root: &str, exclude_suffixes: &[String]) -> anyhow::Result<Vec<CloudDoc>>;

    /// Download a doc's bundle; returns the local `.rmdoc` path, or `None` if not found.
    fn fetch(&self, doc: &CloudDoc) -> anyhow::Result<Option<PathBuf>>;

    /// Upload `pdf` into `folder` under visible name `name`.
    ///
    /// Uses destructive replace: `rmapi rm <folder>/<name>` (ignored if absent),
    /// then `rmapi put`.  Safe for digests because they are write-only generated
    /// docs — no handwriting to preserve.
    fn put(&self, pdf: &Path, folder: &str, name: &str) -> anyhow::Result<()>;
}

// ---------------------------------------------------------------------------
// RmapiRunner trait
// ---------------------------------------------------------------------------

/// Runs `rmapi` subcommands.  Abstracted so sequences are unit-testable without
/// shelling out to the real binary.
pub trait RmapiRunner: std::fmt::Debug {
    /// Run `rmapi <args>`; `args` never includes the binary name.
    fn run(&self, args: &[&str]) -> anyhow::Result<()>;

    /// Run `rmapi <args>` with `dir` as the working directory.  Returns `Ok(true)`
    /// on success, `Ok(false)` on a clean non-zero exit (e.g. doc not found).
    /// `Err` only on failure to spawn.
    fn try_run_in(&self, dir: &Path, args: &[&str]) -> anyhow::Result<bool>;

    /// Run `rmapi <args>` and capture stdout.  Returns the stdout string.
    fn run_capture(&self, args: &[&str]) -> anyhow::Result<String>;
}

// ---------------------------------------------------------------------------
// RmapiBackend
// ---------------------------------------------------------------------------

/// Lists / fetches / uploads via [`RmapiRunner`].
#[derive(Debug)]
pub struct RmapiBackend<R: RmapiRunner> {
    runner: R,
}

impl<R: RmapiRunner> RmapiBackend<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    /// Idempotently create every ancestor of `folder` (mkdir -p semantics).
    /// rmapi `mkdir` errors on an existing dir; we ignore those.
    fn mkdir_p(&self, folder: &str) {
        let mut path = String::new();
        for comp in folder
            .trim_matches('/')
            .split('/')
            .filter(|c| !c.is_empty())
        {
            path.push('/');
            path.push_str(comp);
            let _ = self.runner.run(&["-ni", "mkdir", &path]);
        }
    }

    /// Recursively walk `dir_path`, collecting docs whose names do not end with
    /// any of `exclude_suffixes`.  Calls `rmapi ls <dir_path>` and recurses into
    /// sub-directories.
    fn walk(
        &self,
        dir_path: &str,
        exclude_suffixes: &[String],
        out: &mut Vec<CloudDoc>,
    ) -> anyhow::Result<()> {
        let stdout = self.runner.run_capture(&["-ni", "ls", dir_path])?;
        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // rmapi ls format: "[d]\tname" for directories, "[f]\tname" for files.
            let (kind, name) = if let Some(rest) = line.strip_prefix("[d]\t") {
                ("d", rest)
            } else if let Some(rest) = line.strip_prefix("[f]\t") {
                ("f", rest)
            } else {
                // Unexpected format — skip.
                continue;
            };

            let child_path = if dir_path.ends_with('/') {
                format!("{}{}", dir_path, name)
            } else {
                format!("{}/{}", dir_path, name)
            };

            if kind == "d" {
                self.walk(&child_path, exclude_suffixes, out)?;
            } else {
                // Filter out generated digest docs.
                let excluded = exclude_suffixes
                    .iter()
                    .any(|suf| name.ends_with(suf.as_str()));
                if !excluded {
                    out.push(CloudDoc {
                        path: child_path,
                        name: name.to_string(),
                        folder: dir_path.to_string(),
                        version: None,
                    });
                }
            }
        }
        Ok(())
    }
}

impl<R: RmapiRunner> Backend for RmapiBackend<R> {
    fn list(&self, root: &str, exclude_suffixes: &[String]) -> anyhow::Result<Vec<CloudDoc>> {
        let mut docs = Vec::new();
        self.walk(root, exclude_suffixes, &mut docs)?;
        Ok(docs)
    }

    fn fetch(&self, doc: &CloudDoc) -> anyhow::Result<Option<PathBuf>> {
        let tmp = tempfile::tempdir()?;
        let ok = self
            .runner
            .try_run_in(tmp.path(), &["-ni", "get", &doc.path])?;
        if !ok {
            return Ok(None);
        }
        let produced = tmp.path().join(format!("{}.rmdoc", doc.name));
        if !produced.exists() {
            return Ok(None);
        }
        let dest = std::env::temp_dir().join(format!("rmdigest-{}.rmdoc", doc.name));
        let _ = std::fs::remove_file(&dest);
        std::fs::rename(&produced, &dest)
            .or_else(|_| std::fs::copy(&produced, &dest).map(|_| ()))?;
        Ok(Some(dest))
    }

    fn put(&self, pdf: &Path, folder: &str, name: &str) -> anyhow::Result<()> {
        // Stage the PDF under `name.pdf` in a tempdir so rmapi derives the right
        // visible name from the file stem.
        let stage = tempfile::tempdir()?;
        let staged = stage.path().join(format!("{}.pdf", name));
        std::fs::copy(pdf, &staged)?;

        self.mkdir_p(folder);

        // Best-effort remove: a missing doc is fine.
        let _ = self
            .runner
            .run(&["-ni", "rm", &format!("{}/{}", folder, name)]);

        let staged_str = staged
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("non-UTF-8 staged path"))?;
        self.runner.run(&["-ni", "put", staged_str, folder])
    }
}

// ---------------------------------------------------------------------------
// ProcessRmapi: real runner (token-clobber-guarded)
// ---------------------------------------------------------------------------

/// Real runner: invokes the `rmapi` binary. Guards against rmapi's token-clobber
/// bug (it can zero its own conf on a transient failure, bricking later calls) by
/// snapshotting a good conf at construction and restoring it if a call empties it.
#[derive(Debug)]
pub struct ProcessRmapi {
    bin: PathBuf,
    conf_path: PathBuf,
    snapshot: Vec<u8>,
}

impl ProcessRmapi {
    /// Resolve the default rmapi binary (`rmapi` on PATH) and conf path.
    pub fn new() -> anyhow::Result<Self> {
        Self::with(PathBuf::from("rmapi"), default_conf_path())
    }

    /// Construct with explicit binary + conf paths (used by tests).
    pub fn with(bin: PathBuf, conf_path: PathBuf) -> anyhow::Result<Self> {
        resolve_bin(&bin)?;
        let snapshot = std::fs::read(&conf_path).map_err(|_| {
            anyhow::anyhow!(
                "rmapi is not paired (no conf at {}). Pair once by running `rmapi` \
                 with a code from https://my.remarkable.com/device/desktop/connect",
                conf_path.display()
            )
        })?;
        if is_blank_conf(&snapshot) {
            anyhow::bail!(
                "rmapi conf at {} has blank tokens; re-pair by running `rmapi`",
                conf_path.display()
            );
        }
        Ok(Self {
            bin,
            conf_path,
            snapshot,
        })
    }

    fn attempt(&self, args: &[&str]) -> anyhow::Result<bool> {
        use std::process::{Command, Stdio};
        let status = Command::new(&self.bin)
            .args(args)
            .stdin(Stdio::null())
            .status()?;
        Ok(status.success())
    }

    fn attempt_in(&self, dir: &Path, args: &[&str]) -> anyhow::Result<bool> {
        use std::process::{Command, Stdio};
        let status = Command::new(&self.bin)
            .args(args)
            .current_dir(dir)
            .stdin(Stdio::null())
            .status()?;
        Ok(status.success())
    }

    fn attempt_capture(&self, args: &[&str]) -> anyhow::Result<(bool, String)> {
        use std::process::{Command, Stdio};
        let out = Command::new(&self.bin)
            .args(args)
            .stdin(Stdio::null())
            .output()?;
        let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
        Ok((out.status.success(), stdout))
    }

    fn conf_blanked(&self) -> bool {
        std::fs::read(&self.conf_path)
            .map(|b| is_blank_conf(&b))
            .unwrap_or(true)
    }
}

impl RmapiRunner for ProcessRmapi {
    fn run(&self, args: &[&str]) -> anyhow::Result<()> {
        if self.attempt(args)? {
            return Ok(());
        }
        if self.conf_blanked() {
            std::fs::write(&self.conf_path, &self.snapshot)?;
            if self.attempt(args)? {
                return Ok(());
            }
        }
        anyhow::bail!("rmapi {:?} failed", args);
    }

    fn try_run_in(&self, dir: &Path, args: &[&str]) -> anyhow::Result<bool> {
        let ok = self.attempt_in(dir, args)?;
        if ok {
            return Ok(true);
        }
        if self.conf_blanked() {
            std::fs::write(&self.conf_path, &self.snapshot)?;
            return self.attempt_in(dir, args);
        }
        Ok(false)
    }

    fn run_capture(&self, args: &[&str]) -> anyhow::Result<String> {
        let (ok, stdout) = self.attempt_capture(args)?;
        if ok {
            return Ok(stdout);
        }
        if self.conf_blanked() {
            std::fs::write(&self.conf_path, &self.snapshot)?;
            let (ok2, stdout2) = self.attempt_capture(args)?;
            if ok2 {
                return Ok(stdout2);
            }
        }
        anyhow::bail!("rmapi {:?} failed", args);
    }
}

/// Resolve the rmapi conf path from env vars, mirroring rmapi's own resolution.
pub fn default_conf_path() -> PathBuf {
    if let Ok(p) = std::env::var("RMAPI_XDG_HOME") {
        return PathBuf::from(p).join("rmapi/rmapi.conf");
    }
    if let Ok(p) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(p).join("rmapi/rmapi.conf");
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/rmapi/rmapi.conf")
}

/// Verify the binary is runnable: an explicit path must be an existing file; a
/// bare name must be found on PATH.
pub fn resolve_bin(bin: &Path) -> anyhow::Result<()> {
    if bin.components().count() > 1 || bin.is_absolute() {
        if bin.is_file() {
            return Ok(());
        }
        anyhow::bail!("`{}` is not an executable file", bin.display());
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path) {
        if dir.join(bin).is_file() {
            return Ok(());
        }
    }
    anyhow::bail!(
        "`{}` not found on PATH; run inside `nix develop`",
        bin.display()
    )
}

/// A conf is "blank" unless it has a non-empty devicetoken AND usertoken.
pub fn is_blank_conf(bytes: &[u8]) -> bool {
    let s = String::from_utf8_lossy(bytes);
    let token_ok = |key: &str| {
        s.lines().any(|l| {
            l.trim()
                .strip_prefix(key)
                .map(|rest| {
                    let v = rest.trim_start_matches(':').trim().trim_matches('"');
                    !v.is_empty()
                })
                .unwrap_or(false)
        })
    };
    !(token_ok("devicetoken") && token_ok("usertoken"))
}

// ---------------------------------------------------------------------------
// LocalBackend
// ---------------------------------------------------------------------------

/// Local backend: no cloud; `put` writes PDFs to a directory.  Used by `--local`
/// and orchestration tests.
#[derive(Debug)]
pub struct LocalBackend {
    pub output_dir: PathBuf,
}

impl LocalBackend {
    pub fn new(output_dir: impl Into<PathBuf>) -> Self {
        Self {
            output_dir: output_dir.into(),
        }
    }
}

impl Backend for LocalBackend {
    fn list(&self, _root: &str, _exclude_suffixes: &[String]) -> anyhow::Result<Vec<CloudDoc>> {
        Ok(vec![])
    }

    fn fetch(&self, _doc: &CloudDoc) -> anyhow::Result<Option<PathBuf>> {
        Ok(None)
    }

    fn put(&self, pdf: &Path, _folder: &str, name: &str) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.output_dir)?;
        let dest = self.output_dir.join(format!("{}.pdf", name));
        std::fs::copy(pdf, dest)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    // -----------------------------------------------------------------------
    // FakeRunner
    // -----------------------------------------------------------------------

    /// Records every invocation and returns canned stdout for `run_capture`.
    #[derive(Debug, Default)]
    struct FakeRunner {
        calls: Rc<RefCell<Vec<Vec<String>>>>,
        /// Map from first non-flag arg (the subcommand path arg) to canned stdout.
        capture_responses: HashMap<String, String>,
        /// If set, `try_run_in` writes a `<name>.rmdoc` file in the dir.
        write_rmdoc: bool,
    }

    impl FakeRunner {
        fn with_capture(mut self, key: impl Into<String>, stdout: impl Into<String>) -> Self {
            self.capture_responses.insert(key.into(), stdout.into());
            self
        }

        fn writing(mut self) -> Self {
            self.write_rmdoc = true;
            self
        }
    }

    impl RmapiRunner for FakeRunner {
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
            if self.write_rmdoc {
                if let Some(remote) = args.last() {
                    let name = remote.split('/').next_back().unwrap_or("doc");
                    std::fs::write(dir.join(format!("{}.rmdoc", name)), b"fake bundle")?;
                }
            }
            Ok(self.write_rmdoc)
        }

        fn run_capture(&self, args: &[&str]) -> anyhow::Result<String> {
            self.calls
                .borrow_mut()
                .push(args.iter().map(|s| s.to_string()).collect());
            // Look up by the last arg (typically the path being ls'd).
            let key = args.last().copied().unwrap_or("");
            Ok(self.capture_responses.get(key).cloned().unwrap_or_default())
        }
    }

    // -----------------------------------------------------------------------
    // list: recursive + suffix exclusion
    // -----------------------------------------------------------------------

    #[test]
    fn list_recursive_and_excludes_suffixes() {
        // Simulates:
        //   /Watch
        //     [d] Author
        //     [f] TopDoc
        //     [f] TopDoc — Digest        ← should be excluded
        //   /Watch/Author
        //     [f] Great Book             ← included
        //     [f] Great Book — Digest    ← excluded
        //     [f] Great Book — Annotated ← excluded

        let root_ls = "[d]\tAuthor\n[f]\tTopDoc\n[f]\tTopDoc \u{2014} Digest\n";
        let author_ls =
            "[f]\tGreat Book\n[f]\tGreat Book \u{2014} Digest\n[f]\tGreat Book \u{2014} Annotated\n";

        let calls = Rc::new(RefCell::new(Vec::new()));
        let runner = FakeRunner {
            calls: calls.clone(),
            ..Default::default()
        }
        .with_capture("/Watch", root_ls)
        .with_capture("/Watch/Author", author_ls);

        let backend = RmapiBackend::new(runner);
        let suffixes = vec![
            " \u{2014} Digest".to_string(),
            " \u{2014} Annotated".to_string(),
        ];
        let docs = backend.list("/Watch", &suffixes).unwrap();

        // Should have exactly 2 docs: TopDoc and Great Book.
        assert_eq!(docs.len(), 2, "expected 2 docs, got: {:?}", docs);

        let top = docs
            .iter()
            .find(|d| d.name == "TopDoc")
            .expect("TopDoc missing");
        assert_eq!(top.folder, "/Watch");
        assert_eq!(top.path, "/Watch/TopDoc");
        assert!(top.version.is_none());

        let book = docs
            .iter()
            .find(|d| d.name == "Great Book")
            .expect("Great Book missing");
        assert_eq!(book.folder, "/Watch/Author");
        assert_eq!(book.path, "/Watch/Author/Great Book");

        // Should NOT appear in the results.
        assert!(docs.iter().all(|d| !d.name.contains("Digest")));
        assert!(docs.iter().all(|d| !d.name.contains("Annotated")));
    }

    // -----------------------------------------------------------------------
    // put: mkdir-p + rm + put arg sequence
    // -----------------------------------------------------------------------

    #[test]
    fn put_issues_correct_arg_sequence() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let runner = FakeRunner {
            calls: calls.clone(),
            ..Default::default()
        };
        let backend = RmapiBackend::new(runner);

        // Create a real temp PDF to copy.
        let tmp = tempfile::tempdir().unwrap();
        let pdf = tmp.path().join("source.pdf");
        std::fs::write(&pdf, b"%PDF").unwrap();

        backend
            .put(&pdf, "/Books/Digests/Author", "My Book — Digest")
            .unwrap();

        let c = calls.borrow();
        // mkdir -p /Books/Digests/Author → 3 mkdir calls.
        assert_eq!(c[0], vec!["-ni", "mkdir", "/Books"]);
        assert_eq!(c[1], vec!["-ni", "mkdir", "/Books/Digests"]);
        assert_eq!(c[2], vec!["-ni", "mkdir", "/Books/Digests/Author"]);
        // rm (best-effort).
        assert_eq!(
            c[3],
            vec!["-ni", "rm", "/Books/Digests/Author/My Book — Digest"]
        );
        // put: last arg must be the folder.
        assert_eq!(c[4].len(), 4, "put should have 4 args");
        assert_eq!(c[4][0], "-ni");
        assert_eq!(c[4][1], "put");
        // c[4][2] is the staged path — check it ends with the right filename.
        assert!(
            c[4][2].ends_with("My Book — Digest.pdf"),
            "staged path should end with 'My Book — Digest.pdf', got: {}",
            c[4][2]
        );
        assert_eq!(c[4][3], "/Books/Digests/Author");
    }

    // -----------------------------------------------------------------------
    // fetch: calls get and returns the .rmdoc path
    // -----------------------------------------------------------------------

    #[test]
    fn fetch_success_returns_rmdoc_path() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let runner = FakeRunner {
            calls: calls.clone(),
            ..Default::default()
        }
        .writing();
        let backend = RmapiBackend::new(runner);

        let doc = CloudDoc {
            path: "/Books/Author/My Book".to_string(),
            name: "My Book".to_string(),
            folder: "/Books/Author".to_string(),
            version: None,
        };
        let result = backend.fetch(&doc).unwrap();
        assert!(result.is_some(), "expected Some(path)");
        let path = result.unwrap();
        assert!(
            path.exists(),
            "returned path must exist: {}",
            path.display()
        );
        assert!(
            path.file_name().and_then(|n| n.to_str()).unwrap_or("") == "rmdigest-My Book.rmdoc",
            "unexpected filename: {}",
            path.display()
        );
        let c = calls.borrow();
        assert_eq!(c[0], vec!["-ni", "get", "/Books/Author/My Book"]);
    }

    #[test]
    fn fetch_missing_returns_none() {
        let calls = Rc::new(RefCell::new(Vec::new()));
        let runner = FakeRunner {
            calls: calls.clone(),
            ..Default::default()
        };
        let backend = RmapiBackend::new(runner);
        let doc = CloudDoc {
            path: "/Books/Author/Ghost".to_string(),
            name: "Ghost".to_string(),
            folder: "/Books/Author".to_string(),
            version: None,
        };
        let result = backend.fetch(&doc).unwrap();
        assert!(result.is_none());
    }

    // -----------------------------------------------------------------------
    // LocalBackend::put writes the PDF
    // -----------------------------------------------------------------------

    #[test]
    fn local_backend_put_writes_pdf() {
        let out = tempfile::tempdir().unwrap();
        let backend = LocalBackend::new(out.path());

        let src_dir = tempfile::tempdir().unwrap();
        let src = src_dir.path().join("input.pdf");
        std::fs::write(&src, b"%PDF-content").unwrap();

        backend.put(&src, "/ignored/folder", "My Digest").unwrap();

        let dest = out.path().join("My Digest.pdf");
        assert!(dest.exists(), "dest file must exist: {}", dest.display());
        assert_eq!(std::fs::read(&dest).unwrap(), b"%PDF-content");
    }

    // -----------------------------------------------------------------------
    // is_blank_conf
    // -----------------------------------------------------------------------

    #[test]
    fn blank_conf_detection() {
        assert!(is_blank_conf(b""));
        assert!(is_blank_conf(b"devicetoken: \"\"\nusertoken: \"\""));
        assert!(!is_blank_conf(
            b"devicetoken: \"abc123\"\nusertoken: \"def456\""
        ));
        // Only one token present → blank.
        assert!(is_blank_conf(b"devicetoken: \"abc123\""));
    }

    // -----------------------------------------------------------------------
    // resolve_bin
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_bin_finds_existing_absolute() {
        // /bin/sh should always exist.
        assert!(resolve_bin(Path::new("/bin/sh")).is_ok());
    }

    #[test]
    fn resolve_bin_rejects_missing_absolute() {
        assert!(resolve_bin(Path::new("/nonexistent/rmapi-fake-bin")).is_err());
    }
}
