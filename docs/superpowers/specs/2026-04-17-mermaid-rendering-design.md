# Mermaid diagram rendering

Render fenced ` ```mermaid ` / ` ```mmd ` code blocks as rasterized diagrams in the markdown view, using the `mermaid-rs-renderer` crate (pure Rust, no browser).

## Goals

- Transparently replace mermaid code blocks with a rendered diagram in the same visual slot.
- Zero runtime dependencies beyond the Rust crate (no Node.js, no headless browser).
- Stay responsive when folding, scrolling, or resizing: avoid re-rendering diagrams on every frame.
- Fail gracefully: a broken diagram should not crash the app or hide the fact that it's broken.

## Non-goals

- Rendering mermaid in the syntax-highlighted source view (`src/source_render.rs`). That view shows raw text.
- Interactive diagram features (zoom, click, export).
- Mermaid theme customization beyond automatic dark/light selection.
- Persistent on-disk cache.

## Behavior

| Question | Decision |
|---|---|
| How are mermaid blocks detected? | Fence info token equals `mermaid` or `mmd` (case-insensitive, first whitespace token). |
| What if rendering fails? | Replace the diagram with a short error image showing only `Mermaid error: <msg>`. Source is not shown. |
| Diagram size? | Render at intrinsic SVG size. Scale down if wider than content width. Never scale up. Center horizontally within the block-indent content column. |
| Theme? | Auto-pick mermaid theme from app bg brightness. Dark bg → dark mermaid theme; light bg → modern (light) mermaid theme. No user-facing config. |
| Caching? | In-memory cache keyed by `(hash(source), target_width, dark_flag)`. Cleared when the document is reloaded. |
| Feature gating? | Always compiled in. Mermaid support is not a Cargo feature of dmbdip. |

## Architecture

```
fenced ```mermaid / ```mmd  →  Block::Mermaid { source }
                                       │
                                       ▼
                   MermaidCache::get_or_render(source, max_w, dark, ...)
                                       │
                       miss ─► mermaid_rs_renderer::render_with_options → SVG
                              → rasterize to RgbImage (crate `png` feature → resvg)
                              → scale-down-to-fit pass if needed
                              → insert Arc<RgbImage> into cache
                                       │
                                       ▼
                        composite into main page image
                          (centered, at current y)
```

The cache lives on `AppState` and is threaded into `render_markdown`, `render_preview`, and `compute_total_height` as `&mut MermaidCache`. The compute-total-height pass and the draw pass both use `get_or_render`; the second call is a cache hit, so sizes stay consistent.

## Changes by file

### `src/types.rs`

Add a variant:

```rust
pub(crate) enum Block {
    // ...existing...
    Mermaid { source: String },
}
```

### `src/parsing.rs`

- Import `CodeBlockKind` alongside `Tag`, `TagEnd`, etc.
- Add parser state `let mut code_lang: Option<String> = None;`.
- In the `MdEvent::Start(Tag::CodeBlock(kind))` arm, capture the fence info string:
  ```rust
  code_lang = match &kind {
      CodeBlockKind::Fenced(info) => Some(info.to_string()),
      CodeBlockKind::Indented => None,
  };
  ```
- In `MdEvent::End(TagEnd::CodeBlock)`, dispatch:
  ```rust
  let lang = code_lang.take().unwrap_or_default()
      .split_whitespace().next().unwrap_or("")
      .to_ascii_lowercase();
  let block = if lang == "mermaid" || lang == "mmd" {
      Block::Mermaid { source: text }
  } else {
      Block::CodeBlock { text }
  };
  blocks.push(block);
  ```

### `src/mermaid.rs` (new)

```rust
use std::collections::HashMap;
use std::sync::Arc;
use image::RgbImage;

use crate::fonts::Fonts;
use crate::theme::Theme;

#[derive(Hash, PartialEq, Eq)]
pub(crate) struct MermaidCacheKey {
    pub source_hash: u64,
    pub target_width: u32,
    pub dark: bool,
}

pub(crate) struct MermaidCache {
    entries: HashMap<MermaidCacheKey, Arc<RgbImage>>,
}

impl MermaidCache {
    pub fn new() -> Self { Self { entries: HashMap::new() } }
    pub fn clear(&mut self) { self.entries.clear(); }

    pub fn get_or_render(
        &mut self,
        source: &str,
        max_width: u32,
        dark: bool,
        fonts: &Fonts,
        theme: &Theme,
    ) -> Arc<RgbImage> { /* see flow below */ }
}

fn is_dark_bg(bg: image::Rgb<u8>) -> bool {
    let [r, g, b] = bg.0;
    (r as u32 + g as u32 + b as u32) < 3 * 128
}
```

Internal helpers (private): `render_mermaid_svg`, `rasterize_svg_to_rgb`, `render_error_image`, `scale_to_fit_width`.

`get_or_render` flow:

1. Build `MermaidCacheKey` from `hash(source)` + width + dark.
2. Cache hit → return `Arc` clone.
3. Cache miss: try `render_mermaid_svg(source, dark)` → `rasterize_svg_to_rgb(svg, max_width)`. On any error, build `render_error_image(msg, max_width, fonts, theme)`.
4. Insert `Arc<RgbImage>` and return.

### `src/render.rs`

Signatures gain `&mut MermaidCache`:

