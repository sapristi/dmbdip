-- Live markdown preview rendered by `dmbdip --serve`, shown through kitty
-- unicode placeholders. See README.md ("Neovim preview") and
-- .project/docs/specs/2026-11-22-14-13-nvim-preview-design.md.

local diacritics = require("dmbdip.diacritics")

local M = {}

local config = {
  cmd = "dmbdip",
  debounce_ms = 150,
}

local PLACEHOLDER = vim.fn.nr2char(0x10EEEE)
local ID_MIN, ID_MAX = 0x100000, 0xFF0000 -- base ids are picked in [ID_MIN, ID_MAX)

---@type table<integer, table> preview state keyed by source buffer
local previews = {}
M._previews = previews -- exposed for tests

local ns = vim.api.nvim_create_namespace("dmbdip")
local seeded = false

function M.setup(opts)
  config = vim.tbl_extend("force", config, opts or {})
end

local function notify(msg, level)
  vim.notify("dmbdip: " .. msg, level or vim.log.levels.INFO)
end

local function terminal()
  if type(_G.Snacks) ~= "table" then
    pcall(require, "snacks")
  end
  local ok, term = pcall(function()
    return _G.Snacks.image.terminal
  end)
  return ok and term or nil
end

local function valid_win(win)
  return win ~= nil and vim.api.nvim_win_is_valid(win)
end

local function hl_name(id)
  return "DmbdipImg" .. id
end

--- Window that drives scrolling: the most recently entered window showing the
--- source buffer, else any window showing it, else nil (sync paused).
local function source_win(P)
  if valid_win(P.src_win) and vim.api.nvim_win_get_buf(P.src_win) == P.src then
    return P.src_win
  end
  for _, w in ipairs(vim.fn.win_findbuf(P.src)) do
    if w ~= P.win then
      P.src_win = w
      return w
    end
  end
  return nil
end

--- `{ cols, cell_w, cell_h }` for the preview window right now.
local function geometry(P)
  local size = P.term.size()
  local info = vim.fn.getwininfo(P.win)[1]
  local cols = vim.api.nvim_win_get_width(P.win) - (info and info.textoff or 0)
  return {
    cols = cols,
    cell_w = math.floor(size.cell_width + 0.5),
    cell_h = math.floor(size.cell_height + 0.5),
  }
end

local function same_geometry(a, b)
  return a and b and a.cols == b.cols and a.cell_w == b.cell_w and a.cell_h == b.cell_h
end

local function send(P)
  if P.closed then
    return
  end
  if P.in_flight then
    P.pending = true
    return
  end
  if not valid_win(P.win) then
    return
  end
  local geom = geometry(P)
  if geom.cols < 1 then
    return
  end
  P.pending = false
  P.in_flight = geom
  P.last_sent = geom
  P.next_id = P.next_id + 1
  local text = table.concat(vim.api.nvim_buf_get_lines(P.src, 0, -1, false), "\n")
  local req = vim.json.encode({
    id = P.next_id,
    type = "render",
    text = text,
    cols = geom.cols,
    cell_w = geom.cell_w,
    cell_h = geom.cell_h,
  })
  vim.fn.chansend(P.job, req .. "\n")
end

local function schedule_render(P)
  P.timer:stop()
  P.timer:start(config.debounce_ms, 0, vim.schedule_wrap(function()
    send(P)
  end))
end

--- Re-render when the preview's width or the cell size changed.
local function check_resize(P)
  if valid_win(P.win) and not same_geometry(geometry(P), P.last_sent) then
    send(P)
  end
end

local function sync_scroll(P)
  local w = source_win(P)
  if not w or not valid_win(P.win) or #P.anchors == 0 then
    return
  end
  local topline = vim.fn.getwininfo(w)[1].topline
  local anchors = P.anchors
  local row = 0
  local i = nil
  for k, a in ipairs(anchors) do
    if a[1] <= topline then
      i = k
    else
      break
    end
  end
  if i then
    local l0, r0 = anchors[i][1], anchors[i][2]
    -- Past the last block, interpolate towards the end of both documents.
    local nxt = anchors[i + 1] or { vim.api.nvim_buf_line_count(P.src) + 1, P.total_rows }
    local l1, r1 = nxt[1], nxt[2]
    row = r0
    if l1 > l0 then
      row = r0 + (topline - l0) / (l1 - l0) * (r1 - r0)
    end
  end
  local line_count = vim.api.nvim_buf_line_count(P.buf)
  local target = math.max(1, math.min(line_count, math.floor(row) + 1))
  vim.api.nvim_win_call(P.win, function()
    vim.fn.winrestview({ topline = target, lnum = target, col = 0 })
  end)
end

local function delete_image(P, id)
  P.term.request({ a = "d", d = "I", i = id })
  pcall(vim.api.nvim_set_hl, 0, hl_name(id), {})
