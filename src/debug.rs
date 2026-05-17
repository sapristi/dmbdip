use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

static LOG: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
static START: OnceLock<Instant> = OnceLock::new();

pub(crate) fn init(enabled: bool) -> Option<PathBuf> {
    let _ = START.set(Instant::now());
    if !enabled {
        let _ = LOG.set(None);
        return None;
    }
    let path = std::env::var("DMBDIP_DEBUG_LOG")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir().join("dmbdip-debug.log"));
    match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
    {
        Ok(f) => {
            let _ = LOG.set(Some(Mutex::new(f)));
            Some(path)
        }
        Err(_) => {
            let _ = LOG.set(None);
            None
        }
    }
}

pub(crate) fn enabled() -> bool {
    matches!(LOG.get(), Some(Some(_)))
}

pub(crate) fn log_line(line: &str) {
    if let Some(Some(mtx)) = LOG.get() {
        if let Ok(mut f) = mtx.lock() {
            let elapsed = START.get().map(|s| s.elapsed().as_secs_f64()).unwrap_or(0.0);
            let _ = writeln!(f, "{:>10.3} {}", elapsed, line);
        }
    }
}

#[macro_export]
macro_rules! dlog {
    ($($arg:tt)*) => {
        if $crate::debug::enabled() {
            $crate::debug::log_line(&format!($($arg)*));
        }
    }
}
