# Development

## Tech Stack

- **Language:** Rust
- **Terminal:** Kitty (graphics protocol, raw RGB f=24, double-buffered)
- **Key crates:** `image`, `base64`, `crossterm`, `pulldown-cmark`, `ab_glyph`, `imageproc`, `notify-debouncer-mini`, `mermaid-rs-renderer`, `resvg`, `usvg`
- **Fonts:** DejaVu Sans (regular, bold, oblique) + DejaVu Sans Mono

## Architecture

- Parse markdown into `Vec<Block>` (Heading, Paragraph, CodeBlock, Table, Metadata, List, Mermaid)
- Inline text uses `Vec<Span>` with styles: Normal, Bold, Italic, Code, Strikethrough
- `HeadingInfo` tracks each heading's position, number, fold state
- `AppState` manages blocks, headings, cursor, scroll, fold, search state
- Rendering (`src/render.rs`): `layout_blocks` computes each unfolded block's slot (`y`, height
  including its trailing gap; an H1's extra margin is in its own slot) and the content height
  without drawing (mermaid diagrams are only sized: `MermaidCache::size`). `draw_block` draws one
  block at a page y onto an image whose row 0 is a given page row (clipped). Code blocks, lists
  and tables also get `parts` from the layout (y of each source line, item or row), so drawing
  skips the parts outside the image without measuring them. `render_markdown` (TUI) draws every
  slot onto one page image; `render_region` draws only the blocks overlapping a row range
  straight into a strip-sized image, so a strip's cost follows its height, not the blocks'
  (tested), and is pixel-identical to the same rows of `render_markdown` (tested). `MermaidCache` keeps SVGs and
  sizes (unbounded) apart from rasterized images (unbounded in the TUI, 16 LRU in serve mode).