```rust
pub(crate) fn render_markdown(
    blocks: &[Block], headings: &mut [HeadingInfo],
    width: u32, vp_height: u32,
    fonts: &Fonts, theme: &Theme, layout: &LayoutParams,
    mermaid_cache: &mut MermaidCache,   // NEW
) -> (RgbImage, Vec<(usize, u32)>, u32);

pub(crate) fn render_preview(
    blocks: &[Block], headings: &[HeadingInfo],
    width: u32, max_height: u32,
    fonts: &Fonts, theme: &Theme, layout: &LayoutParams,
    mermaid_cache: &mut MermaidCache,   // NEW
) -> RgbImage;
```

`compute_total_height` likewise. New branch shape (same structure in each pass):

```rust
Block::Mermaid { source } => {
    let indented_width = content_width - layout.block_indent;
    let dark = is_dark_bg(theme.bg);
    let diagram = mermaid_cache.get_or_render(source, indented_width, dark, fonts, theme);
    let (dw, dh) = (diagram.width(), diagram.height());
    let x = margin_left + layout.block_indent + (indented_width - dw) / 2;
    image::imageops::overlay(&mut img, &*diagram, x as i64, y as i64);
    y += dh + layout.paragraph_gap;
}
```

`compute_total_height` adds `diagram.height() + paragraph_gap` to its running total; `render_preview` skips composite if `y + dh > max_height`.

### `src/state.rs`

Two exhaustive-match sites need a new arm:

```rust
// block_contains_text — mermaid source is searchable
Block::Mermaid { source } => source.to_lowercase().contains(query),

// compute_block_highlights — cannot highlight inside a rasterized image
Block::Mermaid { .. } => {}
```

Add a field to `AppState`:

```rust
pub(crate) mermaid_cache: MermaidCache,
```

Initialize with `MermaidCache::new()`. Clear it wherever the document is rebuilt on disk change (file-watcher reload). Do **not** clear it on fold, scroll, search, or resize — the cache key's width field handles resize automatically.

### `src/main.rs`

Add `mod mermaid;`. Thread the cache from `AppState` into render calls.

### `Cargo.toml`

```toml
mermaid-rs-renderer = { version = "0.2", default-features = false }
resvg = "0.46"
usvg = "0.46"
```

The crate's `png` feature only exposes a function that writes to a file path (not useful for in-memory compositing), so we depend on `resvg`/`usvg` directly at the same versions the crate uses and rasterize into a `tiny_skia::Pixmap`. The crate's default `cli` feature is off.

## Mermaid → app theme mapping

```rust
fn is_dark_bg(bg: Rgb<u8>) -> bool {
    let [r, g, b] = bg.0;
    (r as u32 + g as u32 + b as u32) < 3 * 128
}
```

- Light app bg → `mermaid_rs_renderer::Theme::modern()` unchanged.
- Dark app bg → `Theme::modern()` with color fields overridden to a dark palette (bg, text, primary/secondary/tertiary surfaces, edge labels, cluster fills, line color). The crate does not ship a dark constructor; we construct the variant ourselves. Palette defined in `src/mermaid.rs`.

## Error rendering

On any failure from parse, layout, or rasterization, `render_error_image` returns a small `RgbImage`:

- Background: `theme.bg`.
- Text: `"Mermaid error: <msg>"` drawn with `fonts.mono` at `theme.body_size` in `theme.code_color`.
- Padding: 8px.
- Width: `min(intrinsic_text_width + padding, max_width)`.
- Height: one or two wrapped lines of text.

Error images go through the same cache (errors are stable for a given source/width/brightness).

## Testing

**Parsing (`src/parsing.rs`):**

- `parses_mermaid_by_language_tag` — fence `mermaid` → `Block::Mermaid`.
- `parses_mmd_alias` — fence `mmd` → `Block::Mermaid`.
- `non_mermaid_fence_stays_code_block` — fence `rust` → `Block::CodeBlock`.
- `mermaid_lang_is_case_insensitive` — fence `MERMAID` → `Block::Mermaid`.
- `mermaid_lang_ignores_trailing_attrs` — fence `mermaid title=foo` → `Block::Mermaid`.

**Mermaid module (`src/mermaid.rs`):**

- `renders_valid_diagram_produces_nonempty_image` — trivial diagram, width > 0 and height > 0.
- `invalid_source_produces_error_image_without_panic` — bogus text yields an error image, not a panic.
- `cache_returns_same_arc_on_hit` — second call with identical args returns a pointer-equal `Arc`.
- `cache_key_distinguishes_width` — different widths miss the cache.
- `cache_key_distinguishes_brightness` — dark vs light flag miss the cache.

**Render integration (`src/render.rs`):**

- `render_with_mermaid_block_increases_total_height` — doc with a mermaid fence is taller than the same doc with that fence removed.
- `render_is_idempotent_on_second_call_with_same_inputs` — second call yields identical total height (cache path).

**Manual:** append a small mermaid example (e.g., `flowchart LR; A --> B --> C`) to `docs/sample.md`.

## Risks and open points

- **Which mermaid themes the crate actually ships** (dark variant exists?). Verified at implementation time; fallback to the closest dark variant if `Theme::dark()` is absent.
- **Rasterization DPI.** Mermaid SVG intrinsic sizes tend to be small; rasterizing at 1× may look fuzzy on HiDPI terminals. Start at 1×; if output looks soft we add a 2× rasterize + nearest downscale pass later.
- **Compile-time cost.** The `png` feature pulls in resvg's dependency tree. Acceptable; this is already a graphical tool.
- **Large diagrams.** A diagram that's tall (thousands of nodes) still renders in full because we never scale up — just scale down to width. Vertical overflow is fine; the doc scrolls. If a user hits extreme cases we can revisit with a max-height clamp later (YAGNI).
