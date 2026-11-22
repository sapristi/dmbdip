# Neovim preview: in-place toggle and large documents

Builds on `2026-11-22-14-13-nvim-preview-design.md` (T-002, done). Replaces its render protocol.

## Intent

The main way to use the plugin is to flip the current window between the markdown source and
the rendered preview with a key, then read and scroll. Live editing with a side-by-side preview
is secondary (kept, not prioritised). Top priority: smooth scrolling and displaying large
documents (thousands of lines, HiDPI screens).

Today the server renders the whole page into one image, writes every strip, and the plugin keeps
every strip in kitty. Server memory is ~60 KB per preview row at normal DPI (×4 on HiDPI) —
~450 MB for a 5,000-line doc — the first render renders everything, and kitty's ~320 MB image
cap (shared with other images, oldest silently evicted) is reached after ~1,500 source lines in
a full-width window.

## Goals

1. `:DmbdipToggle` swaps the current window between a markdown buffer and its preview, keeping
   the reading position both ways. The plugin defines no key mapping; the README shows a
   lazy.nvim `keys` entry.
2. Memory bounded independently of document size, in both the server and kitty: the server keeps
   the parsed document and block layout, and renders a strip only when asked; the plugin keeps
   only strips near the view in kitty.
3. Images cover only the content column (`max_content_width` + margins), not the full window
   width; the preview is centred with blank cells around it.
4. `:DmbdipPreview` (split, live re-render on edit, scroll sync) keeps working on the new
   protocol.

## Non-goals

- Folding, heading keys, SSH delivery, sub-row smooth scrolling (T-003).
- Incremental text updates / partial re-layout (T-004).
- Changing the TUI's display path (T-001), though the strip renderer is reusable for it.

## Anti-goals

- TUI rendering must stay pixel-identical (fold arrows, folds, heights).
- Scrolling must never wait on a full-document render.
- Never delete kitty images that aren't ours; never let kitty replies reach neovim (`q=2`).

## Approaches considered (server memory)

- **A. Keep the full page image in the server** (what T-002 does). Simplest, but memory and first
  render time grow with document size; fine to ~1–2k lines only. Rejected.
- **B. Render strips on demand (picked).** A layout pass computes every block's y and height
  (already done implicitly by `compute_total_height`); a strip is drawn by rendering only the
  blocks overlapping it. Flat memory, fast first strip. Costs a refactor of `src/render.rs`.

(Earlier choices kept from T-002: strip pixels go through `/dev/shm`/temp files read by kitty with
`t=t`; snacks.nvim for cell size and graphics commands.)

## Design

### 1. Rendering (`src/render.rs`)

- **Layout pass**: `layout_blocks(blocks, headings, width, fonts, theme, layout, mermaid_cache)
  → (Vec<BlockLayout>, total_height)`, `BlockLayout { block_index, y, height }`. A block's slot
  runs from its `y` to the next block's `y`, so it includes its trailing gap; an H1's extra
  top margin belongs to the H1's own slot (its `y` is where the margin starts). Folded blocks are skipped as today. `compute_total_height`
  becomes a thin wrapper (TUI keeps its `vp_height / 2` bottom padding).
- **Block drawing**: factor the per-block `match` of `render_markdown` into one function that
  draws a block at a given `y` onto an image. `render_markdown` keeps its signature and output
  (built on the two pieces above).
- **Region rendering**: `render_region(…, layouts, y0, height) → RgbImage` of the full render
  width × `height`: background fill, then each block overlapping `[y0, y0+height)` is drawn into
  a scratch image of the block's own height (at y = 0, with heading state such as `y_pos` set
  as in the full render) and the overlapping rows are copied in. Glyphs never extend past a
  block's slot because the slot includes its trailing gap; the equivalence test below guards it.
- **Mermaid** *(builder-check call)*: the layout pass must not rasterize diagrams. Add a
  size path to `MermaidCache`: render the mermaid source to SVG and read the output size from
  the `usvg` tree (same scaling rule as `rasterize_svg_to_rgb`, `src/mermaid.rs:104`), caching
  `key → (svg, width, height)`; rasterize only when a strip draws the block. Error diagrams:
  their size comes from rendering the (small) error image. The pixel cache gets an optional
  capacity (LRU): the server uses a small bound (16 images); the TUI keeps today's unbounded
  behaviour. The size cache is unbounded (a few bytes per diagram plus the SVG text).
- Caching other block images is left to
  the builder (optional LRU if profiling shows it helps — a tall code block overlapping several
  strips gets re-drawn per strip otherwise).

**Equivalence test (required)**: for `docs/sample.md`, `SAMPLE_MD` and a generated long document
(> 3 strips, with code blocks, tables, lists, mermaid, H1s) concatenating `render_region` strips
yields an image pixel-identical to `render_markdown` (with `fold_arrows=false`, `vp_height=0`).

