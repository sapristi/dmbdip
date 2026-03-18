use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify_debouncer_mini::{new_debouncer, Debouncer, DebouncedEventKind};
use notify_debouncer_mini::notify;

pub struct FileWatcher {
    debouncer: Debouncer<notify::RecommendedWatcher>,
    watched_dir: Option<PathBuf>,
    target_file: Arc<Mutex<Option<PathBuf>>>,
}

impl FileWatcher {
    /// Creates a new file watcher. Watches the parent directory and filters
    /// events for the target file, so atomic-save (write+rename) works.
    pub fn new() -> Option<(Self, Receiver<()>)> {
        let (tx, rx) = mpsc::channel();
        let target_file: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
        let target_clone = Arc::clone(&target_file);

        let debouncer = new_debouncer(Duration::from_millis(200), move |res: notify_debouncer_mini::DebounceEventResult| {
            if let Ok(events) = res {
                let target = target_clone.lock().ok();
                let target_path = target.as_ref().and_then(|t| t.as_deref());
                for event in &events {
                    if event.kind == DebouncedEventKind::Any {
                        match target_path {
                            Some(tp) if event.path == tp => { let _ = tx.send(()); break; }
                            None => { let _ = tx.send(()); break; }
                            _ => {}
                        }
                    }
                }
            }
        }).ok()?;

        Some((FileWatcher { debouncer, watched_dir: None, target_file }, rx))
    }

    pub fn watch(&mut self, path: &Path) {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let parent = canonical.parent().map(|p| p.to_path_buf());

        // Update the target file for event filtering
        if let Ok(mut target) = self.target_file.lock() {
            *target = Some(canonical);
        }

        // Only re-watch directory if it changed
        if self.watched_dir.as_ref() != parent.as_ref() {
            self.unwatch_dir();
            if let Some(ref dir) = parent {
                let _ = self.debouncer.watcher().watch(dir, notify::RecursiveMode::NonRecursive);
                self.watched_dir = Some(dir.clone());
            }
        }
    }

    fn unwatch_dir(&mut self) {
        if let Some(ref dir) = self.watched_dir.take() {
            let _ = self.debouncer.watcher().unwatch(dir);
        }
    }
}
