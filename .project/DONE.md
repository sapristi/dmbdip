# DONE

## T-002 — Add `dmbdip --serve` headless mode for a neovim preview
created: 2026-11-10 12:37
done: 2026-11-22 14:49

Goal: a live markdown preview pane inside neovim, rendered by dmbdip. `:terminal dmbdip` can't
work (neovim's terminal does not pass kitty graphics through), and a kitty split (`kitty @ launch
--location=vsplit dmbdip %`) only reloads on save, with no scroll sync.

dmbdip side: a `--serve` mode with no TUI and no terminal output, speaking line-delimited JSON
on stdin/stdout. Requests: render (buffer text, which may be unsaved, + width in px) → render
and keep the page in memory; return the strips (see T-001) and a source-line → pixel-y map.
The map already exists in pieces: `parse_markdown` (`src/parsing.rs:35`) returns each block's
source line, `render_markdown` (`src/render.rs:125`) returns each block's y. Rendering is all
`pub(crate)` in a binary crate, so either add the serve module to the binary or split out a lib
target. Strip delivery: raw RGB via shared memory or a tmpfs file locally; zlib (`o=z`) when
neovim runs over SSH, since then pixels must go through the pty.

neovim side (~200 lines Lua, separate plugin): a scratch split filled with kitty unicode
placeholders (one image id per strip); debounced TextChanged → render; scroll sync by setting the
preview window's topline from the line → y map. Reuse `Snacks.image.terminal` (cell pixel size,
tmux passthrough, SSH detection) but not `Snacks.image.placement`: snacks caches images by file
path for the session (`snacks/image/image.lua:50`), so it can't replace a frame in place.
Scroll: neovim scrolls the placeholder buffer natively, so steps are one cell row; LazyVim's
`snacks.scroll` (enabled by default) animates them. Pixel-smooth momentum scrolling like the TUI
(`src/smooth_scroll.rs`) would need per-frame re-transmission (conflicts with T-001); kitty's
placement source-rect crop might allow a sub-row offset but is untested with unicode placeholders.

Folding: done natively in the preview buffer with manual folds, no re-render: placeholder cells
reference (image, row, col) independently, so hidden rows don't affect the rest. Requires serve
mode to pad each heading to start on a cell-row boundary and to return per heading its rows,
level and source line. A fold spans the rows after the heading up to the next heading of the same
or higher level (same rule as `is_block_folded`, `src/headings.rs:32`), so the heading stays
visible. The ▶/▼ arrows (`src/render.rs:178`) and the orange current-heading bar are then drawn
by neovim (signs / virt_text from `foldclosed()`, line highlight) instead of in the image;
heading numbers stay in the image (they don't depend on folds).

Keys in the preview buffer: Tab/S-Tab → next/prev visible heading row, placed at 1/4 of the
window (as `navigate_heading`, `src/state.rs:99`); Space → `za`; motions native. Search stays
in the source buffer (the preview has no text). Mirroring source-buffer folds into the preview
is possible later (no fold event: check `foldclosed()` on CursorMoved/WinScrolled or wrap
zc/zo/za).

Outcome: first version (version A of the spec
`.project/docs/specs/2026-11-22-14-13-nvim-preview-design.md`): `dmbdip --serve` renders buffer
text into ≤297-row raw RGB strips in `/dev/shm` (kitty `t=t`) and returns strips + a
source-line → row map; the Lua plugin (`lua/dmbdip/`, `:DmbdipPreview`) shows them with unicode
placeholders, re-renders on edit/resize and syncs scroll source → preview. Folding, heading
keys, SSH delivery, long documents and smooth scrolling moved to T-003. Commits 305f38c
(optional fold arrows), b0c3185 (leading H1 height fix), 3df8e8c (serve mode), f4dcd43 (plugin).
