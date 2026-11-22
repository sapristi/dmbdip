# Neovim preview: folding and heading navigation Plan

**Spec:** `.project/docs/specs/2026-11-22-17-52-nvim-preview-headings-design.md`
**Goal:** fold sections, jump between headings and show ▶/▼ arrows plus the current-heading bar
in the neovim preview, without re-rendering.
**Approach:** serve mode aligns heading slots to cell rows and returns heading rows/levels/colours
(protocol 3). The plugin creates neovim manual folds (displayed with `foldtext=""`), reads fold
state back from neovim only when it must rebuild them, draws arrows/bar as overlay extmarks
beside the image, and exposes functions the user maps (no key mappings in the plugin).

## Constraints

- TUI rendering stays pixel-identical: alignment applies only when `row_align > 1` (serve).
- `PROTOCOL = 3` in `src/serve.rs` and `lua/dmbdip/init.lua`.
- The plugin defines no key mappings. Public API: `require("dmbdip").next_heading()`,
  `.prev_heading()`, `.toggle_fold()`; commands `:DmbdipNextHeading`, `:DmbdipPrevHeading`,
  `:DmbdipFold` in `plugin/dmbdip.lua`.
- Preview buffer `filetype=dmbdip`.
- Neovim keeps the fold state; the plugin keeps no persistent list of folded headings. It reads
  `foldclosed()` only before rebuilding folds (re-layout, toggle out/in).
- Window-local preview options: `foldmethod=manual`, `foldenable=true`, `foldtext=""`,
  `fillchars` containing `fold: `, `winhighlight` containing `Folded:DmbdipFolded` (empty group).
  `signcolumn=no`, `foldcolumn=0`, `statuscolumn=""` stay.
- Arrow/bar: 2 reserved columns left of the image; plugin sends `cols − 2` in `layout` and writes
  `offset_cols + 2` leading spaces; overlay extmark at byte column `offset_cols`: bar `▎` (hl
  `DmbdipCursor`, fg = layout `cursor`) then arrow `▼`/`▶` (hl `DmbdipHeading<level>`, fg =
  heading `color`).
- Heading key = `level .. "\0" .. title .. "\0" .. n` (n = occurrence of that level+title so far).
- Docs commands: `cargo test`, `cargo build`, then
  `nvim --headless -u NONE --cmd 'set rtp^=.' -l tests/nvim/smoke.lua`.
- Don't include "Co-authored-by" in commit messages. Working tree was clean at plan time.

## Risks

