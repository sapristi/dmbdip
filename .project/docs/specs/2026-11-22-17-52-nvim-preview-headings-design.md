# Neovim preview: folding, heading arrows and keys (T-003, headings part)

Builds on `2026-11-22-15-30-nvim-toggle-large-docs-design.md` (protocol 2, windowed strips,
`:DmbdipToggle`). Covers only the headings part of T-003; mirroring source folds, SSH delivery
and sub-row smooth scrolling stay in T-003.

## Intent

Reading a document in the preview should feel like the dmbdip TUI: fold sections, jump between
headings, see which section you are in. Today the preview has none of it: serve mode renders no
fold arrows and no cursor bar, and the plugin has no keys.

## Goals

- **Folding without re-rendering**: a toggle-fold function (mapped by the user, e.g. to Space) folds/unfolds the current heading's section in the
  preview buffer using neovim manual folds. The heading stays fully visible; the section's
  content rows are hidden. Nested sections fold as in the TUI.
- **Current heading follows the neovim cursor**: the last heading whose first row is at or above
  the cursor row (after `j`, search, `<C-d>`, Tab…). The toggle-fold function acts on it, even when the cursor is
  deep inside the section.
- **Next / previous heading** (mapped by the user, e.g. Tab / S-Tab): move the cursor to the first row of the next / previous *visible* heading
  (headings inside a closed fold are skipped) and scroll so that row sits at 1/4 of the window
  height from the top, like `navigate_heading` (`src/state.rs`).
- **▶/▼ arrows and the current-heading bar drawn by neovim**, right beside the image, matching
  the TUI: arrow in the heading's colour on the heading's first text row (▼ open,
  ▶ folded); bar in the theme's `cursor_color` beside the current heading's rows only (not the
  whole section). Heading numbers stay in the image.

## Non-goals

- Mirroring source-buffer folds into the preview (stays in T-003).
- SSH delivery, sub-row smooth scrolling (stay in T-003).
- Fold persistence across neovim sessions.
- Key mappings: the plugin defines none (as today); it exposes functions and a `dmbdip`
  filetype, and the README shows example mappings.

## Anti-goals

- No re-layout or strip re-render on fold toggle or heading navigation: folding must be instant.
- TUI rendering stays pixel-identical (row alignment is serve-only).
- Must not break `:DmbdipToggle` position mapping or `:DmbdipPreview` scroll sync.

## Constraints

- Neovim folds work on buffer lines = preview cell rows. A heading must start on a cell-row
  boundary and the content after it must too, otherwise a fold would cut through a row and leave
  a sliver of the neighbouring block visible.
- A closed fold shows one line. With `foldtext=""` (neovim ≥ 0.10) that line is the fold's first
  buffer line rendered as is. **Verified by a throwaway spike** (headless `screenattr`): when the
  `Folded` highlight is cleared for the window (`winhighlight` maps `Folded` to an empty group),
  the closed-fold line keeps the extmark foreground exactly, so the placeholder still carries its
  image id. Without that mapping the default `Folded` group changes the attributes.
- Folds are window-local and buffer rows change on every re-layout, so the plugin must rebuild
  folds after a re-layout, carrying over which headings were closed.

## Approaches considered (folding)

- **A. Neovim manual folds (picked).** Server aligns heading slots to cell rows and returns per
  heading its rows; the plugin creates manual folds. Instant, no re-render, strips stay loaded.
  Costs a slightly different vertical rhythm in the preview (headings rounded to rows) and the
  `foldtext=""` trick (spike passed).
- **B. Server-side folding.** Plugin sends the folded set with `layout`; server lays out without
  hidden blocks like the TUI and visible strips are re-rendered. Simplest semantics, but each
  Space costs a re-layout plus strip reload (noticeable on mermaid-heavy pages) and goes against
  "no re-render". Rejected.

Drawing arrows and bar: overlay virtual text right beside the image (picked) rather than a
`statuscolumn` or signs: the image is centred, so a column at the window's left edge would sit far
from the headings in a wide window, and a sign has a single highlight group while the TUI shows
bar and arrow in different colours. *(my call, after the builder check)*

Fold state: neovim's folds are the only record (picked) rather than a plugin-side list of folded
headings, so every way of opening/closing a fold (`z` keys, mouse, `'foldopen'` on search) stays
consistent with the arrows and survives re-layouts. *(decided with the user)*

## Design

### Server (`src/render.rs`, `src/serve.rs`) — protocol 3

