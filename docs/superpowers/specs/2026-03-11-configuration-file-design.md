# Configuration File Support

## Overview

Add a TOML configuration file at `$XDG_CONFIG_HOME/dmbdip/dmbdip.toml` (defaulting to `~/.config/dmbdip/dmbdip.toml`) that allows users to customize theme colors, font sizes, layout parameters, and font paths. All fields are optional — missing fields fall back to current hardcoded defaults.

## Config File Format

```toml
[theme]
bg = "#1e1e28"
body_color = "dcdcdc"       # '#' prefix optional
body_size = 18.0
code_color = "#e6b450"
code_bg = "#2d2d3a"
cursor_color = "#ffb432"
h1_color = "#64a0ff"
h1_size = 36.0
h2_color = "#50c8c8"
h2_size = 28.0
h3_color = "#78dc78"
h3_size = 22.0
meta_key_color = "#b48cff"
meta_val_color = "#c8c8c8"
table_border = "#646478"
table_header_bg = "#323241"

[layout]
margin_left = 20
margin_right = 20
paragraph_gap = 16
max_content_width = 900
h1_extra_margin = 40
block_indent = 24
scroll_step = 40
cursor_width = 4
cursor_margin = 6

[fonts]
regular = "/usr/share/fonts/TTF/DejaVuSans.ttf"
bold = "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf"
italic = "/usr/share/fonts/TTF/DejaVuSans-Oblique.ttf"
mono = "/usr/share/fonts/TTF/DejaVuSansMono.ttf"
```

## Color Parsing

Colors are 6-digit hex strings. Both `"1e1e28"` and `"#1e1e28"` are accepted. Invalid color strings cause the app to print a warning to stderr and fall back to the default value.

## Layout Value Validation

Layout values are clamped to reasonable ranges to prevent panics:
- `margin_left`, `margin_right`: min 0, max 200
- `paragraph_gap`, `h1_extra_margin`, `block_indent`: min 0, max 200
- `max_content_width`: min 100, max 4000
- `scroll_step`: min 1, max 500
- `cursor_width`: min 1, max 20
- `cursor_margin`: min 0, max 50

## Architecture

### New module: `src/config.rs`

Defines:

- `Config` — top-level struct with optional `theme`, `layout`, `fonts` sections
- `ThemeConfig` — all `Option<String>` for colors, `Option<f32>` for sizes
- `LayoutConfig` — all `Option<u32>` fields
- `FontsConfig` — all `Option<String>` paths
- `load_config() -> Config` — reads config from `$XDG_CONFIG_HOME/dmbdip/dmbdip.toml` (falling back to `~/.config/dmbdip/dmbdip.toml`), returns empty Config if file missing
- `parse_hex_color(s: &str) -> Option<Rgb<u8>>` — strips optional `#`, parses 6 hex digits

All config structs derive `serde::Deserialize` with `#[serde(default)]`.

### Changes to existing modules

**`src/theme.rs`**:
- `default_theme()` becomes `build_theme(config: &ThemeConfig) -> Theme`
- For each field: use parsed config value if present, else hardcoded default

**`src/constants.rs`**:
- Layout constants remain as fallback defaults
- New `LayoutParams` struct with the same fields, constructed from config with fallback to constants
- Keybinding constants stay unchanged

**`src/fonts.rs`**:
- `load_fonts()` accepts `Option<&FontsConfig>` for custom paths
- Falls back to current path detection logic if paths not specified

**`src/state.rs`**:
- `AppState` gains a `layout: LayoutParams` field
- All references to layout constants (MARGIN_LEFT, PARAGRAPH_GAP, etc.) use `self.layout.*` instead
- `compute_block_highlights` updated to use `LayoutParams` instead of constants

**`src/render.rs`**:
- `render_markdown` and `render_preview` accept `&Theme` and `&LayoutParams` as parameters instead of constructing theme internally or importing constants
- `compute_total_height` and all sub-rendering functions updated similarly

**`src/main.rs`**:
- Calls `load_config()` at startup before loading fonts
- Passes config to theme builder, font loader, and AppState

### New dependencies

- `toml` — TOML parser
- `serde` with `derive` feature — deserialization

### Error handling

- Missing config file: silently use all defaults (normal case)
- Malformed TOML: print warning to stderr, use all defaults
- Invalid color value: print warning to stderr for that field, use default for that field
- Invalid font path: existing panic behavior (fonts are required)

### What stays unchanged

- Keybindings remain hardcoded
- Rendering logic/algorithms unchanged
- Kitty protocol code unchanged
- Browser mode uses same theme/layout (passed through)
- Overlay colors (help, search bar) stay hardcoded — separate concern for later
- Line height multipliers (1.3, 1.4, 1.5), code block padding, table cell padding — intentionally not configurable
- Browser sidebar width (`BROWSER_LEFT_COLS`) — not configurable
