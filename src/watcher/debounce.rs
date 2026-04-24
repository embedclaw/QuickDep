//! Debounce logic for bursty file system events.
//!
//! The watcher receives many closely-spaced events for a single save or git
//! checkout. This helper coalesces them over a configurable settle window.

use crate::watcher::{FileChangeEvent, WatchEventKind};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Debounces file-system events over a fixed delay.
#[derive(Debug)]
pub struct EventDebouncer {
    delay: Duration,
    last_event_at: Option<Instant>,
    pending: HashMap<PathBuf, FileChangeEvent>,
}

impl EventDebouncer {
    /// Create a new debouncer with the given settle delay.
    pub fn new(delay: Duration) -> Self {
        Self {
            delay,
            last_event_at: None,
            pending: HashMap::new(),
        }
    }

    /// Return the configured debounce delay.
    pub fn delay(&self) -> Duration {
        self.delay
    }

    /// Add a new file-system event at the given time.
    pub fn push_at(&mut self, event: FileChangeEvent, now: Instant) {
        self.last_event_at = Some(now);
        self.pending
            .entry(event.path.clone())
            .and_modify(|existing| existing.kind = merge_kind(existing.kind, event.kind))
            .or_insert(event);
    }

    /// Add a new event using `Instant::now()`.
    pub fn push(&mut self, event: FileChangeEvent) {
        self.push_at(event, Instant::now());
    }

    /// Return whether enough idle time has passed to flush the batch.
    pub fn ready_at(&self, now: Instant) -> bool {
        self.last_event_at
            .is_some_and(|last| !self.pending.is_empty() && now.duration_since(last) >= self.delay)
    }

    /// Return whether the batch is ready to flush using the current time.
    pub fn ready(&self) -> bool {
        self.ready_at(Instant::now())
    }

    /// Drain all pending events in path order.
    pub fn drain(&mut self) -> Vec<FileChangeEvent> {
        let mut events = self
            .pending
            .drain()
            .map(|(_, event)| event)
            .collect::<Vec<_>>();
        events.sort_by(|left, right| left.path.cmp(&right.path));
        self.last_event_at = None;
        events
    }

    /// Return the number of pending paths.
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Return `true` when no events are pending.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

fn merge_kind(existing: WatchEventKind, incoming: WatchEventKind) -> WatchEventKind {
    match (existing, incoming) {
        (WatchEventKind::Deleted, _) | (_, WatchEventKind::Deleted) => WatchEventKind::Deleted,
        (WatchEventKind::Added, WatchEventKind::Modified)
        | (WatchEventKind::Modified, WatchEventKind::Added) => WatchEventKind::Added,
        (_, next) => next,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(path: &str, kind: WatchEventKind) -> FileChangeEvent {
        FileChangeEvent {
            path: PathBuf::from(path),
            kind,
        }
    }

    #[test]
    fn test_debouncer_coalesces_same_path() {
        let base = Instant::now();
        let mut debouncer = EventDebouncer::new(Duration::from_millis(500));

        debouncer.push_at(make_event("src/main.rs", WatchEventKind::Modified), base);
        debouncer.push_at(
            make_event("src/main.rs", WatchEventKind::Modified),
            base + Duration::from_millis(100),
        );

        assert!(!debouncer.ready_at(base + Duration::from_millis(400)));
        assert!(debouncer.ready_at(base + Duration::from_millis(700)));

        let events = debouncer.drain();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].path, PathBuf::from("src/main.rs"));
    }

    #[test]
    fn test_debouncer_delete_wins() {
        let base = Instant::now();
        let mut debouncer = EventDebouncer::new(Duration::from_millis(500));

        debouncer.push_at(make_event("src/main.rs", WatchEventKind::Added), base);
        debouncer.push_at(
            make_event("src/main.rs", WatchEventKind::Deleted),
            base + Duration::from_millis(50),
        );

        let events = debouncer.drain();
        assert_eq!(events[0].kind, WatchEventKind::Deleted);
    }
}
