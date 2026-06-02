//! Sync timing instrumentation: toggle resolution, subscriber install, and the
//! span constructors used across the sync pipeline. Off by default; enabled by
//! the `--timings` flag or `RMAPPS_TIMINGS=1`.

use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::EnvFilter;

/// Resolve whether timing output is enabled. The `--timings` flag wins; failing
/// that, a truthy `RMAPPS_TIMINGS` value (`1`/`true`/`yes`, case-insensitive)
/// enables it. Off otherwise. Kept pure (args injected) so it is unit-testable.
pub fn timings_enabled(flag: bool, env: Option<&str>) -> bool {
    if flag {
        return true;
    }
    matches!(
        env.map(|s| s.trim().to_ascii_lowercase()).as_deref(),
        Some("1") | Some("true") | Some("yes")
    )
}

/// Install the timing subscriber when enabled. Uses an stderr `fmt` layer that
/// logs each span's busy/idle duration on close (`FmtSpan::CLOSE`), so a run
/// prints a per-stage breakdown. Honors `RUST_LOG` if set, else defaults to
/// `info` (the level our spans use). No-op when disabled, leaving spans
/// effectively free. Safe to call once; a second install is ignored.
pub fn init(enabled: bool) {
    if !enabled {
        return;
    }
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_span_events(FmtSpan::CLOSE)
        .with_writer(std::io::stderr)
        .with_target(false)
        .try_init();
}

/// Root span for one sync invocation.
pub fn sync_span() -> tracing::Span {
    tracing::info_span!("sync.run")
}

/// Per-task span; `name` is the app key (`bujo`/`reader`/`digest`).
pub fn task_span(name: &str) -> tracing::Span {
    tracing::info_span!("task", name = name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing::span::Attributes;
    use tracing::{Id, Subscriber};
    use tracing_subscriber::layer::{Context, Layer};
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::registry::LookupSpan;

    /// A minimal layer that records the name of every span created while it is
    /// the active subscriber — enough to prove our helpers emit the right spans.
    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<String>>>);

    impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Capture {
        fn on_new_span(&self, attrs: &Attributes<'_>, _id: &Id, _ctx: Context<'_, S>) {
            self.0.lock().unwrap().push(attrs.metadata().name().to_string());
        }
    }

    #[test]
    fn toggle_flag_wins() {
        assert!(timings_enabled(true, None));
        assert!(timings_enabled(true, Some("0")));
    }

    #[test]
    fn toggle_env_truthy() {
        assert!(timings_enabled(false, Some("1")));
        assert!(timings_enabled(false, Some("true")));
        assert!(timings_enabled(false, Some("YES")));
        assert!(timings_enabled(false, Some("  true  ")));
    }

    #[test]
    fn toggle_off_by_default() {
        assert!(!timings_enabled(false, None));
        assert!(!timings_enabled(false, Some("0")));
        assert!(!timings_enabled(false, Some("nope")));
    }

    #[test]
    fn helpers_emit_named_spans() {
        let cap = Capture::default();
        let names = cap.0.clone();
        let subscriber = tracing_subscriber::registry().with(cap);
        tracing::subscriber::with_default(subscriber, || {
            let _s = sync_span().entered();
            let _t = task_span("reader").entered();
        });
        let names = names.lock().unwrap();
        assert!(names.iter().any(|n| n == "sync.run"), "got {names:?}");
        assert!(names.iter().any(|n| n == "task"), "got {names:?}");
    }
}
