use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use notify_debouncer_mini::{new_debouncer, Debouncer, DebouncedEventKind};
use notify_debouncer_mini::notify;

pub struct FileWatcher {
    debouncer: Debouncer<notify::RecommendedWatcher>,
    watched_path: Option<PathBuf>,
}

impl FileWatcher {
    pub fn new(path: &Path) -> Option<(Self, Receiver<()>)> {
        let (tx, rx) = mpsc::channel();

        let debouncer = new_debouncer(Duration::from_millis(200), move |res: notify_debouncer_mini::DebounceEventResult| {
            if let Ok(events) = res {
                if events.iter().any(|e| e.kind == DebouncedEventKind::Any) {
                    let _ = tx.send(());
                }
            }
        }).ok()?;

        let mut watcher = FileWatcher {
            debouncer,
            watched_path: None,
        };
        watcher.watch(path);
        Some((watcher, rx))
    }

    pub fn watch(&mut self, path: &Path) {
        self.unwatch();
        if let Ok(canonical) = path.canonicalize() {
            let _ = self.debouncer.watcher().watch(&canonical, notify::RecursiveMode::NonRecursive);
            self.watched_path = Some(canonical);
        }
    }

    pub fn unwatch(&mut self) {
        if let Some(ref path) = self.watched_path.take() {
            let _ = self.debouncer.watcher().unwatch(path);
        }
    }
}
