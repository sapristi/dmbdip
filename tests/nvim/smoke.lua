-- Headless smoke test for the neovim preview plugin.
-- Run from the repo root after `cargo build`:
--   nvim --headless -u NONE --cmd 'set rtp^=.' -l tests/nvim/smoke.lua

local root = vim.fn.getcwd()
local PLACEHOLDER = vim.fn.nr2char(0x10EEEE)
local CELL_W, CELL_H = 10, 20

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

vim.o.columns = 200
vim.o.lines = 50
vim.o.termguicolors = true
vim.cmd("filetype on")
vim.cmd("runtime plugin/dmbdip.lua")

local dmbdip = require("dmbdip")
dmbdip.setup({ cmd = root .. "/target/debug/dmbdip", debounce_ms = 10 })

vim.cmd("edit " .. root .. "/docs/sample.md")
local src = vim.api.nvim_get_current_buf()
local src_win = vim.api.nvim_get_current_win()
check(vim.bo[src].filetype == "markdown", "sample.md should be markdown")

vim.cmd("DmbdipPreview")
local P = dmbdip._previews[src]
check(P ~= nil, "preview not opened")
check(vim.api.nvim_get_current_win() == src_win, "focus should stay in the source window")
local pid = vim.fn.jobpid(P.job)

local function transmits()
  return vim.tbl_filter(function(r)
    return r.a == "t"
  end, requests)
end

check(vim.wait(30000, function()
  return P.nstrips > 0
end, 20), "no render response")

-- Buffer content.
local lines = vim.api.nvim_buf_get_lines(P.buf, 0, -1, false)
check(#lines == P.total_rows, ("line count %d ~= total_rows %d"):format(#lines, P.total_rows))
for i, l in ipairs(lines) do
  check(l:sub(1, #PLACEHOLDER) == PLACEHOLDER, "line " .. i .. " does not start with a placeholder")
end
check(P.layout.cols > 0, "cols")
check(vim.fn.strchars(lines[1]) == P.layout.cols + 2, "first line: cols placeholders + 2 diacritics")

-- Graphics requests.
local first = transmits()
check(#first == P.nstrips, ("%d transmits for %d strips"):format(#first, P.nstrips))
for i, r in ipairs(first) do
  check(r.t == "t" and r.f == 24, "transmit uses t=t, f=24")
  check(r.s == P.layout.cols * CELL_W, "transmit width")
  check(r.v == P.layout.rows[i] * CELL_H, "transmit height")
  check(r.i == P.base_id + i - 1, "transmit id")
  check(r.i >= 0x100000 and r.i < 0x1000000, "id range")
  check(vim.base64.decode(r.data):find("tty%-graphics%-protocol"), "file path")
end
for _, r in ipairs(requests) do
  check(r.q == nil or r.q == 2, "request must not override q")
end
local hl = vim.api.nvim_get_hl(0, { name = "DmbdipImg" .. P.base_id })
check(hl.fg == P.base_id, "highlight fg carries the image id")

-- Scroll sync: moving the source down moves the preview down.
vim.api.nvim_win_call(src_win, function()
  vim.fn.winrestview({ topline = 60, lnum = 60 })
end)
dmbdip._sync_scroll(src)
local preview_top = vim.fn.getwininfo(P.win)[1].topline
check(preview_top > 1, "preview should scroll with the source (topline " .. preview_top .. ")")

-- Edit: at least one strip is re-sent.
local before = #transmits()
local n = vim.api.nvim_buf_line_count(src)
vim.api.nvim_buf_set_lines(src, n, n, false, { "", "A new closing paragraph." })
vim.api.nvim_exec_autocmds("TextChanged", { buffer = src })
check(vim.wait(30000, function()
  return #transmits() > before
end, 20), "no re-transmit after edit")
check(#transmits() - before < P.nstrips or P.nstrips == 1, "edit at end should not resend every strip")

-- Close: every transmitted id deleted, job gone, no leftover files.
local ids = {}
for _, r in ipairs(transmits()) do
  ids[r.i] = true
end
vim.cmd("DmbdipPreview")
check(dmbdip._previews[src] == nil, "preview not closed")
check(not vim.api.nvim_win_is_valid(P.win), "preview window not closed")
for _, r in ipairs(requests) do
  if r.a == "d" then
    check(r.d == "I", "delete uses d=I")
    ids[r.i] = nil
  end
end
check(next(ids) == nil, "images not deleted: " .. vim.inspect(vim.tbl_keys(ids)))
check(vim.fn.jobwait({ P.job }, 5000)[1] ~= -1, "server did not exit")
local leftovers = vim.fn.glob("/dev/shm/dmbdip-" .. pid .. "-tty-graphics-protocol-*", false, true)
check(#leftovers == 0, "leftover strip files: " .. vim.inspect(leftovers))

print("ok")
os.exit(0)
