# Neovim Toggle and Large Docs Plan

**Spec:** `.project/docs/specs/2026-11-22-15-30-nvim-toggle-large-docs-design.md`
**Goal:** In-place `:DmbdipToggle` between source and preview, with memory bounded independently of document size (on-demand strip rendering in the server, windowed strip loading in kitty, content-width images).
**Approach:** Refactor `src/render.rs` into a layout pass + per-block drawing + region rendering (pixel-identical to today), make mermaid layout size-only, replace the serve protocol with `layout` + `strips` requests (protocol 2), then rework the Lua plugin around a per-source preview state shared by the in-place toggle and the split.

## Constraints

- The spec is authoritative for protocol fields (`layout`/`strips`/`stale`/`error`, `"protocol": 2`, `doc`, `image_cols`, `offset_cols`, `total_rows`, `strip_rows`, `nstrips`, `bg`, `anchors`), the image width rule, the windowed-loading ranges (`[top − H, bot + H]` want, outside `[top − 3H, bot + 3H]` evict), and the `:DmbdipPreview` / `:DmbdipToggle` behaviours.
- TUI output must stay pixel-identical; TUI `MermaidCache` stays unbounded.
- Server mermaid pixel cache bound: 16 images (LRU).
- `strip_rows` clamped to `[1, 297]`.
- Strip file naming, location (`/dev/shm` else temp dir) and EOF cleanup unchanged from T-002.
- The plugin defines no key mappings.
- Every kitty command via `Snacks.image.terminal.request()`.
- Commit messages: conventional style, no "Co-authored-by". Stage by path.
- Uncommitted at plan time in this repo: none. `~/.config/nvim/lua/plugins/dmbdip.lua` is the user's file (untracked in their config repo): edit it only as Task 5 says, never commit in `~/.config/nvim`.

## Risks

- Region rendering differs from full render at block boundaries (glyph overflow, H1 margin, table/code backgrounds) → pixel-equivalence test over several documents and several strip heights (Task 1).
- Layout still rasterizing mermaid → test that layout of a doc with diagrams leaves the pixel cache empty (Task 2).
- Stale strip responses after an edit or resize painting old pixels → `doc` mismatch returns `stale` and the plugin drops it; smoke test edits during an in-flight strips request (Tasks 3, 4).
- Toggle losing the reading position → smoke test round trip: toggle in and out at a mid-document topline, source topline back within ±1 line of the original (Task 4).
- Kitty memory growing while scrolling → smoke test: after scrolling top→bottom, the number of loaded strips never exceeds the strips intersecting `[top − 3H, bot + 3H]` and the first strips were deleted (Task 4).

## Tasks

### Task 1: Render refactor — layout pass, block drawing, region rendering

**Touches:** `src/render.rs` (and `src/state.rs` only if signatures it uses change)

