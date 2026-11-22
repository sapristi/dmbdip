# Neovim live preview: `dmbdip --serve` + minimal plugin (T-002, version A)

## Intent

A live markdown preview pane inside neovim, rendered by dmbdip, that follows unsaved buffer
edits and scrolls with the source. `:terminal dmbdip` can't work (neovim's terminal does not
pass kitty graphics through), and a kitty split only reloads on save with no scroll sync.

This is the first working version. Folding, heading keys, SSH delivery and smooth scrolling are
deferred to T-003 (`.project/TODO.md`).

## Goals

- `dmbdip --serve`: headless mode, no TUI, no terminal output, line-delimited JSON on
  stdin/stdout. Renders buffer text it is given, writes the page as horizontal strips of raw RGB
  to `/dev/shm`, and returns strip descriptions plus a source-line → cell-row map.
- A Lua plugin in this repo: `:DmbdipPreview` opens a vertical split showing the rendered page
  through kitty unicode placeholders, re-renders on edit (debounced), keeps the preview's top
  row in sync with the source window's top line, and re-renders on resize.

## Non-goals (this version)

- Folding, Tab/S-Tab heading navigation, Space toggle, fold arrows/cursor bar in neovim (T-003).
- Neovim over SSH (pixels through the pty) (T-003).
- Pixel-smooth scrolling (T-003). Preview scrolls by cell rows.
- Preview → source scroll sync. Sync is source → preview only.
- Changing the TUI's rendering/transfer (that's T-001).
- Very long documents: all strips stay in kitty for the preview's lifetime. Kitty caps image
  storage (~320 MB per session, shared with other images) and silently evicts the oldest ones,
  which would leave blank regions. Accepted for this version (roughly 4,000 source lines at
  normal DPI, ~4× fewer on HiDPI); keeping only near-view strips is in T-003.

## Anti-goals

- Typing in the source buffer must not feel slower. Rendering happens in the dmbdip process;
  neovim never handles pixel data, only paths and small JSON.
- Must not disturb other kitty images in the session (e.g. snacks.nvim images): use an image id
  range that won't collide, and only delete our own images.
- Kitty replies must never reach neovim's input: every graphics command uses `q=2`.

## Constraints

- Requires kitty-protocol terminal with unicode placeholder support (kitty; also works through
  tmux with passthrough, which `Snacks.image.terminal` handles), neovim with `termguicolors`.
- neovim and the terminal on the same machine (strip files read by the terminal).
- Linux and macOS: strip files go to `/dev/shm` when it exists, else `std::env::temp_dir()`
  (kitty accepts both as temp locations). *(builder-check call)*
- Unicode placeholders: one row diacritic per row, 297 diacritics → at most 297 rows per image.
  Image id is carried by the cell foreground colour (24-bit), so ids must be < 2^24.
- A virtual placement of `c` columns × `r` rows scales the image to that cell box, so each strip
  must be exactly `cols × cell_w` px wide and `rows × cell_h` px tall to avoid scaling.
- Rendering code is `pub(crate)` in the binary crate: the serve module lives in the binary
  (`src/serve.rs`), no lib split.

## Approaches considered (pixel delivery)

1. **Temp files in `/dev/shm` (picked).** dmbdip writes each changed strip as raw RGB to a
   file; Lua sends kitty `a=t,t=t,f=24,s=…,v=…` with the path; kitty reads then deletes it.
   Tiny protocol, nothing heavy through neovim. Local only.
2. Pixels inline: dmbdip returns ready escape sequences with base64/zlib pixels in the JSON;
   Lua passes them through. Works over SSH but pushes megabytes through the job channel and
   Lua on every re-render. Rejected for now (SSH path is in T-003).
3. POSIX shared memory (`t=s`). Same as 1 with `shm_open`; marginally faster, fiddlier
   cleanup. Not worth it since `/dev/shm` files are already in RAM.

