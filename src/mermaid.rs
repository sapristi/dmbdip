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

fn mermaid_theme_for(dark: bool) -> mermaid_rs_renderer::Theme {
    let mut t = mermaid_rs_renderer::Theme::modern();
    if dark {
        t.background = "#1e1e1e".to_string();
        t.primary_color = "#264f78".to_string();
        t.primary_text_color = "#d4d4d4".to_string();
        t.primary_border_color = "#569cd6".to_string();
        t.secondary_color = "#3c3c3c".to_string();
        t.tertiary_color = "#2d2d30".to_string();
        t.line_color = "#7f7f7f".to_string();
        t.text_color = "#d4d4d4".to_string();
        t.edge_label_background = "#1e1e1e".to_string();
        t.cluster_background = "#2d2d30".to_string();
        t.cluster_border = "#569cd6".to_string();
    }
    t
}

pub(crate) fn render_mermaid_svg(source: &str, dark: bool) -> Result<String, String> {
    let mut opts = mermaid_rs_renderer::RenderOptions::modern();
    opts.theme = mermaid_theme_for(dark);
    mermaid_rs_renderer::render_with_options(source, opts)
        .map_err(|e| e.to_string())
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

    #[test]
    fn render_svg_valid_diagram_returns_svg_string() {
        let result = render_mermaid_svg("flowchart LR\n    A --> B", false);
        assert!(result.is_ok(), "expected Ok, got: {:?}", result);
        let svg = result.unwrap();
        assert!(svg.contains("<svg"), "output should contain <svg tag");
    }

    #[test]
    fn render_svg_dark_palette_affects_output() {
        let light = render_mermaid_svg("flowchart LR\n    A --> B", false).unwrap();
        let dark = render_mermaid_svg("flowchart LR\n    A --> B", true).unwrap();
        // Different theme palettes produce different SVG bytes.
        assert_ne!(light, dark, "dark and light SVGs should differ");
    }

}