### 2. Serve protocol v2 (`src/serve.rs`)

Line-delimited JSON as before; the old `render` request is removed (plugin and binary ship
from the same repo). Every response carries `"protocol": 2`; the plugin refuses to run against
another value with "dmbdip binary too old/new, reinstall".

**layout** — parse, lay out and keep the document; no pixels rendered, no files written:

```json
{"id": 1, "type": "layout", "text": "...", "cols": 200, "cell_w": 10, "cell_h": 21, "strip_rows": 48}
```
```json
{"id": 1, "type": "layout", "protocol": 2, "doc": 1, "image_cols": 97, "offset_cols": 51,
 "total_rows": 3120, "strip_rows": 48, "nstrips": 65, "bg": "#1e1e2e",
 "anchors": [[1, 0], [5, 3]]}
```

- `cols`: window text width in cells. Render width in px = `min(cols × cell_w,
  max_content_width + margin_left + margin_right)` rounded **down** to a multiple of `cell_w`
  (`image_cols = width / cell_w`), and must exceed `margin_left + margin_right + block_indent`
  (else error, as today). `offset_cols = (cols − image_cols) / 2` (integer division).
- `strip_rows`: requested rows per strip, clamped to `[1, 297]`. Strip `i` covers rows
  `[i·strip_rows, min((i+1)·strip_rows, total_rows))`.
- `total_rows = ceil(total_height / cell_h)`, min 1; the last strip is padded with `bg`.
- `doc`: increments on every successful layout; identifies the kept document.
- `anchors` as in T-002 (`[source_line, row]`, from the layout pass).
- `bg`: theme background, so the plugin can paint the blank cells to match.

**strips** — render strips of the kept document and write them as files:

```json
{"id": 2, "type": "strips", "doc": 1, "indices": [3, 4, 5]}
```
```json
{"id": 2, "type": "strips", "protocol": 2, "doc": 1,
 "strips": [{"index": 3, "rows": 48, "path": "/dev/shm/dmbdip-…-tty-graphics-protocol-7.rgb"}]}
```

- `doc` ≠ current → `{"type": "stale", "id": 2, "doc": <current>}` (not an error; the plugin
  drops it). Out-of-range indices → error.
- Files: same naming, location and EOF cleanup as T-002. Each strip is exactly
  `image_cols·cell_w × rows·cell_h` px raw RGB.
- No change detection: requested strips are always rendered and written.

Errors: `{"type":"error","id":…,"message":…}` as today; panics caught as today.

### 3. Plugin: preview state and display

One preview per source buffer (keyed by bufnr): server job, preview buffer, image id base, last
layout (`doc`, geometry, `changedtick` of the source it was made from), loaded strips set.

- **Preview buffer**: `buftype=nofile`, `bufhidden=hide` (kept while the source lives),
  `filetype=dmbdip`, not modifiable. Lines: `total_rows` lines of `offset_cols` spaces followed by
  `image_cols` placeholder cells (`U+10EEEE` + row diacritic of the row within its strip +
  column diacritic 0, then bare `U+10EEEE`). Each strip's line range gets an extmark with
  highlight `DmbdipImg<id>` (fg = id) on the placeholder columns only. Lines are rewritten only
  when `total_rows`, `image_cols`, `offset_cols` or `strip_rows` change.
- **Background**: highlight `DmbdipBg` with `bg = layout.bg`; preview windows get
  `winhighlight=Normal:DmbdipBg,EndOfBuffer:DmbdipBg`.
- **Preview window** = the window most recently showing the preview buffer. Geometry
  (`cols` = text width, `strip_rows` = window height in rows, cell size from
  `Snacks.image.terminal.size()`) comes from it.
- **Window options** while the preview buffer is shown (same set as T-002 plus `winhighlight`):
  applied on `BufWinEnter` of the preview buffer, previous values saved per window and restored on
  `BufWinLeave`, so the source buffer gets its own options back in toggle mode.

**Windowed loading**: let `top`/`bot` be the preview window's first/last visible rows and
`H` its height.
- *Want* = strips intersecting `[top − H, bot + H]`. Request the wanted strips not loaded (one
  `strips` request for all of them).
- *Evict* loaded strips entirely outside `[top − 3H, bot + 3H]`: `a=d,d=I,i=id`.
- Run on `WinScrolled`/`WinResized` of the preview window, after every layout response, and when
  the preview buffer is shown. Requests are serialised: at most one in flight; while one is in
  flight, just remember that loading needs re-evaluation.
- On a strips response: for each strip transmit `a=t,t=t,f=24,s=image_cols·cell_w,
  v=rows·cell_h,i=id` (path base64) and place `a=p,U=1,i=id,p=1,c=image_cols,r=rows`; mark
  loaded.