**Row alignment.** `layout_blocks` gains a `row_align: u32` parameter (TUI passes 1, serve passes
`cell_h`). When `row_align > 1`, before placing a heading block its `y` is rounded up to a
multiple of `row_align` (the previous slot grows to the new `y`, keeping the "slot runs to the
next block's y" invariant), and its slot height is rounded up so the next block also starts on a
multiple. With `row_align = 1` the layout is unchanged (existing pixel-identity tests keep
passing). Drawing uses the slots' `y`, so `render_region` needs no change beyond receiving the
aligned layouts; the heading's text sits at the top of its (possibly taller) slot.

**Layout response** gains:

- `cursor`: `"#rrggbb"`, the theme's `cursor_color`.
- `headings`: one entry per heading block, in document order:
  `{ "level": 1..6, "line": <source line, 1-based>, "row": <first row of slot>,
  "text_row": <row of the first text line>, "end_row": <last row of slot>,
  "title": "<plain heading text>", "color": "#rrggbb" }`.
  `row`/`end_row` come from the aligned slot (`y / cell_h`, `(y + height) / cell_h − 1`);
  `text_row` skips an H1's extra top margin. `color` is the heading level's colour
  (`heading_style`).

`PROTOCOL` becomes 3 in both `src/serve.rs` and the plugin (the plugin already refuses a
mismatching binary).

### Plugin (`lua/dmbdip/init.lua`)

**Neovim owns the fold state.** The plugin keeps no list of folded headings. It creates folds
and reads them back (`foldclosed()`) only when it has to rebuild them. Folding is plain neovim:
`za`/`zo`/`zc`/`zR`/`zM`, mouse, `'foldopen'` all just work. Folds are window-local, as in
neovim: the toggled window and a split may fold different sections.

**Fold ranges.** For heading `h`: rows `[h.end_row, stop − 1]` where `stop` is the `row` of the
next heading with level ≤ `h.level`, or `total_rows` (same rule as `is_block_folded`,
`src/headings.rs`). Starting at the heading's *last* row makes the closed-fold line show that
row as is, so the whole heading stays visible. A heading whose range is a single row (no
content before the next same-or-higher heading) is not foldable: no fold, no arrow change.

**Heading keys.** To carry fold state across a re-layout, headings are matched by key =
`level .. "\0" .. title .. "\0" .. n`, `n` = occurrence count of that (level, title) so far in
the document. Survives edits that move lines. *(my call)*

**Building folds.** `build_folds(P, win, closed_keys)`: with the view saved
(`winsaveview`/`winrestview`): `zE`, create one fold per foldable heading (`:{a},{b}fold`,
1-based), then open all (`zR`/`foldlevel` high), then close (`:{a}foldclose`) those whose key is
in `closed_keys`, deepest first so ancestors are applied last. Nesting follows the ranges.

**When folds are rebuilt.**
- *Re-layout* (edit, resize, cell size): for each window showing the preview, read the closed
  keys from the *old* layout (`foldclosed(h.end_row + 1) == h.end_row + 1` for each foldable
  heading) just before replacing the buffer lines, then rebuild with the new layout and those
  keys. A layout that doesn't change the buffer lines (`write_lines` no-op) and has identical
  heading rows needs no rebuild. Known limit: a closed fold nested inside a closed ancestor
  can't be read without opening the ancestor (`foldclosed()` returns the outermost), so it comes
  back open after a re-layout. Accepted. *(my call)*
- *Toggle out / back in*: on toggle out, record the closed keys and the layout `doc` id for that
  window (neovim may restore manual folds itself from its per-window buffer info; don't rely on
  it). On show, rebuild from the recorded keys (empty on first show).
- New split window: rebuild with no closed keys.

**Filetype.** The preview buffer gets `filetype=dmbdip`, so users can map keys for the preview
only (lazy.nvim `keys = { { …, ft = "dmbdip" } }` or a `FileType dmbdip` autocmd). Set it in a
way that doesn't trigger markdown plugins (it's a new filetype; nothing else attaches).