- Kitty display: double-buffered with alternating image IDs, raw RGB, BufWriter
- Alternate screen mode for proper key passthrough
- Serve mode (`dmbdip --serve`, `src/serve.rs`), protocol 3: headless, no TUI, one JSON request
  per line on stdin, one JSON response line on stdout; every response has `"protocol": 3`.
  Serve lays out with `row_align = cell_h` (`layout_blocks`' last parameter; the TUI passes 1,
  which leaves the layout unchanged): each heading slot starts and ends on a cell row (the
  previous slot grows to the rounded-up `y`), so the plugin can fold whole buffer lines.
  - `layout` (`id, text, cols, cell_w, cell_h, strip_rows`): parses and lays out the document and
    keeps it; renders nothing. Image width = `min(cols × cell_w, max_content_width + margins)`
    rounded down to whole cells. Answers `doc` (incremented per layout), `image_cols`,
    `offset_cols = (cols − image_cols) / 2`, `total_rows`, `strip_rows` (clamped to 1..297, the
    unicode placeholder row-diacritic limit), `nstrips`, `bg` (`#rrggbb`),
    `anchors[[line, row]]` (each block's source line and cell row), `cursor` (`#rrggbb`, the
    theme's cursor colour) and `headings[{level, line, row, text_row, end_row, title, color}]`
    (document order; the slot covers rows `[row, end_row]`, `text_row` skips an H1's margin,
    `color` is the level's colour).
  - `strips` (`id, doc, indices`): renders those strips of the kept document with
    `render_region` and writes each as raw RGB (`image_cols·cell_w × rows·cell_h`, last strip
    padded with `bg`) to `dmbdip-<pid>-tty-graphics-protocol-<n>.rgb` in `/dev/shm` (else the
    temp dir); kitty reads and deletes them (`t=t`). Answers `strips[{index, rows, path}]`. A
    `doc` other than the current one gets `{"type": "stale", "doc": <current>}`.
  - Errors: `{"type": "error", "id", "message"}`. Leftover files are removed at stdin EOF.
- Neovim plugin (repo root is a plugin): `plugin/dmbdip.lua` defines `:DmbdipToggle`,
  `:DmbdipPreview` and the heading commands; `lua/dmbdip/init.lua` keeps one preview per source buffer: a
  `dmbdip --serve` job, a preview buffer (`bufhidden=hide`) of `total_rows` lines of
  `offset_cols + 2` spaces then `U+10EEEE` placeholders coloured by image id (`base + strip index`),
  and the set of strips loaded in the terminal. `:DmbdipToggle` swaps the current window's
  buffer (position mapped through the anchors both ways); `:DmbdipPreview` shows the same
  buffer in a split with live re-layout on edit and source → preview scroll sync. One request
  in flight at a time; layouts first (source changed, or the preview window's width, height or
  cell size changed). Windowed loading: on scroll/resize/layout/show, the strips intersecting
  `[top − H, bot + H]` that are not loaded are requested, and strips entirely outside
  `[top − 3H, bot + 3H]` are deleted (`a=d,d=I`); all are deleted when the preview buffer is no
  longer shown. Graphics commands go through `Snacks.image.terminal.request` (`q=2`, tmux
  passthrough). `lua/dmbdip/diacritics.lua` is kitty's row/column diacritics table.
  Folding: the preview buffer (`filetype=dmbdip`) gets neovim manual folds, one per heading with
  content, over rows `[end_row, stop − 1]` (`stop` = row of the next heading of the same or
  higher level, as `is_block_folded`), so a closed fold's line is the heading's last row shown
  as is (`foldtext=""`, `Folded` mapped to the empty `DmbdipFolded` so the placeholders keep
  their image-id colours). Neovim owns the fold state: the plugin reads it back
  (`foldclosed()`) only to rebuild the folds, keyed by level + title + occurrence, before a
  re-layout replaces the lines and when a window toggles out (restored when it shows the
  preview again; a new window starts unfolded). The layout is requested 2 columns narrower and
  each line starts with `offset_cols + 2` spaces: these 2 columns hold overlay extmarks
  (namespace `dmbdip_marks`, rows near the view) with the current-heading bar (`DmbdipCursor`)
  and the ▼/▶ arrow (`DmbdipHeading<level>`), refreshed on `CursorMoved`, `WinScrolled`, after
  a layout and after the heading functions. `next_heading`/`prev_heading`/`toggle_fold` (and
  `:DmbdipNextHeading`, `:DmbdipPrevHeading`, `:DmbdipFold`) act on the heading at or above the
  cursor. Designs:
  `.project/docs/specs/2026-11-22-14-13-nvim-preview-design.md`,
  `.project/docs/specs/2026-11-22-15-30-nvim-toggle-large-docs-design.md`,
  `.project/docs/specs/2026-11-22-17-52-nvim-preview-headings-design.md`.
- `build.rs` sets `DMBDIP_VERSION` (shown by `--version`): plain package version at the `v<version>` tag or without git, otherwise appends `git describe` output, e.g. `0.7.0 (v0.7.0-3-g1a2b3c4-dirty)`

## Workflow Rules

- Commit changes each time a task is done
- Clean git history if appropriate (squash fixup commits)
- Evaluate quality every few commits and after fixing bugs

## Manual testing

The file docs/sample.md can be used for manual testing

Neovim plugin smoke test (headless, stubs snacks), after `cargo build`:

```
nvim --headless -u NONE --cmd 'set rtp^=.' -l tests/nvim/smoke.lua
```

# Track advancement

## Todo

(no pending tasks)

## Completed Tasks

- [x] Task 0: Fix inline code spacing and code block indentation
- [x] Task 1: Hierarchical heading numbering (1., 1.1., 1.1.1.)
- [x] Task 2: Heading navigation with cursor (orange bar in left margin)
- [x] Task 3: Section folding with ▶/▼ indicators
- [x] Task 4: New keybindings (arrows for nav, space for scroll)
- [x] Task 5: CLAUDE.md as both README and CLAUDE.md with user info at top
- [x] Task 6: --help flag with usage and keybindings
- [x] Task 7: Help overlay when pressing `h`
- [x] Task 8: Vim-style search with `/`, `n`/`N` navigation
- [x] Task 9: File browser mode with two-pane layout
- [x] Task 10: TOML configuration file support (theme, layout, fonts)
- [x] Task 11: Source file viewing with syntax highlighting (syntect)
- [x] Task 12: File watcher for auto-reload on external edits (notify-debouncer-mini)
- [x] Task 13: Mermaid diagram rendering via mermaid-rs-renderer (auto dark/light, in-memory cache)