- After a new layout (`doc` changed) all loaded strips are stale: keep their images on screen
  until replaced, but treat all as not loaded so the wanted ones are re-requested; evict the
  rest. Delete images for indices ≥ new `nstrips`.
- When the preview buffer is no longer shown in any window (`BufWinLeave` and no other window
  shows it), delete all its images and mark none loaded. The server keeps the document.

**Layout requests**: sent when the preview is shown and the source `changedtick` or geometry
differs from the last layout; in split mode also on debounced `TextChanged`/`TextChangedI` (as
today) and on resize. Serialised with the strips requests (one in flight overall).

### 4. Plugin: `:DmbdipToggle` (in place)

- In a window showing a markdown buffer: create the preview state if missing (same checks as
  today: snacks, `termguicolors`, markdown filetype, executable), then
  `nvim_win_set_buf(win, preview_buf)` and position it.
- In a window showing a preview buffer: `nvim_win_set_buf(win, source_buf)` and position it.
- **Source → preview position**: preview topline = row interpolated from source topline (the
  existing `sync_scroll` interpolation); preview cursor on that row. If the layout is not yet
  current, position once the layout response arrives.
- **Preview → source position**: source topline = line interpolated back from preview topline
  over the same anchors (inverse of the above: between consecutive anchors, linear in row);
  restore the source cursor saved when toggling in if it lies inside the new visible range,
  else put the cursor on the new topline.
- **Source buffer unloading** *(builder-check call)*: with `hidden` off or a source
  `bufhidden` of unload/delete/wipe, switching the window away would unload the source. Teardown
  therefore listens only to `BufDelete`/`BufWipeout` of the source, not `BufUnload`; a source that
  gets unloaded by toggling is reloaded by `nvim_win_set_buf` on toggle-out. If
  `nvim_win_set_buf` fails (e.g. modified buffer with `nohidden`), the error is shown as a notice
  and nothing changes (wrap in `pcall`).
- If the preview buffer is already shown in a split, toggling in shows it in this window too;
  geometry follows the most recently entered window showing it. Not specially handled or tested.

### 5. Plugin: `:DmbdipPreview` (split)

Same behaviour as T-002 on top of the shared state: opens the preview buffer in a right split
(focus stays on the source), live layout on edit, source → preview scroll sync. Closing the split
window only hides the preview buffer (images deleted as above).

*(builder-check call)* The preview state records the split window it opened (`split_win`, or
none). `:DmbdipPreview`:
- from the source buffer, or from inside the split: if `split_win` is valid, close it (preview
  hidden, server kept); else open the split next to the current source window;
- from a window showing the preview in place (not `split_win`): notice "use :DmbdipToggle" and
  do nothing;
- the split's live re-render and scroll sync apply only while `split_win` is valid.

**Teardown**: when the source buffer is deleted/unloaded/wiped (as today), or `VimLeavePre`:
delete images, stop the job, wipe the preview buffer. If the preview buffer is wiped, the
same. Server exiting unexpectedly → error notice and teardown.

### 6. Docs and config

- README "Neovim preview": both commands, lazy.nvim spec with
  `keys = { { "<leader>mp", "<cmd>DmbdipToggle<cr>", desc = "Toggle markdown preview" } }` as an
  example, large-document note replaced (memory now bounded).
- `docs/DEVELOPMENT.md`: protocol v2 and windowed loading.
- User's own `~/.config/nvim/lua/plugins/dmbdip.lua`: add `cmd = { "DmbdipPreview",
  "DmbdipToggle" }` and the `<leader>mp` key (the user asked for it).
- `.project/TODO.md`: remove the long-documents item from T-003.

## Testing

- Rust: the equivalence test (above); TUI render unchanged (existing render tests); layout
  response fields (`image_cols`, `offset_cols` for a wide and a narrow window, strip count,
  padding); strips request writes files of exact size; stale doc → `stale`; out-of-range index →
  error; EOF cleanup.
- Performance check (reported, not a hard gate): a generated ~5,000-line document at
  `cols=200, cell_w=20, cell_h=42` (HiDPI): time of the layout request, time of one
  `strip_rows=50` strips request, and the server's peak RSS (`/usr/bin/time -v`). Compare with
  the T-002 full render of the same document.
- Lua headless smoke test (`tests/nvim/smoke.lua`, stubbed snacks): toggle in → preview buffer
  in the same window at the mapped row; only strips near the view transmitted; scrolling to the
  end transmits the last strips and deletes the first ones; toggle out → source buffer back with
  mapped topline, all images deleted; split mode still updates on edit; missing binary / protocol
  mismatch handled.
- Manual (user, in kitty): toggle on a large doc, scroll with `<C-d>`/`G`/`gg`, check memory of
  kitty stays flat, text crisp and centred, background matches.

## Left to the builder

- Internal structure of the render refactor and Lua modules.
- Optional block-image cache in the server.
- Exact debounce/timer wiring and autocmd layout.
