//! Session command-trace writer. When recording is on, `Session` mutating
//! methods append a `kind: "call"` JSON line per invocation; `record_assert`
//! appends a `kind: "assert"` line (regardless of the recording flag).
//!
//! Entries land in `<state_dir>/trace.jsonl`. Trace writes are best-effort —
//! callers swallow errors so an I/O hiccup on the trace file never breaks the
//! session.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TraceEntry {
    Call {
        ts: String,
        cmd: Vec<String>,
        args: serde_json::Value,
        result: serde_json::Value,
    },
    Assert {
        ts: String,
        target: String,
        expected: serde_json::Value,
    },
}

#[derive(Debug, Clone)]
pub struct TraceWriter {
    path: PathBuf,
}

impl TraceWriter {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn append_call(
        &self,
        cmd: &[&str],
        args: serde_json::Value,
        result: serde_json::Value,
    ) -> std::io::Result<()> {
        let entry = TraceEntry::Call {
            ts: chrono::Utc::now().to_rfc3339(),
            cmd: cmd.iter().map(|s| s.to_string()).collect(),
            args,
            result,
        };
        self.append(&entry)
    }

    pub fn append_assert(&self, target: &str, expected: serde_json::Value) -> std::io::Result<()> {
        let entry = TraceEntry::Assert {
            ts: chrono::Utc::now().to_rfc3339(),
            target: target.to_string(),
            expected,
        };
        self.append(&entry)
    }

    fn append(&self, entry: &TraceEntry) -> std::io::Result<()> {
        let line = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        f.write_all(line.as_bytes())?;
        f.write_all(b"\n")?;
        Ok(())
    }
}

/// Read a `trace.jsonl` back into a `Vec<TraceEntry>`. Blank lines are skipped.
pub fn read_trace(path: &Path) -> std::io::Result<Vec<TraceEntry>> {
    let bytes = std::fs::read_to_string(path)?;
    let mut out = Vec::new();
    for line in bytes.lines() {
        if line.trim().is_empty() {
            continue;
        }
        out.push(
            serde_json::from_str(line)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?,
        );
    }
    Ok(out)
}
