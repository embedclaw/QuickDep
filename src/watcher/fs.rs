//! Filesystem watcher built on top of `notify`.
//!
//! This layer only deals with file-system notifications and pause/resume
//! semantics. Higher-level debounce and hash filtering live in sibling
//! modules.

use crate::watcher::WatcherError;
use notify::{recommended_watcher, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::mpsc;

/// Normalized watcher event kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchEventKind {
    Added,
    Modified,
    Deleted,
}

/// File-system change event emitted by the watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChangeEvent {
    /// Absolute file path.
    pub path: PathBuf,
    /// Normalized event kind.
    pub kind: WatchEventKind,
}

/// Live filesystem watcher with pause/resume support.
pub struct FileSystemWatcher {
    root: PathBuf,
    paused: Arc<AtomicBool>,
    needs_resync: Arc<AtomicBool>,
    _watcher: RecommendedWatcher,
}

impl FileSystemWatcher {
    /// Start watching a project root and forward normalized events to the channel.
    pub fn new(
        root: impl AsRef<Path>,
        event_tx: mpsc::UnboundedSender<FileChangeEvent>,
    ) -> Result<Self, WatcherError> {
        let root = root.as_ref().canonicalize()?;
        let paused = Arc::new(AtomicBool::new(false));
        let needs_resync = Arc::new(AtomicBool::new(false));
        let paused_flag = paused.clone();
        let resync_flag = needs_resync.clone();

        let mut watcher = recommended_watcher(move |event: notify::Result<Event>| {
            if paused_flag.load(Ordering::SeqCst) {
                resync_flag.store(true, Ordering::SeqCst);
                return;
            }

            let Ok(event) = event else {
                return;
            };

            for file_event in normalize_event(&event) {
                if event_tx.send(file_event).is_err() {
                    return;
                }
            }
        })?;

        watcher.watch(&root, RecursiveMode::Recursive)?;

        Ok(Self {
            root,
            paused,
            needs_resync,
            _watcher: watcher,
        })
    }

    /// Pause forwarding new events.
    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    /// Resume forwarding new events.
    pub fn resume(&self) -> bool {
        self.paused.store(false, Ordering::SeqCst);
        self.needs_resync.swap(false, Ordering::SeqCst)
    }

    /// Return whether event forwarding is currently paused.
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    /// Return whether a full resync is required because events arrived while paused.
    pub fn needs_resync(&self) -> bool {
        self.needs_resync.load(Ordering::SeqCst)
    }

    /// Return the watched project root.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn normalize_event(event: &Event) -> Vec<FileChangeEvent> {
    let Some(kind) = normalize_kind(&event.kind) else {
        return Vec::new();
    };

    event
        .paths
        .iter()
        .filter(|path| !path.exists() || !path.is_dir())
        .map(|path| FileChangeEvent {
            path: path.clone(),
            kind,
        })
        .collect()
}

fn normalize_kind(kind: &EventKind) -> Option<WatchEventKind> {
    match kind {
        EventKind::Create(_) => Some(WatchEventKind::Added),
        EventKind::Modify(_) => Some(WatchEventKind::Modified),
        EventKind::Remove(_) => Some(WatchEventKind::Deleted),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::{event::CreateKind, Event};
    use tempfile::tempdir;

    #[test]
    fn test_normalize_event_filters_directories() {
        let temp_dir = tempdir().expect("create temp dir");
        let file_path = temp_dir.path().join("src/main.rs");
        std::fs::create_dir_all(file_path.parent().expect("parent")).expect("create dir");
        std::fs::write(&file_path, "fn main() {}\n").expect("write file");

        let event = Event {
            kind: EventKind::Create(CreateKind::Any),
            paths: vec![temp_dir.path().join("src"), file_path.clone()],
            attrs: Default::default(),
        };

        let normalized = normalize_event(&event);
        assert_eq!(
            normalized,
            vec![FileChangeEvent {
                path: file_path,
                kind: WatchEventKind::Added,
            }]
        );
    }

    #[test]
    fn test_pause_resume_state() {
        let temp_dir = tempdir().expect("create temp dir");
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let watcher = FileSystemWatcher::new(temp_dir.path(), event_tx).expect("create watcher");

        assert!(!watcher.is_paused());
        watcher.pause();
        assert!(watcher.is_paused());
        assert!(!watcher.needs_resync());
        assert!(!watcher.resume());
        assert!(!watcher.is_paused());
    }
}