end

local function write_lines(P, resp, geom)
  local lines = {}
  local marks = {}
  local tail = string.rep(PLACEHOLDER, resp.cols - 1)
  local col0 = diacritics[1]
  for _, strip in ipairs(resp.strips) do
    local start = #lines
    for r = 1, strip.rows do
      lines[#lines + 1] = PLACEHOLDER .. diacritics[r] .. col0 .. tail
    end
    marks[#marks + 1] = { start, #lines, P.base_id + strip.index }
  end
  vim.bo[P.buf].modifiable = true
  vim.api.nvim_buf_set_lines(P.buf, 0, -1, false, lines)
  vim.bo[P.buf].modifiable = false
  vim.api.nvim_buf_clear_namespace(P.buf, ns, 0, -1)
  for _, m in ipairs(marks) do
    vim.api.nvim_buf_set_extmark(P.buf, ns, m[1], 0, {
      end_row = m[2],
      end_col = 0,
      hl_group = hl_name(m[3]),
      hl_eol = false,
    })
  end
  P.layout = { cols = resp.cols, total_rows = resp.total_rows, rows = vim.tbl_map(function(s)
    return s.rows
  end, resp.strips), cell_w = geom.cell_w, cell_h = geom.cell_h }
end

local function layout_changed(P, resp)
  local l = P.layout
  if not l or l.cols ~= resp.cols or l.total_rows ~= resp.total_rows or #l.rows ~= #resp.strips then
    return true
  end
  for i, s in ipairs(resp.strips) do
    if l.rows[i] ~= s.rows then
      return true
    end
  end
  return false
end

local function apply(P, resp, geom)
  if not vim.api.nvim_buf_is_valid(P.buf) then
    return
  end
  for _, strip in ipairs(resp.strips) do
    local id = P.base_id + strip.index
    vim.api.nvim_set_hl(0, hl_name(id), { fg = string.format("#%06x", id) })
    if strip.path ~= nil and strip.path ~= vim.NIL then
      P.term.request({
        a = "t",
        t = "t",
        f = 24,
        s = resp.cols * geom.cell_w,
        v = strip.rows * geom.cell_h,
        i = id,
        data = vim.base64.encode(strip.path),
      })
      P.term.request({ a = "p", U = 1, i = id, p = 1, c = resp.cols, r = strip.rows })
    end
  end
  for index = #resp.strips, P.nstrips - 1 do
    delete_image(P, P.base_id + index)
  end
  P.nstrips = #resp.strips
  if layout_changed(P, resp) then
    write_lines(P, resp, geom)
  end
  P.total_rows = resp.total_rows
  P.anchors = resp.anchors
  sync_scroll(P)
end

local function on_response(P, line)
  local ok, resp = pcall(vim.json.decode, line)
  local geom = P.in_flight
  P.in_flight = false
  if not ok or type(resp) ~= "table" then
    notify("bad response from server: " .. line, vim.log.levels.WARN)
  elseif resp.type == "error" then
    -- Repeated errors (e.g. a too narrow window on every edit) are shown once
    -- until a render succeeds again.
    local msg = tostring(resp.message)
    if msg ~= P.last_error then
      P.last_error = msg
      notify(msg, vim.log.levels.WARN)
    end
  elseif resp.type == "render" and geom then
    P.last_error = nil
    apply(P, resp, geom)
  end
  if P.pending then
    send(P)
  end
end

local function close(P)
  if P.closed then
    return
  end
  P.closed = true
  previews[P.src] = nil
  if not P.timer:is_closing() then
    P.timer:stop()
    P.timer:close()
  end
  pcall(vim.api.nvim_del_augroup_by_id, P.augroup)
  for index = 0, P.nstrips - 1 do
    delete_image(P, P.base_id + index)
  end
  -- Closing stdin makes the server exit and remove its leftover strip files.
  pcall(vim.fn.chanclose, P.job, "stdin")
  if valid_win(P.win) and #vim.api.nvim_list_wins() > 1 then
    pcall(vim.api.nvim_win_close, P.win, true)
  end
  if vim.api.nvim_buf_is_valid(P.buf) then
    pcall(vim.api.nvim_buf_delete, P.buf, { force = true })
  end
end

--- Line-buffered handler for job output chunks.
local function line_reader(on_line)
  local partial = ""
  return function(_, data)
    for i, chunk in ipairs(data) do
      if i == 1 then
        chunk = partial .. chunk
      end
      if i == #data then
        partial = chunk
      elseif chunk ~= "" then
        on_line(chunk)
      end
    end
  end
end

local function open(src)
  local term = terminal()
  if not term then
    notify("snacks.nvim is required (Snacks.image.terminal not found)", vim.log.levels.ERROR)
    return
  end
  if not vim.o.termguicolors then
    notify("'termguicolors' must be set", vim.log.levels.ERROR)
    return
  end
  if not vim.bo[src].filetype:find("markdown") then
    notify("not a markdown buffer", vim.log.levels.WARN)
    return
  end
  if vim.fn.executable(config.cmd) ~= 1 then
    notify("dmbdip not found: " .. tostring(config.cmd), vim.log.levels.ERROR)
    return
  end
  if term.env then
    pcall(term.env) -- detects the terminal; sets up tmux passthrough
  end
  if not seeded then
    math.randomseed(vim.uv.hrtime())
    seeded = true
  end

  local src_win = vim.api.nvim_get_current_win()
  local buf = vim.api.nvim_create_buf(false, true)
  vim.bo[buf].buftype = "nofile"
  vim.bo[buf].bufhidden = "wipe"
  vim.bo[buf].swapfile = false
  vim.bo[buf].modifiable = false
  vim.bo[buf].filetype = "dmbdip"
  local win = vim.api.nvim_open_win(buf, false, { split = "right", win = src_win })
  local wo = {
    number = false,
    relativenumber = false,
    signcolumn = "no",
    foldcolumn = "0",
    wrap = false,
    list = false,
    cursorline = false,
    cursorcolumn = false,
    colorcolumn = "",
    spell = false,
    statuscolumn = "",
    scrolloff = 0,
    sidescrolloff = 0,
  }
  for k, v in pairs(wo) do
    vim.api.nvim_set_option_value(k, v, { scope = "local", win = win })
  end

  local P = {
    src = src,
    src_win = src_win,
    buf = buf,
    win = win,
    term = term,
    base_id = math.random(ID_MIN, ID_MAX - 1),
    timer = assert(vim.uv.new_timer()),
    augroup = vim.api.nvim_create_augroup("dmbdip_preview_" .. src, { clear = true }),
    next_id = 0,
    in_flight = false,
    pending = false,
    nstrips = 0,
    anchors = {},
    total_rows = 0,
  }
  previews[src] = P

  local started, job = pcall(vim.fn.jobstart, { config.cmd, "--serve" }, {
    stdin = "pipe",
    on_stdout = line_reader(function(line)
      on_response(P, line)
    end),
    on_stderr = line_reader(function(line)
      notify(line, vim.log.levels.WARN)
    end),
    on_exit = function(_, code)
      if not P.closed then
        notify(("server exited (code %d)"):format(code), vim.log.levels.ERROR)
        close(P)
      end
    end,
  })
  P.job = started and job or 0
  if P.job <= 0 then
    notify("could not start " .. tostring(config.cmd) .. (started and "" or (": " .. tostring(job))), vim.log.levels.ERROR)
    P.job = nil
    close(P)
    return
  end

  local function au(event, opts)
    opts.group = P.augroup
    vim.api.nvim_create_autocmd(event, opts)
  end
  au({ "TextChanged", "TextChangedI" }, {
    buffer = src,
    callback = function()
      schedule_render(P)
    end,
  })
  au("WinScrolled", {
    callback = function()
      local w = source_win(P)
      if w and vim.v.event[tostring(w)] then
        sync_scroll(P)
      end
    end,
  })
  au({ "WinEnter", "BufWinEnter" }, {
    callback = function()
      local w = vim.api.nvim_get_current_win()
      if w ~= P.win and vim.api.nvim_win_get_buf(w) == src then
        P.src_win = w
        sync_scroll(P)
      end
    end,
  })
  au("WinResized", {
    callback = function()
      if vim.tbl_contains(vim.v.event.windows or {}, P.win) then
        check_resize(P)
      end
    end,
  })
  au("VimResized", {
    callback = function()
      check_resize(P)
    end,
  })
  au({ "BufWipeout", "BufDelete", "BufUnload" }, {
    buffer = src,
    callback = function()
      vim.schedule(function()
        close(P)
      end)
    end,
  })
  au("BufWipeout", {
    buffer = buf,
    callback = function()
      vim.schedule(function()
        close(P)
      end)
    end,
  })
  au("WinClosed", {
    pattern = tostring(win),
    callback = function()
      vim.schedule(function()
        close(P)
      end)
    end,
  })

  send(P)
end

--- Toggle the preview for a buffer (default: current). Called from the
--- preview window itself, closes that preview.
function M.toggle(bufnr)
  bufnr = bufnr or vim.api.nvim_get_current_buf()
  for _, P in pairs(previews) do
    if P.buf == bufnr then
      return close(P)
    end
  end
  if previews[bufnr] then
    return close(previews[bufnr])
  end
  open(bufnr)
end

function M.close_all()
  for _, P in pairs(previews) do
    close(P)
  end
end

M._sync_scroll = function(bufnr)
  local P = previews[bufnr]
  if P then
    sync_scroll(P)
  end
end

return M
