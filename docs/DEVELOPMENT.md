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
- Two-pass rendering: compute height, then draw to RgbImage (re-renders on fold/nav changes)
- Kitty display: double-buffered with alternating image IDs, raw RGB, BufWriter
- Alternate screen mode for proper key passthrough
- Serve mode (`dmbdip --serve`, `src/serve.rs`): headless, no TUI. Reads one JSON render request
  per line on stdin (`id, type="render", text, cols, cell_w, cell_h`) and answers one JSON line
  (`id, type="render", cols, total_rows, strips[{index, rows, path|null}], anchors[[line, row]]`,
  or `type="error"`). Renders at `cols × cell_w` px without fold arrows, pads to whole cell rows,
  cuts into strips of ≤297 rows (unicode placeholder row-diacritic limit) and writes only changed
  strips (hash of pixels, or geometry change) as raw RGB files named
  `dmbdip-<pid>-tty-graphics-protocol-<n>.rgb` in `/dev/shm` (else the temp dir); kitty reads and
  deletes them (`t=t`). Leftover files are removed at stdin EOF. Anchors map each block's source
  line to its cell row.
- Neovim plugin (repo root is a plugin): `plugin/dmbdip.lua` defines `:DmbdipPreview`;
  `lua/dmbdip/init.lua` runs one `dmbdip --serve` job per source buffer, sends debounced render
  requests (one in flight), transmits strips via `Snacks.image.terminal.request` (`q=2`, tmux
  passthrough) with image ids `base + index`, fills the preview buffer with `U+10EEEE`
  placeholders coloured by id, and syncs the preview's topline from the anchors;
  `lua/dmbdip/diacritics.lua` is kitty's row/column diacritics table. Design:
  `.project/docs/specs/2026-11-22-14-13-nvim-preview-design.md`.

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
