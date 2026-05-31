//! Shared helpers: session directory resolution + CLI-side errors.

use std::path::PathBuf;

pub fn home_dir() -> PathBuf {
    if let Ok(h) = std::env::var("INKCTL_HOME") {
        return PathBuf::from(h);
    }
    if let Ok(x) = std::env::var("XDG_STATE_HOME") {
        return PathBuf::from(x).join("inkctl");
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home)
        .join(".local")
        .join("state")
        .join("inkctl")
}

pub fn resolve_session_id(flag: Option<String>) -> Result<String, String> {
    flag.or_else(|| std::env::var("INKCTL_SESSION").ok())
        .ok_or_else(|| "no session: pass --session or set INKCTL_SESSION".into())
}

pub fn session_dir(id: &str) -> PathBuf {
    home_dir().join(id)
}
