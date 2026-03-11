pub(crate) const SCROLL_STEP: u32 = 40;
pub(crate) const MARGIN_LEFT: u32 = 50;
pub(crate) const MARGIN_RIGHT: u32 = 20;
pub(crate) const PARAGRAPH_GAP: u32 = 16;
pub(crate) const CURSOR_WIDTH: u32 = 4;
pub(crate) const CURSOR_MARGIN: u32 = 6; // gap between cursor and text
pub(crate) const MAX_CONTENT_WIDTH: u32 = 900;
pub(crate) const H1_EXTRA_MARGIN: u32 = 40;
pub(crate) const BLOCK_INDENT: u32 = 24;

pub(crate) const KEYBINDINGS: &[(&str, &str)] = &[
    ("Up / Down", "Navigate between headings"),
    ("Tab", "Toggle fold open/close"),
    ("Right", "Hide file list (full-width)"),
    ("Left", "Show file list / back to browser"),
    ("Space", "Scroll down"),
    ("Ctrl+Space", "Scroll up"),
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
