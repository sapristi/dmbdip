use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use image::{Rgb, RgbImage};

#[derive(Hash, PartialEq, Eq, Clone, Copy)]
pub(crate) struct MermaidCacheKey {
    pub(crate) source_hash: u64,
    pub(crate) target_width: u32,
    pub(crate) dark: bool,
}

pub(crate) struct MermaidCache {
    entries: HashMap<MermaidCacheKey, Arc<RgbImage>>,
}

impl MermaidCache {
    pub(crate) fn new() -> Self {
        Self { entries: HashMap::new() }
    }

    #[allow(dead_code)]
    pub(crate) fn clear(&mut self) {
        self.entries.clear();
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

pub(crate) fn is_dark_bg(bg: Rgb<u8>) -> bool {
    let [r, g, b] = bg.0;
    (r as u32 + g as u32 + b as u32) < 3 * 128
}

pub(crate) fn hash_source(source: &str) -> u64 {
    let mut h = DefaultHasher::new();
    source.hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_new_is_empty() {
        let cache = MermaidCache::new();
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn dark_bg_detection() {
        assert!(is_dark_bg(Rgb([0, 0, 0])));
        assert!(is_dark_bg(Rgb([30, 30, 30])));
        assert!(!is_dark_bg(Rgb([200, 200, 200])));
        assert!(!is_dark_bg(Rgb([255, 255, 255])));
    }

    #[test]
    fn hash_source_is_deterministic() {
        let a = hash_source("flowchart LR; A-->B");
        let b = hash_source("flowchart LR; A-->B");
        assert_eq!(a, b);
    }

    #[test]
    fn hash_source_differs_on_different_input() {
        let a = hash_source("flowchart LR; A-->B");
        let b = hash_source("flowchart LR; A-->C");
        assert_ne!(a, b);
    }
}