1. Add `pub(crate) struct BlockLayout { block_index: usize, y: u32, height: u32 }` and `pub(crate) fn layout_blocks(blocks, headings, width, fonts, theme, layout, mermaid_cache) -> (Vec<BlockLayout>, u32)` (second value = content height without the TUI's `vp_height / 2` padding). Slot = from block `y` to next block's `y` (last block: to the end height before the final padding); an H1's `y` is where its extra margin starts (same as today's `block_positions`). Folded blocks skipped.
2. Rewrite `compute_total_height` as `layout_blocks(..).1 + layout.paragraph_gap + vp_height / 2` (or whatever reproduces today's exact value; check against the current implementation).
3. Factor the per-block `match` of `render_markdown` into a function drawing one block at a given `y` onto an image (arrows flag passed through); it also sets `headings[hi].y_pos`/`heading_height` as today.
4. Add `pub(crate) fn render_region(blocks, headings, width, fonts, theme, layout, mermaid_cache, fold_arrows, layouts: &[BlockLayout], y0: u32, height: u32) -> RgbImage`: background-filled `width × height`; each block whose slot overlaps `[y0, y0+height)` is drawn into a scratch image of its slot height at y = 0 (heading y fields set to the block's absolute y afterwards if anything reads them) and the overlapping rows copied in.
5. Make `render_markdown` use `layout_blocks` + the block function; signature and output unchanged.
6. Tests: (a) for `SAMPLE_MD`, `docs/sample.md` (`include_str!("../docs/sample.md")`) and a generated long document (≥ 600 rendered rows at 21 px cells, mixing H1/H2, paragraphs, code blocks, a table, lists, one mermaid block), with `width = 970`, `fold_arrows = false`: stacking `render_region` strips of heights 21×48 and 21×297 (and one odd height, e.g. 1000 px) is byte-identical to `render_markdown(..., vp_height = 0, ...)` over the same height (pad comparison area with `bg`); (b) `compute_total_height` unchanged for the existing test docs (assert against values computed by the old code before refactoring — record them first); (c) all existing render tests pass unchanged.

**Check:** `cargo test` passes, 0 warnings in `cargo build`. Commit `refactor(render): layout pass and region rendering`.

### Task 2: Mermaid size-only layout and bounded pixel cache

**Touches:** `src/mermaid.rs`, `src/render.rs`, `src/state.rs` / `src/browser.rs` (constructor call sites only)

1. `MermaidCache` gains a size cache `key → (svg: String, w: u32, h: u32)` (or the error image's size for failures) and a method `size(&mut self, source, target_width, dark, fonts, theme) -> (u32, u32)` that renders to SVG and computes the output size with the same rule as `rasterize_svg_to_rgb` (`src/mermaid.rs:104`) without rasterizing. `get_or_render` reuses the cached SVG when present.
2. Pixel cache gets an optional capacity: `MermaidCache::new()` stays unbounded (TUI), `MermaidCache::with_capacity(16)` evicts least-recently-used images.
3. `layout_blocks` uses `size()` for mermaid blocks; drawing uses `get_or_render`.
4. Tests: layout of a doc with two mermaid blocks fills the size cache and leaves the pixel cache empty; a capacity-2 cache rendering 3 diagrams holds 2; Task 1's equivalence test still passes.

**Check:** `cargo test` passes. Commit `perf(mermaid): size-only layout and optional LRU pixel cache`.

### Task 3: Serve protocol v2

**Touches:** `src/serve.rs`, `src/main.rs` (only if the entry point signature changes)

1. Remove the `render` request. Add `layout` and `strips` handling and the `stale` response exactly as in spec §2; every response (including errors and `stale`) carries `"protocol": 2`.
2. Server state: parsed blocks, headings, `Vec<BlockLayout>`, geometry (`width_px`, `image_cols`, `cell_w`, `cell_h`, `strip_rows`, `total_rows`), `doc` counter, `MermaidCache::with_capacity(16)`. No full-page image is ever allocated.
3. Width rule: `width = min(cols × cell_w, max_content_width + margin_left + margin_right)` rounded down to a multiple of `cell_w`; error if `≤ margin_left + margin_right + block_indent`; keep the existing caps (20000 px width, 1000 px cells). `offset_cols = (cols − image_cols) / 2`. `bg` formatted `#rrggbb` from `theme.bg`.
4. `strips`: for each index, `render_region(.., y0 = index·strip_rows·cell_h, height = rows·cell_h)` (rows of the last strip may be fewer; pixels past the content are `bg`), write file, return `{index, rows, path}`. Requests before any layout → error.
5. Replace the T-002 serve tests with: layout fields for a wide window (`cols=200, cell_w=10` → image capped, offset > 0) and a narrow one (offset 0); `nstrips = ceil(total_rows / strip_rows)`; `strip_rows` clamping (0 → 1, 1000 → 297); strips files have exact byte size; stitched strips equal `render_markdown` for the same width (reuse Task 1 helper); `doc` mismatch → `stale`; out-of-range index → error; malformed JSON → error and loop continues; EOF cleanup.
6. Performance measurement (report numbers in the final report, not a gate): generate a ~5,000-line markdown doc into the scratch/temp dir (repeat `docs/sample.md` content), build `--release`, and time through `--serve` with `/usr/bin/time -v`: one `layout` at `cols=200, cell_w=20, cell_h=42, strip_rows=50`, then one `strips` request for index 0 and one for the middle index; record each request's time (from the `--debug` log, `DMBDIP_DEBUG_LOG` pointing into the temp dir) and peak RSS. For comparison, run the same doc through the T-002 binary (`git stash` is not allowed; instead use `git worktree add <tmp> e4bb2c9`, build there, run one T-002 `render` request with the same geometry, then `git worktree remove`). Remove generated strip files afterwards.

**Check:** `cargo test` passes; manual: `printf '%s\n' '{"id":1,"type":"layout","text":"# Hi\n\nworld","cols":200,"cell_w":10,"cell_h":20,"strip_rows":10}' '{"id":2,"type":"strips","doc":1,"indices":[0]}' | cargo run -q -- --serve` prints a layout line with `image_cols` = 97 (970/10) and `offset_cols` = 51, then a strips line. Commit `feat(serve): protocol v2 with on-demand strip rendering`.

### Task 4: Plugin rework — shared state, windowed loading, toggle, split

**Touches:** `lua/dmbdip/init.lua` (split into further modules under `lua/dmbdip/` at the implementer's discretion), `plugin/dmbdip.lua`, `tests/nvim/smoke.lua`

1. Per-source preview state as spec §3: job, preview buffer (`bufhidden=hide`), id base, last layout + source `changedtick` + geometry, loaded set, `split_win`, saved window options per window.
2. Preview buffer lines and extmarks per spec §3 (offset spaces, placeholders on `image_cols` columns, `DmbdipImg<id>` highlight only on placeholder columns); `DmbdipBg` from `layout.bg`; window options + `winhighlight=Normal:DmbdipBg,EndOfBuffer:DmbdipBg` applied on `BufWinEnter` of the preview buffer, saved/restored on `BufWinLeave`.
3. Request queue: at most one request in flight overall; layout requests win over strips; `stale` responses dropped; protocol ≠ 2 → error notice "dmbdip binary protocol mismatch (got X, want 2): reinstall dmbdip" and teardown.
4. Windowed loading per spec §3 (want / evict ranges, triggers, after-layout handling, delete images ≥ `nstrips`, delete all when the preview buffer is no longer shown in any window).
5. `:DmbdipToggle` per spec §4 (position mapping both ways, saved source cursor rule, unloading rule: teardown on source `BufDelete`/`BufWipeout` only, `pcall(nvim_win_set_buf)` with notice on failure).
6. `:DmbdipPreview` per spec §5 (`split_win` rules; live layout on debounced edits and resize and scroll sync only while `split_win` is valid; notice "use :DmbdipToggle" from an in-place preview window).
7. `plugin/dmbdip.lua`: define both commands.
8. Rewrite `tests/nvim/smoke.lua` (stubbed snacks, `cmd = target/debug/dmbdip`, generated ~1,500-line doc so there are many strips at the stub's window height) asserting: toggle in → same window shows the preview buffer at the mapped row; only strips intersecting `[top − H, bot + H]` were transmitted; scrolling to the end (`normal! G`, then trigger `WinScrolled` handling) transmits the last strips and deletes the first ones; loaded count never exceeds the evict range's strip count; toggle out → source buffer in the same window, topline within ±1 line of the original, all images deleted; editing the source while the preview is hidden then toggling in → new layout (`doc` increases); split mode: `:DmbdipPreview` opens a split, an edit re-layouts and re-requests visible strips, `:DmbdipPreview` again closes the split and keeps the server; `:DmbdipPreview` from an in-place preview → notice only; source `:bdelete!` → job exits, preview buffer wiped; missing binary → one error, no window/state; protocol mismatch (stub job printing `{"protocol":1,...}` via `cmd` set to a small shell script in the temp dir) → error notice and teardown; no `dmbdip-*-tty-graphics-protocol-*` files of the job's pid left in `/dev/shm`.

**Check:** `cargo build && nvim --headless -u NONE --cmd 'set rtp^=.' -l tests/nvim/smoke.lua` prints `ok`, exit 0. Commit `feat(nvim): in-place toggle and windowed strip loading`.

### Task 5: Docs, user config, task bookkeeping

**Touches:** `README.md`, `docs/DEVELOPMENT.md`, `.project/TODO.md`, `~/.config/nvim/lua/plugins/dmbdip.lua` (user's file, outside the repo), `~/.local/bin/dmbdip` (reinstall)

1. README "Neovim preview": both commands, lazy.nvim spec `{ "sapristi/dmbdip", dependencies = { "folke/snacks.nvim" }, cmd = { "DmbdipPreview", "DmbdipToggle" }, keys = { { "<leader>mp", "<cmd>DmbdipToggle<cr>", desc = "Toggle markdown preview" } }, opts = {} }`; replace the long-document limitation with a note that memory is bounded.
2. `docs/DEVELOPMENT.md`: protocol v2, windowed loading, render refactor (layout pass / region rendering).
3. `.project/TODO.md`: remove the "Long documents" bullet from T-003.
4. User config `~/.config/nvim/lua/plugins/dmbdip.lua`: set `cmd = { "DmbdipPreview", "DmbdipToggle" }` and add `keys = { { "<leader>mp", "<cmd>DmbdipToggle<cr>", desc = "Toggle markdown preview" } }`, keeping the rest (`dir`, `name`, `dependencies`, `opts`) and its tab indentation. Do not commit anything in `~/.config/nvim`.
5. Reinstall the binary: `cargo install --path . --root ~/.local` (protocol 2 is required by the new plugin).

**Check:** `git diff --stat HEAD~1` (after commit) lists only README, DEVELOPMENT, TODO; `echo '{"id":1,"type":"layout","text":"x","cols":80,"cell_w":10,"cell_h":20,"strip_rows":10}' | dmbdip --serve` prints a line with `"protocol":2`; `nvim --headless +'lua print(vim.fn.exists(":DmbdipToggle"))' +qa` prints `2`. Commit `docs: nvim in-place toggle and bounded memory`.
