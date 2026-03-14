use crate::config::LayoutConfig;

pub(crate) const SCROLL_STEP: u32 = 80;
pub(crate) const MARGIN_LEFT: u32 = 50;
pub(crate) const MARGIN_RIGHT: u32 = 20;
pub(crate) const PARAGRAPH_GAP: u32 = 16;
pub(crate) const CURSOR_WIDTH: u32 = 4;
pub(crate) const CURSOR_MARGIN: u32 = 6;
pub(crate) const MAX_CONTENT_WIDTH: u32 = 900;
pub(crate) const H1_EXTRA_MARGIN: u32 = 40;
pub(crate) const BLOCK_INDENT: u32 = 24;
pub(crate) const LIST_INDENT_PER_LEVEL: u32 = 24;

#[derive(Clone, Copy)]
pub(crate) struct LayoutParams {
    pub(crate) margin_left: u32,
    pub(crate) margin_right: u32,
    pub(crate) paragraph_gap: u32,
    pub(crate) max_content_width: u32,
    pub(crate) h1_extra_margin: u32,
    pub(crate) block_indent: u32,
    pub(crate) scroll_step: u32,
    pub(crate) cursor_width: u32,
    pub(crate) cursor_margin: u32,
}

fn clamp(val: u32, min: u32, max: u32) -> u32 {
    val.max(min).min(max)
}

impl LayoutParams {
    pub(crate) fn from_config(config: &LayoutConfig) -> Self {
        Self {
            margin_left: clamp(config.margin_left.unwrap_or(MARGIN_LEFT), 0, 200),
            margin_right: clamp(config.margin_right.unwrap_or(MARGIN_RIGHT), 0, 200),
            paragraph_gap: clamp(config.paragraph_gap.unwrap_or(PARAGRAPH_GAP), 0, 200),
            max_content_width: clamp(config.max_content_width.unwrap_or(MAX_CONTENT_WIDTH), 100, 4000),
            h1_extra_margin: clamp(config.h1_extra_margin.unwrap_or(H1_EXTRA_MARGIN), 0, 200),
            block_indent: clamp(config.block_indent.unwrap_or(BLOCK_INDENT), 0, 200),
            scroll_step: clamp(config.scroll_step.unwrap_or(SCROLL_STEP), 1, 500),
            cursor_width: clamp(config.cursor_width.unwrap_or(CURSOR_WIDTH), 1, 20),
            cursor_margin: clamp(config.cursor_margin.unwrap_or(CURSOR_MARGIN), 0, 50),
        }
    }
}

impl Default for LayoutParams {
    fn default() -> Self {
        Self {
            margin_left: MARGIN_LEFT,
            margin_right: MARGIN_RIGHT,
            paragraph_gap: PARAGRAPH_GAP,
            max_content_width: MAX_CONTENT_WIDTH,
            h1_extra_margin: H1_EXTRA_MARGIN,
            block_indent: BLOCK_INDENT,
            scroll_step: SCROLL_STEP,
            cursor_width: CURSOR_WIDTH,
            cursor_margin: CURSOR_MARGIN,
        }
    }
}

pub(crate) const KEYBINDINGS: &[(&str, &str)] = &[
    ("Tab / Shift+Tab", "Navigate between headings"),
    ("Space", "Toggle fold open/close"),
    ("Right", "Hide file list (full-width)"),
    ("Left", "Show file list / back to browser"),
    ("Up / Down", "Scroll"),
    ("j / k", "Small scroll steps"),
    ("PgUp / PgDn", "Half-page scroll"),
    ("Home / End", "Jump to top/bottom"),
    ("/", "Search text"),
    ("n / N", "Next/previous search match"),
    ("e", "Open in $EDITOR"),
    ("h", "Show this help"),
    ("q / Esc / Ctrl-C", "Quit"),
];

pub(crate) const BROWSER_KEYBINDINGS: &[(&str, &str)] = &[
    ("Up / Down, j / k", "Move cursor"),
    ("Right / Enter", "Open file or enter directory"),
    ("Left", "Go to parent directory"),
    ("e", "Open in $EDITOR"),
    ("h", "Show this help"),
    ("q / Esc / Ctrl-C", "Quit"),
];
