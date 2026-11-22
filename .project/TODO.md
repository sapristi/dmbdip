# TODO

## T-001 — Render page as strips, don't resend when scrolling
created: 2026-11-10 12:37

Today every scroll step re-crops the page and re-sends the whole viewport: `kitty_display_raw`
(`src/kitty.rs:84`) base64-encodes raw RGB (f=24) and writes it through the pty (~6.4 MB per
redraw for a 1600×1000 window). `--debug` logs `b64_us` / `write_us` per frame to measure it.

Idea: split the rendered page (`render_markdown`, `src/render.rs:125`) into horizontal strips,
each sent to kitty once as its own image (re-sent only when the content or width changes), and
scroll by moving the strips' placements instead of transmitting pixels. Only send strips near
the viewport on long documents: kitty stores decoded pixels and caps image storage (~320 MB by
default), so memory is the limit, not transfer size.

Also worth trying, independently: transmit via shared memory (`t=s`, raw f=24) instead of base64
over the pty when running locally; keep the pty path (optionally `o=z` zlib, fast level) for
remote/SSH sessions. Compression is not worth it locally: it adds encode/decode cost and kitty
keeps decoded pixels anyway.

The same strip design is the basis of the neovim preview (T-002): there, strips are shown with
kitty unicode placeholders (virtual placements, `U=1`), at most 297 rows per image because of
the row diacritics, so neovim scrolls the preview buffer natively.

## T-002 — Add `dmbdip --serve` headless mode for a neovim preview
created: 2026-11-10 12:37

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

## T-003 — nvim preview: folding, heading keys, SSH delivery
created: 2026-11-22 14:04

Follow-up to T-002 (first version: `--serve` + minimal Lua plugin with live render and scroll
sync only). Remaining from the T-002 spec:

- Folding in the preview buffer with manual folds, no re-render: serve mode pads each heading to
  a cell-row boundary and returns per heading its rows, level and source line. A fold spans the
  rows after the heading up to the next heading of the same or higher level (same rule as
  `is_block_folded`, `src/headings.rs`), so the heading stays visible.
- ▶/▼ arrows and the orange current-heading bar drawn by neovim (signs / virt_text from
  `foldclosed()`, line highlight) instead of in the image; heading numbers stay in the image.
- Keys in the preview buffer: Tab/S-Tab → next/prev visible heading row placed at 1/4 of the
  window (as `navigate_heading`, `src/state.rs`); Space → `za`.
- Optional: mirror source-buffer folds into the preview (no fold event: check `foldclosed()` on
  CursorMoved/WinScrolled or wrap zc/zo/za).
- Strip delivery when neovim runs over SSH: pixels must go through the pty (zlib `o=z`).
- Long documents: keep only strips near the view in kitty (kitty caps image storage at ~320 MB
  and silently evicts oldest images → blank regions). Needs Lua to request strips on scroll and
  the server to keep strip pixels between requests. T-002 accepted the limit.
- Sub-row smooth scrolling (kitty placement source-rect crop, untested with unicode placeholders).
