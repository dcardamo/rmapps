//! Pure debounce keyed by (rule_path, doc id). Clock is injected (`Instant`).
use crate::watch::reconcile::Job;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Default)]
#[allow(dead_code)]
pub struct Debouncer {
    pending: HashMap<(String, String), (Job, Instant)>, // key -> (latest job, fire-at)
}

#[allow(dead_code)]
impl Debouncer {
    /// Record/refresh a job; its fire time becomes `now + job.debounce`.
    pub fn offer(&mut self, job: Job, now: Instant) {
        let key = (job.rule_path.clone(), job.doc.id.clone());
        let fire_at = now + job.debounce;
        self.pending.insert(key, (job, fire_at));
    }

    /// Remove and return all jobs whose window has elapsed.
    pub fn ready(&mut self, now: Instant) -> Vec<Job> {
        let ready_keys: Vec<_> = self
            .pending
            .iter()
            .filter(|(_, (_, fire_at))| *fire_at <= now)
            .map(|(k, _)| k.clone())
            .collect();
        ready_keys
            .into_iter()
            .filter_map(|k| self.pending.remove(&k).map(|(j, _)| j))
            .collect()
    }

    /// Earliest pending fire time, for computing the next select! sleep.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.pending.values().map(|(_, t)| *t).min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WatchAction;
    use crate::watch::reconcile::{ChangedDoc, Job};
    use std::time::Duration;

    fn job(id: &str) -> Job {
        Job {
            action: WatchAction::Digest,
            rule_path: "/Books".into(),
            debounce: Duration::from_secs(30),
            doc: ChangedDoc {
                id: id.into(),
                name: "B".into(),
                parent: "p".into(),
                path: format!("/Books/{id}"),
            },
        }
    }

    #[test]
    fn fires_after_window() {
        let t0 = Instant::now();
        let mut d = Debouncer::default();
        d.offer(job("1"), t0);
        assert!(d.ready(t0).is_empty());
        assert!(d.ready(t0 + Duration::from_secs(29)).is_empty());
        let ready = d.ready(t0 + Duration::from_secs(31));
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].doc.id, "1");
    }

    #[test]
    fn repeated_offer_resets_window_and_collapses() {
        let t0 = Instant::now();
        let mut d = Debouncer::default();
        d.offer(job("1"), t0);
        d.offer(job("1"), t0 + Duration::from_secs(20)); // reset -> fire at t0+50
        assert!(d.ready(t0 + Duration::from_secs(31)).is_empty());
        assert_eq!(d.ready(t0 + Duration::from_secs(51)).len(), 1);
    }

    #[test]
    fn next_deadline_is_earliest() {
        let t0 = Instant::now();
        let mut d = Debouncer::default();
        assert!(d.next_deadline().is_none());
        d.offer(job("1"), t0);
        let mut j2 = job("2");
        j2.debounce = Duration::from_secs(10);
        d.offer(j2, t0);
        assert_eq!(d.next_deadline(), Some(t0 + Duration::from_secs(10)));
    }
}
