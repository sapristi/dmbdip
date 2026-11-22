-- Headless smoke test for the neovim preview plugin.
-- Run from the repo root after `cargo build`:
--   nvim --headless -u NONE --cmd 'set rtp^=.' -l tests/nvim/smoke.lua

local root = vim.fn.getcwd()
local PLACEHOLDER = vim.fn.nr2char(0x10EEEE)
local CELL_W, CELL_H = 20, 20 -- headless windows are 80 columns: wide enough to centre
local CMD = root .. "/target/debug/dmbdip"

local function fail(msg)
  io.stderr:write("FAIL: " .. msg .. "\n")
  os.exit(1)
end

local function check(cond, msg)
  if not cond then
    fail(msg)
  end
end

-- Stub snacks: fixed cell size, record graphics requests.
local requests = {}
_G.Snacks = {
  image = {
    terminal = {
      size = function()
        return {
          cell_width = CELL_W,
          cell_height = CELL_H,
          columns = vim.o.columns,
          rows = vim.o.lines,
          width = vim.o.columns * CELL_W,
          height = vim.o.lines * CELL_H,
        }
      end,
      request = function(opts)
        requests[#requests + 1] = vim.deepcopy(opts)
      end,
    },
  },
}

local notes = {}
vim.notify = function(msg, level)
  notes[#notes + 1] = { msg = msg, level = level }
end

vim.o.columns = 200
vim.o.lines = 50
vim.o.termguicolors = true
vim.o.hidden = false -- toggling in unloads the source: it must come back
vim.cmd("filetype on")
vim.cmd("runtime plugin/dmbdip.lua")

local dmbdip = require("dmbdip")
dmbdip.setup({ cmd = CMD, debounce_ms = 10 })

-- A ~1,500-line document: many strips at this window height.
local doc_lines = {}
for i = 1, 100 do
  vim.list_extend(doc_lines, {
    "## Section " .. i,
    "",
    "Paragraph " .. i .. " with some words to fill the line.",
    "",
    "```",
    "code line one " .. i,
    "code line two " .. i,
    "```",
    "",
    "- item a " .. i,
    "- item b " .. i,
    "",
    "Closing words " .. i .. ".",
    "",
  })
end
local doc_path = vim.fn.tempname() .. ".md"
vim.fn.writefile(doc_lines, doc_path)

vim.cmd("silent edit " .. doc_path)
local src = vim.api.nvim_get_current_buf()
local win = vim.api.nvim_get_current_win()
check(vim.bo[src].filetype == "markdown", "doc should be markdown")
vim.wo[win].number = true

local function by(kind)
  return vim.tbl_filter(function(r)
    return r.a == kind
  end, requests)
end

local function wait_idle(P, what)
  check(vim.wait(30000, function()
    return P.layout ~= nil and not P.in_flight
  end, 10), "no response: " .. what)
end

local function topline(w)
  return vim.fn.getwininfo(w)[1].topline
end

--- Strip indices intersecting rows [a, b] (0-based).
local function strips_between(P, a, b)
  local L = P.layout
  a = math.max(0, a)
  b = math.min(L.total_rows - 1, b)
  local set = {}
  for i = math.floor(a / L.strip_rows), math.floor(b / L.strip_rows) do
    set[i] = true
  end
  return set
end

local function view(w)
  local info = vim.fn.getwininfo(w)[1]
  return info.topline - 1, info.botline - 1, info.height
end

local function count(set)
  local n = 0
  for _ in pairs(set) do
    n = n + 1
  end
  return n
end

-- Check loaded strips are within the evict range; return the count.
local function check_bounded(P, w)
  local top, bot, H = view(w)
  local keep = strips_between(P, top - 3 * H, bot + 3 * H)
  for index in pairs(P.images) do
    check(keep[index], ("strip %d kept outside the evict range"):format(index))
  end
  check(count(P.images) <= count(keep), "too many strips loaded")
end

---------------------------------------------------------------------------
-- Toggle in at a mid-document line.
---------------------------------------------------------------------------
local MID = 700
vim.fn.winrestview({ topline = MID, lnum = MID + 3 })
vim.cmd("DmbdipToggle")
local P = dmbdip._previews[src]
check(P ~= nil, "toggle created no preview")
check(vim.api.nvim_get_current_win() == win, "toggle must stay in the same window")
check(vim.api.nvim_win_get_buf(win) == P.buf, "window should show the preview buffer")
local pid = vim.fn.jobpid(P.job)
wait_idle(P, "first layout")
check(P.layout.nstrips > 10, "expected many strips, got " .. P.layout.nstrips)
check(vim.wo[win].number == false, "preview window options applied")
check(vim.wo[win].winhighlight:find("Normal:DmbdipBg"), "winhighlight set")
check(vim.api.nvim_get_hl(0, { name = "DmbdipBg" }).bg == tonumber(P.layout.bg:sub(2), 16), "DmbdipBg from layout")

-- Mapped position.
local want_top = math.floor(dmbdip._line_to_row(src, MID)) + 1
check(topline(win) == want_top, ("preview topline %d, want %d"):format(topline(win), want_top))
check(want_top > 1, "mid-document maps below the top")

-- Buffer content: centring spaces and 2 reserved columns, then image_cols
-- placeholders.
local L = P.layout
check(type(L.headings) == "table" and #L.headings == 100, "layout has the headings")
check(vim.bo[P.buf].filetype == "dmbdip", "preview filetype")
local lines = vim.api.nvim_buf_get_lines(P.buf, 0, -1, false)
check(#lines == L.total_rows, ("line count %d ~= total_rows %d"):format(#lines, L.total_rows))
check(L.offset_cols > 0, "wide window centres the image " .. vim.inspect({L.image_cols, L.offset_cols, L.geom}))
local lead = string.rep(" ", L.offset_cols + 2)
for i = 1, #lines, 97 do
  check(lines[i]:sub(1, #lead) == lead, "line " .. i .. " lacks centring spaces")
  check(lines[i]:sub(#lead + 1, #lead + #PLACEHOLDER) == PLACEHOLDER, "line " .. i .. " placeholder")
end
check(vim.fn.strchars(lines[1]) == L.offset_cols + 2 + L.image_cols + 2, "cols placeholders + 2 diacritics")
for i = 1, #lines, 97 do
  local n = select(2, lines[i]:gsub(PLACEHOLDER, ""))
  check(n == L.image_cols, ("line %d has %d placeholders, want %d"):format(i, n, L.image_cols))
end
check(L.offset_cols + 2 + L.image_cols <= vim.api.nvim_win_get_width(win), "image and reserved columns fit")
local img_ns = vim.api.nvim_get_namespaces()["dmbdip"]
local marks = vim.api.nvim_buf_get_extmarks(P.buf, img_ns, { L.strip_rows, 0 }, { L.strip_rows, -1 }, { details = true })
check(#marks == 1 and marks[1][3] == L.offset_cols + 2, "highlight starts at the placeholders")
check(marks[1][4].hl_group == "DmbdipImg" .. (P.base_id + 1), "row strip_rows belongs to strip 1")

-- Only strips near the view were transmitted.
local top, bot, H = view(win)
local wanted = strips_between(P, top - H, bot + H)
local sent = {}
for _, r in ipairs(by("t")) do
  sent[r.i - P.base_id] = true
  check(r.t == "t" and r.f == 24, "transmit uses t=t, f=24")
  check(r.s == L.image_cols * CELL_W, "transmit width")
  check(vim.base64.decode(r.data):find("tty%-graphics%-protocol"), "file path")
  check(r.i >= 0x100000 and r.i < 0x1000000, "id range")
end
check(vim.deep_equal(sent, wanted), "transmitted " .. vim.inspect(vim.tbl_keys(sent)) .. ", wanted " .. vim.inspect(vim.tbl_keys(wanted)))
for _, r in ipairs(by("p")) do
  check(r.U == 1 and r.c == L.image_cols, "virtual placement over the image columns")
end
for _, r in ipairs(requests) do
  check(r.q == nil or r.q == 2, "request must not override q")
end
check_bounded(P, win)

-- Toggle straight back out: same position.
vim.cmd("DmbdipToggle")
check(vim.api.nvim_win_get_buf(win) == src, "toggle out shows the source")
check(math.abs(topline(win) - MID) <= 1, ("source topline %d, want %d"):format(topline(win), MID))
check(vim.api.nvim_win_get_cursor(win)[1] == MID + 3, "source cursor restored")
check(vim.wo[win].number == true, "source window options restored")
vim.wait(100)
check(count(P.images) == 0, "images deleted once the preview is hidden")
local deleted = {}
for _, r in ipairs(by("d")) do
  deleted[r.i] = true
end
for _, r in ipairs(by("t")) do
  check(deleted[r.i], "image " .. r.i .. " not deleted")
end

---------------------------------------------------------------------------
-- Toggle in again, scroll to the end: windowed loading.
---------------------------------------------------------------------------
local doc_before = P.layout.doc
requests = {}
vim.cmd("DmbdipToggle")
wait_idle(P, "toggle in again")
check(P.layout.doc == doc_before, "unchanged source: no new layout")
check(#by("t") > 0, "strips re-sent after hiding")
local first_loaded = math.huge
for index in pairs(P.images) do
  first_loaded = math.min(first_loaded, index)
end
vim.cmd("normal! G")
dmbdip._pump(src)
wait_idle(P, "scroll to end")
local last = P.layout.nstrips - 1
check(P.loaded[last], "last strip loaded after scrolling to the end")
local first_deleted = false
for _, r in ipairs(by("d")) do
  if r.i == P.base_id + first_loaded then
    first_deleted = true
  end
end
check(first_deleted, "first loaded strip deleted after scrolling to the end")
check_bounded(P, win)

-- Back to the middle by rows, then toggle out: source follows the preview.
local row = math.floor(P.layout.total_rows / 3)
vim.fn.winrestview({ topline = row + 1, lnum = row + 1 })
dmbdip._pump(src)
wait_idle(P, "scroll to a third")
check_bounded(P, win)
vim.cmd("DmbdipToggle")
local back = topline(win)
local mapped = dmbdip._line_to_row(src, back)
check(math.abs(mapped - row) <= 3, ("source line %d maps to row %d, preview was at %d"):format(back, mapped, row))
check(vim.api.nvim_win_get_cursor(win)[1] == back, "cursor on the new topline (saved one is out of view)")
check(vim.wo[win].number == true, "source window options restored after a second toggle")
check(vim.api.nvim_buf_is_loaded(src), "source reloaded after toggling out")

---------------------------------------------------------------------------
-- Edit while hidden, toggle in: new layout.
---------------------------------------------------------------------------
doc_before = P.layout.doc
vim.api.nvim_buf_set_lines(src, 0, 0, false, { "# New title", "" })
vim.cmd("silent write")
vim.cmd("DmbdipToggle")
wait_idle(P, "layout after edit")
check(P.layout.doc > doc_before, "edit while hidden re-lays out on show")
vim.cmd("DmbdipToggle")

-- Strips arriving after the preview was hidden are dropped, not transmitted.
vim.cmd("DmbdipToggle")
wait_idle(P, "toggle in before hiding")
vim.cmd("normal! G")
dmbdip._pump(src)
check(P.in_flight and P.in_flight.type == "strips", "strips request in flight before hiding")
vim.cmd("DmbdipToggle")
requests = {}
check(vim.wait(30000, function()
  return not P.in_flight
end, 10), "strips response after hiding")
vim.wait(50)
check(#by("t") == 0, "no transmit after the preview was hidden")
check(count(P.images) == 0, "no images kept after the preview was hidden")

-- Toggle in at a new position: the first strips requested are the ones
-- around the mapped position, not around the preview's previous view.
vim.fn.winrestview({ topline = 1300, lnum = 1300 })
local sent_reqs = {}
local orig_chansend = vim.fn.chansend
vim.fn.chansend = function(job, data)
  sent_reqs[#sent_reqs + 1] = vim.json.decode(data)
  return orig_chansend(job, data)
end
vim.cmd("DmbdipToggle")
wait_idle(P, "toggle in at line 1300")
vim.fn.chansend = orig_chansend
local first_strips
for _, r in ipairs(sent_reqs) do
  if r.type == "strips" then
    first_strips = r
    break
  end
end
check(first_strips ~= nil, "strips requested after toggling in")
local top_index = math.floor((topline(win) - 1) / P.layout.strip_rows)
check(
  vim.tbl_contains(first_strips.indices, top_index),
  ("first strips request %s lacks the strip at the view (%d)"):format(vim.inspect(first_strips.indices), top_index)
)
vim.cmd("DmbdipToggle")

---------------------------------------------------------------------------
-- Split mode.
---------------------------------------------------------------------------
local nwins = #vim.api.nvim_list_wins()
vim.cmd("DmbdipPreview")
check(#vim.api.nvim_list_wins() == nwins + 1, "split opened")
check(vim.api.nvim_get_current_win() == win, "focus stays in the source window")
check(vim.wo[win].number == true, "source window options untouched by the split")
local split = P.split_win
check(split and vim.api.nvim_win_get_buf(split) == P.buf, "split shows the preview")
wait_idle(P, "split layout")
check(P.layout.strip_rows == vim.api.nvim_win_get_height(split), "geometry from the split")
check(count(P.loaded) > 0, "split strips loaded")
-- From inside the split, :DmbdipToggle-only notice is not shown (it is the split).
-- Edit: re-layout and visible strips re-requested.
doc_before = P.layout.doc
requests = {}
local n = vim.api.nvim_buf_line_count(src)
vim.api.nvim_buf_set_lines(src, n, n, false, { "", "A new closing paragraph." })
vim.api.nvim_exec_autocmds("TextChanged", { buffer = src })
check(vim.wait(30000, function()
  return P.layout.doc > doc_before and not P.in_flight and not P.debouncing
end, 10), "no re-layout after edit")
check(#by("t") > 0, "visible strips re-sent after edit")
vim.cmd("silent write")
-- Edit while a strips request is in flight: the edit wins, nothing stale shown.
vim.api.nvim_win_call(split, function()
  vim.cmd("normal! G")
end)
dmbdip._pump(src)
check(P.in_flight and P.in_flight.type == "strips", "strips request in flight")
doc_before = P.layout.doc
vim.api.nvim_buf_set_lines(src, 0, 0, false, { "Inserted line.", "" })
vim.api.nvim_exec_autocmds("TextChanged", { buffer = src })
check(vim.wait(30000, function()
  return P.layout.doc > doc_before and not P.in_flight and not P.debouncing
end, 10), "no re-layout after edit during a strips request")
check(count(P.loaded) > 0, "strips of the new layout loaded")
vim.cmd("silent write")
-- Scroll sync from the source.
vim.fn.winrestview({ topline = 400, lnum = 400 })
vim.api.nvim_exec_autocmds("WinScrolled", {})
dmbdip._sync_split(src)
check(topline(split) == math.floor(dmbdip._line_to_row(src, 400)) + 1, "split follows the source")
-- Close the split: server kept.
vim.cmd("DmbdipPreview")
check(not vim.api.nvim_win_is_valid(split), "split closed")
check(dmbdip._previews[src] == P, "state kept after closing the split")
check(vim.fn.jobwait({ P.job }, 0)[1] == -1, "server kept after closing the split")
vim.wait(100)
check(count(P.images) == 0, "images deleted after closing the split")

-- :DmbdipPreview from an in-place preview: notice only.
vim.cmd("DmbdipToggle")
wait_idle(P, "toggle in for notice")
notes = {}
nwins = #vim.api.nvim_list_wins()
vim.cmd("DmbdipPreview")
check(#vim.api.nvim_list_wins() == nwins, "no split from an in-place preview")
check(#notes == 1 and notes[1].msg:find("DmbdipToggle"), "notice: " .. vim.inspect(notes))
vim.cmd("DmbdipToggle")

---------------------------------------------------------------------------
-- :bdelete! of the source: job exits, preview buffer wiped.
---------------------------------------------------------------------------
local job, pbuf = P.job, P.buf
vim.cmd("bdelete! " .. src)
check(vim.wait(5000, function()
  return dmbdip._previews[src] == nil and not vim.api.nvim_buf_is_valid(pbuf)
end, 10), "bdelete should tear the preview down")
check(vim.fn.jobwait({ job }, 5000)[1] ~= -1, "server did not exit after bdelete")
local leftovers = vim.fn.glob("/dev/shm/dmbdip-" .. pid .. "-tty-graphics-protocol-*", false, true)
check(#leftovers == 0, "leftover strip files: " .. vim.inspect(leftovers))

---------------------------------------------------------------------------
-- Folding, heading navigation, arrows and bar (small nested document).
---------------------------------------------------------------------------
local function paras(name, n)
  local t = {}
  for i = 1, n do
    vim.list_extend(t, { name .. " paragraph " .. i .. " with a few words.", "" })
  end
  return t
end
local nested = {}
vim.list_extend(nested, paras("Intro", 30))
for _, sec in ipairs({ { "# A", 4 }, { "## A1", 4 }, { "### A1a", 4 }, { "## A2", 4 }, { "# B", 40 } }) do
  vim.list_extend(nested, { sec[1], "" })
  vim.list_extend(nested, paras(sec[1]:gsub("#+ ", ""), sec[2]))
end
local nested_path = vim.fn.tempname() .. ".md"
vim.fn.writefile(nested, nested_path)
vim.cmd("silent edit " .. nested_path)
src = vim.api.nvim_get_current_buf()
win = vim.api.nvim_get_current_win()
vim.cmd("DmbdipToggle")
P = dmbdip._previews[src]
wait_idle(P, "nested layout")

local function heads()
  local t = {}
  for _, h in ipairs(P.layout.headings) do
    t[h.title] = h
  end
  return t
end
local hd = heads()
local A, A1, A1a, A2, B = hd.A, hd.A1, hd.A1a, hd.A2, hd.B
check(A and A1 and A1a and A2 and B and #P.layout.headings == 5, "nested headings: " .. vim.inspect(P.layout.headings))
local function fc(row)
  return vim.fn.foldclosed(row + 1)
end
local function closed(h)
  return fc(h.end_row) == h.end_row + 1
end
local function goto_row(row)
  vim.api.nvim_win_set_cursor(win, { row + 1, 0 })
end
local function cursor_row()
  return vim.api.nvim_win_get_cursor(win)[1] - 1
end

-- Fold ranges and all folds open after toggling in.
check(A.fold_end == B.row - 1 and A1.fold_end == A2.row - 1 and A1a.fold_end == A2.row - 1, "fold ranges")
check(A2.fold_end == B.row - 1 and B.fold_end == P.layout.total_rows - 1, "fold ranges (A2, B)")
for _, h in ipairs(P.layout.headings) do
  check(h.foldable, h.title .. " foldable")
  check(vim.fn.foldlevel(h.end_row + 1) > 0, h.title .. " has a fold")
  check(fc(h.end_row) == -1, h.title .. " starts open")
end
check(vim.fn.foldlevel(A1a.end_row + 1) == 3, "A1a fold nested in A1 in A")

-- toggle_fold from inside A1 (before A1a) closes A1's range only.
goto_row(A1a.row - 1)
dmbdip.toggle_fold()
check(closed(A1), "A1 closed")
check(fc(A1.row) == -1 or A1.row == A1.end_row, "A1 heading rows stay visible")
check(fc(A1a.row) == A1.end_row + 1, "A1a hidden in A1")
check(fc(A2.row) == -1, "A2 not folded")
check(cursor_row() == A1.row, "cursor on A1's row")

-- Decorations: arrow on each heading's text row, bar on the current one.
local marks_ns = vim.api.nvim_get_namespaces()["dmbdip_marks"]
local function chunks_at(row)
  local m = vim.api.nvim_buf_get_extmarks(P.buf, marks_ns, { row, 0 }, { row, -1 }, { details = true })
  check(#m <= 1, "at most one decoration per row")
  if m[1] then
    check(m[1][3] == P.layout.offset_cols, "decoration in the reserved columns")
    check(m[1][4].virt_text_pos == "overlay", "overlay decoration")
    return m[1][4].virt_text
  end
end
local c = chunks_at(A1.text_row)
check(c and c[1][1] == "▎" and c[1][2] == "DmbdipCursor", "bar on the current heading: " .. vim.inspect(c))
check(c[2][1] == "▶" and c[2][2] == "DmbdipHeading2", "closed arrow in the heading colour: " .. vim.inspect(c))
c = chunks_at(A.text_row)
check(c and c[1][1] == " " and c[2][1] == "▼" and c[2][2] == "DmbdipHeading1", "open arrow, no bar: " .. vim.inspect(c))
for row = A1.text_row + 1, A1.end_row do
  c = chunks_at(row)
  check(c and c[1][1] == "▎", "bar on the current heading's row " .. row)
end
check(chunks_at(A.end_row + 1) == nil, "plain content row has no decoration")
check(A.text_row == A.end_row or chunks_at(A.end_row) == nil, "rows of a non-current heading have no bar")
check(vim.api.nvim_get_hl(0, { name = "DmbdipCursor" }).fg == tonumber(P.layout.cursor:sub(2), 16), "DmbdipCursor colour")
check(vim.api.nvim_get_hl(0, { name = "DmbdipHeading1" }).fg == tonumber(A.color:sub(2), 16), "DmbdipHeading1 colour")
check(vim.wo[win].foldtext == "" and vim.wo[win].foldmethod == "manual", "fold window options")
check(vim.wo[win].fillchars:find("fold: ", 1, true), "fold filler is a space")
check(vim.wo[win].winhighlight:find("Folded:DmbdipFolded", 1, true), "Folded cleared")

-- Nested: close A too; A1 and A2 hidden, B visible.
goto_row(A.row)
dmbdip.toggle_fold()
check(closed(A), "A closed")
check(fc(A1.row) == A.end_row + 1 and fc(A2.row) == A.end_row + 1, "A1, A2 hidden in A")
check(fc(B.row) == -1, "B visible")
c = chunks_at(A.text_row)
check(c[1][1] == "▎" and c[2][1] == "▶", "A current and closed: " .. vim.inspect(c))

-- Strips cover the view after folding near the top.
dmbdip._pump(src)
wait_idle(P, "strips after folding")
local vt, vb = view(win)
for index in pairs(strips_between(P, vt, vb)) do
  check(P.loaded[index], "visible strip " .. index .. " loaded after folding")
end

-- next_heading from A skips A1/A2 and lands on B, a quarter down the window.
local quarter = math.floor(vim.api.nvim_win_get_height(win) / 4)
dmbdip.next_heading()
check(cursor_row() == B.row, "next heading from A is B, got row " .. cursor_row())
check(vim.fn.winline() == quarter + 1, ("B at screen line %d, want %d"):format(vim.fn.winline(), quarter + 1))
dmbdip.next_heading()
check(cursor_row() == B.row, "no heading after B: no move")
dmbdip.prev_heading()
check(cursor_row() == A.row, "prev heading from B is A")
check(vim.fn.winline() == quarter + 1, "A a quarter down the window")
dmbdip.prev_heading()
check(cursor_row() == A.row, "no heading before A: no move")
-- From before the first heading, next goes to the first.
goto_row(0)
dmbdip.next_heading()
check(cursor_row() == A.row, "next heading from the intro is A")
check(topline(win) == 1 or vim.fn.winline() == quarter + 1, "A placed or clamped at the top")

-- Open A; A1 stays closed. Close A2 with plain zc.
goto_row(A.row)
dmbdip.toggle_fold()
check(not closed(A) and closed(A1), "A open, A1 still closed")
goto_row(A2.end_row + 1)
vim.cmd("normal! zc")
check(closed(A2), "A2 closed with zc")
-- (a real zc moves the cursor to the fold's first line: CursorMoved)
vim.api.nvim_exec_autocmds("CursorMoved", { buffer = P.buf })
check(chunks_at(A2.text_row)[2][1] == "▶", "arrow follows a zc fold")

-- Toggle out and back in keeps the closed folds.
vim.cmd("DmbdipToggle")
check(vim.api.nvim_win_get_buf(win) == src, "toggled out")
check(vim.wo[win].foldlevel == 0 and vim.wo[win].fillchars == "", "source window fold options restored")
vim.cmd("DmbdipToggle")
wait_idle(P, "nested toggle in again")
check(closed(A1) and closed(A2) and not closed(A) and not closed(B), "folds kept across toggle out/in")

-- Edit while hidden (lines shift): folds follow their headings.
vim.cmd("DmbdipToggle")
vim.api.nvim_buf_set_lines(src, 0, 0, false, { "Inserted paragraph.", "", "Another one.", "" })
vim.cmd("silent write")
local doc0 = P.layout.doc
vim.cmd("DmbdipToggle")
wait_idle(P, "nested relayout after edit while hidden")
check(P.layout.doc > doc0, "new layout after the edit")
hd = heads()
check(hd.A2.row > A2.row, "A2 moved down")
A, A1, A1a, A2, B = hd.A, hd.A1, hd.A1a, hd.A2, hd.B
check(closed(A1) and closed(A2) and not closed(A) and not closed(B), "folds kept after an edit while hidden")
vim.cmd("DmbdipToggle")

-- Split: a new window starts with all folds open; a zc fold survives a live
-- re-layout that shifts lines.
vim.cmd("DmbdipPreview")
split = P.split_win
wait_idle(P, "nested split")
vim.api.nvim_win_call(split, function()
  check(not closed(A1) and not closed(A2), "a new split starts unfolded")
  vim.api.nvim_win_set_cursor(split, { A2.end_row + 2, 0 })
  vim.cmd("normal! zc")
  check(closed(A2), "A2 closed with zc in the split")
end)
doc0 = P.layout.doc
vim.api.nvim_buf_set_lines(src, 0, 0, false, { "More text at the top.", "", "And more.", "" })
vim.api.nvim_exec_autocmds("TextChanged", { buffer = src })
check(vim.wait(30000, function()
  return P.layout.doc > doc0 and not P.in_flight and not P.debouncing
end, 10), "no re-layout after edit in split")
hd = heads()
check(hd.A2.row > A2.row, "A2 moved down after the live edit")
vim.api.nvim_win_call(split, function()
  check(fc(hd.A2.end_row) == hd.A2.end_row + 1, "A2 still closed at its new rows")
  check(not closed(hd.A1) and not closed(hd.A), "other folds still open")
end)
vim.cmd("DmbdipPreview")
vim.cmd("silent write")

-- Functions are no-ops outside a preview window.
dmbdip.next_heading()
dmbdip.toggle_fold()
check(vim.api.nvim_win_get_buf(win) == src, "no-op in a source window")
vim.cmd("bdelete! " .. src)
vim.fn.delete(nested_path)

---------------------------------------------------------------------------
-- Missing binary; protocol mismatch.
---------------------------------------------------------------------------
vim.cmd("silent edit " .. doc_path)
src = vim.api.nvim_get_current_buf()
win = vim.api.nvim_get_current_win()
notes = {}
dmbdip.setup({ cmd = root .. "/target/debug/no-such-dmbdip" })
vim.cmd("DmbdipToggle")
check(dmbdip._previews[src] == nil, "no preview state without a server binary")
check(vim.api.nvim_win_get_buf(win) == src, "window unchanged without a server binary")
check(#notes == 1 and notes[1].msg:find("not found"), "not-found error: " .. vim.inspect(notes))

local fake = vim.fn.tempname() .. "-dmbdip"
vim.fn.writefile({
  "#!/bin/sh",
  "while read line; do echo '{\"id\":1,\"type\":\"layout\",\"protocol\":1}'; done",
}, fake)
vim.fn.setfperm(fake, "rwx------")
dmbdip.setup({ cmd = fake })
notes = {}
vim.cmd("DmbdipToggle")
check(vim.wait(5000, function()
  return dmbdip._previews[src] == nil
end, 10), "protocol mismatch should tear down")
check(vim.api.nvim_win_get_buf(win) == src, "window back on the source after teardown")
local found = false
for _, note in ipairs(notes) do
  if note.msg:find("protocol mismatch %(got 1, want 3%)") and note.level == vim.log.levels.ERROR then
    found = true
  end
end
check(found, "protocol mismatch notice: " .. vim.inspect(notes))

vim.fn.delete(fake)
vim.fn.delete(doc_path)
print("ok")
os.exit(0)
