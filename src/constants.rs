pub(crate) const SCROLL_STEP: u32 = 40;
pub(crate) const MARGIN_LEFT: u32 = 20;
pub(crate) const MARGIN_RIGHT: u32 = 20;
pub(crate) const PARAGRAPH_GAP: u32 = 16;
pub(crate) const CURSOR_WIDTH: u32 = 4;
pub(crate) const CURSOR_MARGIN: u32 = 6; // gap between cursor and text
pub(crate) const MAX_CONTENT_WIDTH: u32 = 900;
pub(crate) const H1_EXTRA_MARGIN: u32 = 40;
pub(crate) const BLOCK_INDENT: u32 = 24;

pub(crate) const KEYBINDINGS: &[(&str, &str)] = &[
    ("Up / Down", "Navigate between headings"),
    ("Right / Tab", "Toggle fold open/close"),
    ("Space", "Scroll down"),
    ("Ctrl+Space", "Scroll up"),
    ("j / k", "Small scroll steps"),
    ("PgUp / PgDn", "Half-page scroll"),
    ("Home / End", "Jump to top/bottom"),
    ("/", "Search text"),
    ("n / N", "Next/previous search match"),
    ("h", "Show this help"),
    ("q / Esc", "Quit"),
];