**Public functions (no key mappings).** The plugin defines no keys, as today.
`require("dmbdip").next_heading()`, `.prev_heading()`, `.toggle_fold()`, acting on the current
window if it shows a preview buffer, no-op otherwise. Optionally matching commands
`:DmbdipNextHeading`, `:DmbdipPrevHeading`, `:DmbdipFold` *(builder's call)*.

- **Current heading**: last heading whose `row` ≤ cursor row (0-based `lnum − 1`). A cursor in a
  closed fold sits on its first line (the heading's `end_row`), which yields that heading.
- `next_heading` / `prev_heading`: from the current heading (or "before the first heading"),
  find the next / previous visible heading: visible iff `foldclosed(h.row + 1)` is −1 or
  `h.end_row + 1` (its own closed fold starts at `end_row`; inside an ancestor's closed fold it
  returns the ancestor's start). If
  none, do nothing. Otherwise put the cursor on `row + 1` and scroll so that line is
  `floor(height / 4)` screen lines below the window top (e.g. `zt` then `<C-y>` × n, which counts
  screen lines and respects folds; clamp at the buffer top).
- `toggle_fold`: if the current heading is foldable, toggle its fold (`za` on line
  `end_row + 1`), then put the cursor on `row + 1` so it doesn't end up in the hidden range.

**Arrows and bar.** Drawn as extmark overlay virtual text (`virt_text_pos = "overlay"`) in two
columns right beside the image, not in a status column: in a wide window the image is centred
and a status column would sit far from the headings. Overlay chunks can each have their own
highlight, so bar and arrow get different colours. The plugin reserves the 2 columns: it sends
`cols − 2` in `layout` and writes each line with `offset_cols + 2` leading spaces (the image is
then 1 column right of centre). Column `offset_cols` = bar `▎` in `DmbdipCursor` (fg =
`cursor`) on rows `[text_row, end_row]` of the current heading; column `offset_cols + 1` = `▼`
or `▶` in `DmbdipHeading<n>` (fg = heading `color`) on each heading's `text_row`, ▶ when its fold
is closed. Updated on `CursorMoved` (current heading changed), after rebuilding folds, and after
any fold change — since folds can change without an event, refresh the arrows on
`CursorMoved`/`WinScrolled` and after `toggle_fold` by re-reading `foldclosed()` for the
headings in view (cheap). Extmarks live in a separate namespace; overlays on visible rows only
is acceptable.

**Window options** added to `PREVIEW_WO` (already saved/restored by toggle):
`foldmethod=manual`, `foldenable=true`, `foldtext=""`, `fillchars` with `fold: `, and
`winhighlight` extended with `Folded:DmbdipFolded` (an empty group; required, see Constraints).
`foldcolumn=0` and `signcolumn=no` stay.

**Interaction with existing behaviour.**
- Windowed strip loading uses `topline`/`botline` from `getwininfo`; `botline` already accounts
  for folds. Keep the current logic: closed sections inside `[top − H, bot + H]` just load a few
  strips that are not displayed. *(my call)*
- Toggle out (`row_to_line` on the topline) and source → preview scroll sync keep using buffer
  rows; a target row inside a closed fold lands on the fold line, which is fine.

## Testing

- Rust: `layout_blocks` with `row_align = cell_h`: every heading slot `y` and `y + height` are
  multiples of `cell_h`; slots stay contiguous; `row_align = 1` gives the old layout. Serve
  layout response contains `headings` with consistent `row ≤ text_row ≤ end_row`, rows increasing,
  and `cursor`. `render_region` with aligned layouts still matches the same rows of a full render
  done with the same aligned layout.
- Lua (`tests/nvim/smoke.lua`, headless, stubbed snacks): fold ranges for a sample with nested
  headings; `toggle_fold` closes the expected range (`foldclosed`) and the heading's rows stay
  visible; nested close/open; a fold closed with plain `zc` is kept after a re-layout following an
  edit that shifts lines; `next_heading`/`prev_heading` skip headings inside closed folds and put
  the cursor on the heading row; arrow/bar extmarks for a heading row, a current-heading row and a
  plain row; toggle out and back in keeps closed folds; preview buffer has `filetype=dmbdip`.
- Manual: `docs/sample.md` in kitty, toggle in, Tab through, Space fold/unfold nested sections,
  edit in the split while sections are folded.

## Documentation

- README "Neovim preview": the three functions and an example `keys` spec with `ft = "dmbdip"`
  (Tab / S-Tab / Space), drop "no folding or heading navigation" from
  known limits, neovim ≥ 0.10 requirement.
- `docs/DEVELOPMENT.md`: protocol 3 fields, row alignment, plugin fold state.
- T-003 entry in `.project/TODO.md`: remove the done items, keep source-fold mirroring, SSH
  delivery and smooth scrolling.

## Left to the builder

- Exact Lua structure (helper names).
- How arrow/bar extmarks are refreshed (all rows vs visible rows).
- Search strategy for the current heading and fold ranges.