- Aligning a heading must keep slots contiguous (each slot ends at the next slot's `y`) or
  `render_region`'s partition breaks → check in Task 1 test.
- A closed fold must keep the heading visible: fold starts at `end_row`, not `row` → Task 3
  smoke check that `foldclosed(row+1) == -1` and `foldclosed(end_row+1) == end_row+1`.
- A fold closed with plain `zc` (not via `toggle_fold`) must survive a re-layout after an edit
  that shifts lines → Task 3 smoke check.
- Reserving 2 columns must not shift strips/placeholders: the placeholder count per line stays
  `image_cols` → Task 3 smoke check on line content.
- Folded rows change `botline`; strip loading must still cover the visible rows → existing smoke
  checks keep passing plus a check after folding near the top.

## Tasks

### Task 1: Row alignment in `layout_blocks`

**Touches:** `src/render.rs`, `src/serve.rs` (call site only)

1. Add `row_align: u32` as the last parameter of `layout_blocks`. `render_markdown` and existing
   tests pass `1`; `src/serve.rs` passes `1` for now (Task 2 switches it to `cell_h`).
2. When `row_align > 1` and the block is a heading: round `y` up to a multiple of `row_align`
   before placing it, growing the previous slot's `height` so it still ends at the new `y`
   (if there is no previous slot, the first slot just starts later); after computing the
   heading's advance, round it up so `y + advance` is a multiple of `row_align`. The final
   `content_height`/last-slot fix-up stays as it is.
3. Add tests: with `row_align = 21` on `SAMPLE_MD` (and a doc starting with an H1, and one with a
   table and code before a heading): every heading slot's `y` and `y + height` are multiples of
   21; for consecutive slots `a.y + a.height == b.y`. `row_align = 1` behaviour is covered by the
   existing pixel-identity and height tests, which must pass unchanged.
   Also: `render_region` over aligned layouts equals the same rows of a full-page image drawn
   from the same aligned layouts (draw every slot with `render_region` over `[0, total)`).

**Check:** `cargo test` passes, including the existing pixel-identity tests. Commit
`feat(render): optional row alignment of heading slots`.

### Task 2: Serve protocol 3 — headings and cursor colour

**Touches:** `src/serve.rs`

1. `PROTOCOL = 3`. `layout_doc` passes `cell_h` as `row_align`.
2. Add to `LayoutResponse`: `cursor: String` (`#rrggbb` of `theme.cursor_color`) and
   `headings: Vec<HeadingEntry>`, `HeadingEntry { level: u8 (1..6), line: usize (1-based source
   line), row: u32, text_row: u32, end_row: u32, title: String, color: String }`, one per heading
   block in document order (all headings are laid out: serve never folds). From the heading's
   `BlockLayout`: `row = y / cell_h`, `end_row = (y + height) / cell_h − 1`, `text_row = (y +
   margin) / cell_h` where `margin = layout.h1_extra_margin` for H1 else 0. `title =
   spans_to_plain(spans)` (`src/text.rs`), `color` = `heading_style(level, theme).1` as
   `#rrggbb`.
3. Update the module doc comment and the protocol section of serve tests; add a test: layout of a
   doc with nested headings returns `headings` with `row ≤ text_row ≤ end_row`, rows strictly
   increasing, `row` of each heading > `end_row` of the previous one, `(row)` equal to its anchor
   row, `cursor` matching the theme, `protocol == 3`.

**Check:** `cargo test` passes. Commit `feat(serve): protocol 3 with heading rows and cursor colour`.

### Task 3: Plugin — folds, arrows/bar, heading functions

**Touches:** `lua/dmbdip/init.lua`, `plugin/dmbdip.lua`, `tests/nvim/smoke.lua`

1. `PROTOCOL = 3`. `geometry` keeps returning the window text width; `send_layout` sends
   `cols = geom.cols − 2` (skip the layout if that is < 1). `write_lines` uses
   `string.rep(" ", L.offset_cols + 2)` as lead (include it in `lines_key`).
2. Preview buffer: set `filetype=dmbdip` at creation. Window options: add the fold options from
   Constraints to `PREVIEW_WO` (`fillchars`: the window's current value with `fold: ` appended
   or replacing an existing `fold:` entry — compute at apply time; `winhighlight` gains
   `Folded:DmbdipFolded`). Define `DmbdipFolded` as `{}`.
3. Heading data helpers on `P.layout`: `headings` (from the response), `foldable(h)` (fold range
   longer than one row), `fold_range(L, i)` → `end_row, stop − 1` (stop = `row` of next heading
   with level ≤ `h.level`, else `total_rows`), `heading_key` per heading computed once per layout.
4. `read_closed(P, win)`: keys of foldable headings whose fold is closed in `win`
   (`foldclosed(end_row + 1) == end_row + 1`, evaluated via `nvim_win_call`). Nested closed folds
   inside a closed ancestor read as open (accepted limit).
5. `build_folds(P, win, closed)`: inside `nvim_win_call`, `winsaveview`, `normal! zE`, create one
   fold per foldable heading (`:{a},{b}fold` with 1-based lines), `normal! zR`, then for closed
   keys sorted by level descending (deepest first) `:{a}foldclose` with `a = end_row + 1`,
   `winrestview`. Then refresh decorations.
6. Rebuild points:
   - `apply_layout`: before `write_lines`, for each window in `win_findbuf(P.buf)` collect
     `read_closed` against the *old* layout (skip if no old layout); after `write_lines`,
     `build_folds` in each window with its collected keys. If `write_lines` was a no-op and the
     old and new heading rows are identical, skip the rebuild.
   - `toggle_out`: before switching buffers, store `P.toggle_closed[win] = read_closed(P, win)`.
   - `on_show(P, win)`: if `P.layout`, `build_folds(P, win, P.toggle_closed[win] or {})`, then
     clear `P.toggle_closed[win]`. A fresh split gets `{}`.
7. Decorations (namespace `dmbdip_marks`, separate from the image-highlight namespace):
   `decorate(P)` clears and, for each heading, puts an overlay extmark at
   `(text_row, offset_cols)` with chunks `{bar or " ", "DmbdipCursor"}`, `{arrow, hlgroup}`; for
   the current heading's other rows `[text_row + 1, end_row]` an overlay with `{"▎",
   "DmbdipCursor"}, {" "}`. Arrow is `▶` if foldable and closed in the current preview window
   (`foldclosed(end_row + 1) == end_row + 1`), else `▼`. Current heading = last heading with
   `row ≤ cursor_row` of the preview window (nil before the first heading: no bar). Highlights
   `DmbdipCursor` (fg = `cursor`) and `DmbdipHeading1..6` (fg from headings' `color`) set in
   `set_highlights`. Call `decorate` after `build_folds`, on `CursorMoved` and `WinScrolled` in
   the preview buffer (autocmds in the preview's augroup), and after `toggle_fold`.
8. Public functions in `M` (no-op if the current window doesn't show a preview buffer or there is
   no layout):
   - `M.next_heading()` / `M.prev_heading()`: current heading index `c` (0 = before the first);
     find the next/previous heading `h` that is visible (`foldclosed(h.row + 1)` is −1 or equals
     `h.end_row + 1`); none → no-op. Set cursor to line `h.row + 1`, `normal! zt`, then scroll up
     by `math.floor(winheight / 4)` screen lines with `<C-y>` (count; stops at buffer top), keep
     the cursor on the heading line. Then `decorate`.
   - `M.toggle_fold()`: current heading; if foldable, `:{end_row+1}foldopen` if closed else
     `:{end_row+1}foldclose` (on that fold only), cursor to `row + 1`, `decorate`.
   In `plugin/dmbdip.lua` add `:DmbdipNextHeading`, `:DmbdipPrevHeading`, `:DmbdipFold`.
9. Smoke tests (`tests/nvim/smoke.lua`, using its existing 100-section doc plus a small nested
   doc for fold nesting, e.g. `# A`/`## A1`/`### A1a`/`## A2`/`# B` each with a paragraph):
   - layout response has `headings`; preview buffer `filetype == "dmbdip"`; each line starts with
     `offset_cols + 2` spaces followed by `image_cols` placeholders.
   - folds exist for foldable headings and all start open after toggle in.
   - `toggle_fold` with the cursor inside section A1 closes A1's range: `foldclosed(end_row+1) ==
     end_row+1`, `foldclosed(row+1) == -1`, the next heading A2 row is not folded; cursor on A1's
     row.
   - nested: with A1 closed, close A → A1 and A2 hidden, B visible; `next_heading` from A skips
     A1/A2 and lands on B with B's row at `topline + floor(height/4)` (or topline 1
     when clamped).
   - `prev_heading` back to A.
   - `zc` on A2 via plain `normal! zc` with the cursor in A2, then edit the source (insert lines
     at the top) → after re-layout A2 is closed at its new rows.
   - toggle out and back in keeps closed folds.
   - decorations: an extmark with `▼`/`▶` on a heading's `text_row`; bar chunk present on the
     current heading's rows only.
   - all existing smoke checks still pass (strip loading, scroll sync, toggle positions).

**Check:** `cargo build && nvim --headless -u NONE --cmd 'set rtp^=.' -l tests/nvim/smoke.lua`
prints its success line and exits 0. Commit `feat(nvim): folding, heading navigation and arrows
in the preview`.

### Task 4: Documentation and task tracking

**Touches:** `README.md`, `docs/DEVELOPMENT.md`, `.project/TODO.md`

1. README "Neovim preview": add the three functions/commands and an example lazy.nvim `keys`
   spec using `ft = "dmbdip"` for `<Tab>`, `<S-Tab>`, `<Space>`; requirement neovim ≥ 0.10;
   remove "no folding or heading navigation in the preview yet" from known limits (keep the
   cell-row scrolling limit); mention that folds are neovim folds (`zc`/`zo` work).
2. `docs/DEVELOPMENT.md`: protocol 3 (`cursor`, `headings` fields), serve row alignment, the
   plugin's fold handling (neovim owns fold state, rebuild on layout/toggle, overlay decorations,
   2 reserved columns), link the new spec.
3. `.project/TODO.md` T-003: remove the folding, arrows/bar and keys bullets; keep source-fold
   mirroring, SSH delivery and sub-row smooth scrolling.

**Check:** `grep -n "no folding" README.md` prints nothing; `grep -in "protocol 3" docs/DEVELOPMENT.md`
finds the section. Commit `docs: nvim preview folding and heading navigation`.