## Design

### dmbdip side

**CLI.** `dmbdip --serve` (added to `parse_args` and `--help`). Skips graphics detection and
viewport size query. Loads config, theme, layout, fonts as the TUI does. Reads requests from
stdin line by line until EOF, then removes any of its leftover files and exits 0. Errors go to
stderr (and `--debug` log), never to stdout except as JSON responses.

**Dependency.** Add `serde_json`.

**Request** (one JSON object per line):

```json
{"id": 7, "type": "render", "text": "<buffer text>", "cols": 80, "cell_w": 10, "cell_h": 21}
```

- `cols`: preview window width in cells (Lua passes the window's text-area width).
- `cell_w`, `cell_h`: integer cell size in px (Lua rounds what `Snacks.image.terminal.size()`
  gives).

**Response** (one line):

```json
{"id": 7, "type": "render", "cols": 80, "total_rows": 412,
 "strips": [{"index": 0, "rows": 297, "path": "<tmpdir>/dmbdip-<pid>-tty-graphics-protocol-<n>.rgb"},
            {"index": 1, "rows": 115, "path": null}],
 "anchors": [[1, 0], [5, 3], [9, 12]]}
```

- Render width = `cols × cell_w` px. Call `render_markdown` with that width; pass the window
  height-independent value `vp_height = 0` (the bottom padding of `compute_total_height` isn't
  needed: neovim handles scrolling past the end).
- Pad the image height up to a multiple of `cell_h` with the theme background;
  `total_rows = height / cell_h`.
- Cut into strips of up to 297 rows (strip `i` covers rows `[297·i, 297·(i+1))`).
- `path` is set only when the strip's pixels differ from the last response's strip at the same
  index (compare by hash of the pixel bytes; also treat as changed when `cols`, `cell_w`,
  `cell_h` or the strip's row count changed). `null` means "unchanged, keep the image you have".
  First render: all strips have paths.
- File name must contain `tty-graphics-protocol` (kitty only deletes temp files whose path
  contains that string, after reading them). Content: raw RGB bytes, row-major, no header.
- `anchors`: for each rendered block, `[source_line (1-based), row]` with
  `row = floor(block_y / cell_h)`, from `parse_markdown`'s block source lines and
  `render_markdown`'s `block_positions`. Sorted by source line.
- Fold arrows (`▶/▼`, `src/render.rs`) are not drawn in serve mode: add a parameter/flag to
  `render_markdown` (TUI behaviour unchanged). No folds are applied (all headings unfolded).
- `MermaidCache` persists across requests in the serve loop.

**Errors.** Malformed line or render failure →
`{"id": <id or null>, "type": "error", "message": "..."}`; the server keeps running. A panic in
rendering should be caught (`catch_unwind`) and reported the same way.

**Requests are processed in order, synchronously.** Coalescing is the client's job.

### Neovim side

**Layout.** The repo root becomes a neovim plugin: `lua/dmbdip/*.lua` and `plugin/dmbdip.lua`,
so lazy.nvim can install `sapristi/dmbdip` directly. Add `lua/` and `plugin/` to `exclude` in
`Cargo.toml`. Depends on snacks.nvim (`Snacks.image.terminal.size()` for cell size,
`Snacks.image.terminal.request()` for graphics commands: adds `q=2` and tmux passthrough).

**Setup.** `require("dmbdip").setup({ cmd = "dmbdip", debounce_ms = 150 })` (all optional).
`plugin/dmbdip.lua` defines `:DmbdipPreview` (toggle for the current buffer).

**Open.** `:DmbdipPreview` on a markdown buffer:
- start `dmbdip --serve` with `vim.fn.jobstart` (stdin pipe, `on_stdout` line-buffered,
  `on_stderr` → `vim.notify` at debug/warn), one process per previewed source buffer;
- open a vertical split (right) with a scratch buffer (`buftype=nofile`, `bufhidden=wipe`,
  `swapfile=false`, `modifiable` toggled only when writing lines), window options
  `number=false`, `relativenumber=false`, `signcolumn=no`, `foldcolumn=0`, `wrap=false`,
  `list=false`, `cursorline=false`; focus returns to the source window;
- send a first render.

**Render requests.** On `TextChanged`/`TextChangedI` of the source buffer: debounce
(`debounce_ms`), then send. At most one request in flight: if one is pending, remember only that
a new one is needed and send it (with fresh text) when the response arrives. Also re-render on
`WinResized`/`VimResized` affecting the preview window (cols changed).

**Image ids.** Each preview picks a base id at random in `[0x100000, 0xFF0000)`; strip `i` uses
id `base + i`. Highlight group `DmbdipImg<id>` with `fg = string.format("#%06x", id)`.

**Applying a response.**
1. For each strip with a `path`: `request{ a="t", t="t", f=24, s=cols*cell_w, v=rows*cell_h,
   i=id, data=base64(path) }`, then a virtual placement `request{ a="p", U=1, i=id, c=cols,
   r=rows }` (re-transmitting an id replaces the image; recreate the placement each time to be
   safe).
2. Delete images for strip indices beyond the new strip count (`a=d,d=I,i=id`).
3. Rewrite the buffer lines only if `total_rows`, `cols` or any strip row count changed (a pure
   pixel change needs no buffer change): line `r` of strip `i` is `U+10EEEE` + row diacritic
   `r_local` + column diacritic 0, followed by `cols − 1` bare `U+10EEEE` (kitty infers the
   following columns). Highlight each strip's line range with `DmbdipImg<id>` via extmarks
   (`hl_group`, `hl_eol=false`). The 297-entry diacritics table is copied into the plugin
   (kitty's `rowcolumn-diacritics.txt`; snacks keeps its copy private).
4. Store `anchors`; re-apply scroll sync.

**Buffer, not window.** A preview is tied to its source buffer (state keyed by source bufnr).
If the source window switches to another file, the preview stays on the original buffer (no
follow). The scroll-driving window is the most recently entered window showing the source
buffer (track via `WinEnter`/`BufWinEnter`); if none shows it, sync pauses. *(builder-check
call)*

**Scroll sync.** On `WinScrolled` of the source window (and after each render): take the source
window's `topline`, find the last anchor with `line ≤ topline` and the next one, interpolate the
row linearly by line, and set the preview window's top line to `row + 1` with
`nvim_win_call(win, function() vim.fn.winrestview({ topline = … }) end)`. If topline is before
the first anchor, use row 0.

**Close.** `:DmbdipPreview` again, closing the preview window, or wiping the source buffer:
delete our images (`a=d,d=I` for each id), stop the job (close stdin → dmbdip exits and removes
leftover files). Also on `VimLeavePre`.

## Testing

- Rust: unit tests for strip splitting (row counts, padding to `cell_h`), change detection
  (unchanged text → all `path: null`; edit in one region → only that strip changes), anchors
  (rows monotonic, match block positions), JSON request/response round-trip, malformed line →
  error response and the loop continues. A test running the serve loop over in-memory
  reader/writer.
- Manual: open `docs/sample.md` in neovim inside kitty, `:DmbdipPreview`, type, scroll, resize,
  close; check `/dev/shm` holds no leftover `dmbdip-*` files.

## Documentation

- README: a "Neovim preview" section (install with lazy.nvim, requirements, `:DmbdipPreview`).
- `docs/DEVELOPMENT.md`: architecture note on serve mode and the protocol.

## Left to the builder

- Internal structure of `src/serve.rs` and the Lua modules.
- Hash function for change detection (std `DefaultHasher` is fine).
- How the fold-arrow suppression is threaded into `render_markdown` (bool param or a small
  options struct).
- Exact autocmd wiring and debounce implementation (`vim.uv` timer).
