-- Markdown preview rendered by `dmbdip --serve` (protocol 2), shown through
-- kitty unicode placeholders. See README.md ("Neovim preview") and
-- .project/docs/specs/2026-11-22-15-30-nvim-toggle-large-docs-design.md.
--
-- One preview per source buffer: a server job, a preview buffer of
-- placeholder lines, and the strips (images) loaded near the view. The
-- preview buffer is shown either in place of the source (`:DmbdipToggle`) or
-- in a split (`:DmbdipPreview`).

local diacritics = require("dmbdip.diacritics")

local M = {}

local config = {
  cmd = "dmbdip",
  debounce_ms = 150,
}

local PROTOCOL = 2
local PLACEHOLDER = vim.fn.nr2char(0x10EEEE)
local ID_MIN, ID_MAX = 0x100000, 0xFF0000 -- base ids are picked in [ID_MIN, ID_MAX)

-- Window options while the preview buffer is shown in a window.
local PREVIEW_WO = {
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
  winhighlight = "Normal:DmbdipBg,EndOfBuffer:DmbdipBg",
}

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

local function shows(win, buf)
  return valid_win(win) and vim.api.nvim_win_get_buf(win) == buf
end

local function hl_name(id)
  return "DmbdipImg" .. id
end

local function preview_of_buf(buf)
  for _, P in pairs(previews) do
    if P.buf == buf then
      return P
    end
  end
end

---------------------------------------------------------------------------
-- Windows and geometry
---------------------------------------------------------------------------

--- The preview window: the most recently entered window showing the preview
--- buffer, else any window showing it, else nil (preview hidden).
local function preview_win(P)
  if shows(P.win, P.buf) then
    return P.win
  end
  P.win = vim.fn.win_findbuf(P.buf)[1]
  return P.win
end

--- Window that drives split scroll sync: the most recently entered window
--- showing the source buffer.
local function source_win(P)
  if shows(P.src_win, P.src) then
    return P.src_win
  end
  P.src_win = vim.fn.win_findbuf(P.src)[1]
  return P.src_win
end

local function split_active(P)
  return shows(P.split_win, P.buf)
end

--- `{ cols, cell_w, cell_h, strip_rows }` for a window right now.
local function geometry(P, win)
  local size = P.term.size()
  local info = vim.fn.getwininfo(win)[1]
  return {
    cols = vim.api.nvim_win_get_width(win) - (info and info.textoff or 0),
    cell_w = math.floor(size.cell_width + 0.5),
    cell_h = math.floor(size.cell_height + 0.5),
    strip_rows = math.max(1, vim.api.nvim_win_get_height(win)),
  }
end

local function same_geometry(a, b)
  return a ~= nil
    and b ~= nil
    and a.cols == b.cols
    and a.cell_w == b.cell_w
    and a.cell_h == b.cell_h
    and a.strip_rows == b.strip_rows
end

---------------------------------------------------------------------------
-- Source line <-> preview row mapping (over the layout's anchors)
---------------------------------------------------------------------------

local function src_line_count(P)
  if vim.api.nvim_buf_is_loaded(P.src) then
    return vim.api.nvim_buf_line_count(P.src)
  end
  return P.layout and P.layout.line_count or 1
end

--- Preview row (0-based, may be fractional) shown for source line `line`.
local function line_to_row(P, line)
  local L = P.layout
  if not L or #L.anchors == 0 then
    return 0
  end
  local anchors = L.anchors
  local i
  for k, a in ipairs(anchors) do
    if a[1] <= line then
      i = k
    else
      break
    end
  end
  if not i then
    return 0
  end
  local l0, r0 = anchors[i][1], anchors[i][2]
  -- Past the last block, interpolate towards the end of both documents.
  local nxt = anchors[i + 1] or { src_line_count(P) + 1, L.total_rows }
  local l1, r1 = nxt[1], nxt[2]
  if l1 > l0 then
    return r0 + (line - l0) / (l1 - l0) * (r1 - r0)
  end
  return r0
end

--- Source line (1-based) shown at preview row `row` (0-based): the inverse
--- of `line_to_row`, linear in row between consecutive anchors.
local function row_to_line(P, row)
  local L = P.layout
  local count = src_line_count(P)
  if not L or #L.anchors == 0 then
    return 1
  end
  local anchors = L.anchors
  local i
  for k, a in ipairs(anchors) do
    if a[2] <= row then
      i = k
    else
      break
    end
  end
  local line
  if not i then
    line = 1
  else
    local l0, r0 = anchors[i][1], anchors[i][2]
    local nxt = anchors[i + 1] or { count + 1, L.total_rows }
    local l1, r1 = nxt[1], nxt[2]
    line = l0
    if r1 > r0 then
      line = l0 + (row - r0) / (r1 - r0) * (l1 - l0)
    end
  end
  return math.max(1, math.min(count, math.floor(line + 0.5)))
end

--- Put the preview's top line at the row mapped from source line `line`.
local function position_preview(P, win, line)
  if not shows(win, P.buf) or not P.layout then
    return
  end
  local nlines = vim.api.nvim_buf_line_count(P.buf)
  local target = math.max(1, math.min(nlines, math.floor(line_to_row(P, line)) + 1))
  vim.api.nvim_win_call(win, function()
    vim.fn.winrestview({ topline = target, lnum = target, col = 0 })
  end)
  return target
end

---------------------------------------------------------------------------
-- Images
---------------------------------------------------------------------------

local function delete_image(P, index)
  local id = P.base_id + index
  P.term.request({ a = "d", d = "I", i = id })
  pcall(vim.api.nvim_set_hl, 0, hl_name(id), {})
  P.images[index] = nil
  P.loaded[index] = nil
end

local function delete_all_images(P)
  for index in pairs(P.images) do
    delete_image(P, index)
  end
  P.loaded = {}
end

local function set_highlights(P)
  if P.layout then
    vim.api.nvim_set_hl(0, "DmbdipBg", { bg = P.layout.bg })
  end
  for index in pairs(P.images) do
    local id = P.base_id + index
    vim.api.nvim_set_hl(0, hl_name(id), { fg = string.format("#%06x", id) })
  end
end

--- Strip index range `[first, last]` intersecting rows `[a, b]`, or nil.
local function strips_in(L, a, b)
  a = math.max(0, a)
  b = math.min(L.total_rows - 1, b)
  if a > b then
    return nil
  end
  return math.floor(a / L.strip_rows), math.floor(b / L.strip_rows)
end

--- Visible rows `top`, `bot` (0-based) and height of the preview window.
local function view(win)
  local info = vim.fn.getwininfo(win)[1]
  return info.topline - 1, info.botline - 1, info.height
end

--- Delete images outside the keep range; return the wanted strips that are
--- not loaded yet.
local function plan_loading(P, win)
  local L = P.layout
  local top, bot, H = view(win)
  local keep_a, keep_b = top - 3 * H, bot + 3 * H
  for index in pairs(P.images) do
    local first = index * L.strip_rows
    local last = first + L.strip_rows - 1
    if index >= L.nstrips or last < keep_a or first > keep_b then
      delete_image(P, index)
    end
  end
  local missing = {}
  if P.strips_failed == L.doc then
    return missing
  end
  local s0, s1 = strips_in(L, top - H, bot + H)
  if s0 then
    for index = s0, s1 do
      if not P.loaded[index] then
        missing[#missing + 1] = index
      end
    end
  end
  return missing
end

---------------------------------------------------------------------------
-- Preview buffer lines
---------------------------------------------------------------------------

local function write_lines(P)
  local L = P.layout
  local key = table.concat({ L.total_rows, L.image_cols, L.offset_cols, L.strip_rows }, ":")
  if P.lines_key == key then
    return
  end
  P.lines_key = key
  local lead = string.rep(" ", L.offset_cols)
  local tail = string.rep(PLACEHOLDER, L.image_cols - 1)
  local col0 = diacritics[1]
  local lines = {}
  for row = 0, L.total_rows - 1 do
    lines[row + 1] = lead .. PLACEHOLDER .. diacritics[row % L.strip_rows + 1] .. col0 .. tail
  end
  vim.bo[P.buf].modifiable = true
  vim.api.nvim_buf_set_lines(P.buf, 0, -1, false, lines)
  vim.bo[P.buf].modifiable = false
  vim.api.nvim_buf_clear_namespace(P.buf, ns, 0, -1)
  -- Highlight the placeholder columns only (not the centring spaces): the
  -- foreground colour carries the image id of the row's strip.
  local start_col = #lead
  for row = 0, L.total_rows - 1 do
    vim.api.nvim_buf_set_extmark(P.buf, ns, row, start_col, {
      end_row = row,
      end_col = #lines[row + 1],
      hl_group = hl_name(P.base_id + math.floor(row / L.strip_rows)),
    })
  end
end

---------------------------------------------------------------------------
-- Requests (at most one in flight; layout requests first)
---------------------------------------------------------------------------

local pump -- forward declaration

local function source_text(P)
  if vim.api.nvim_buf_is_loaded(P.src) then
    return table.concat(vim.api.nvim_buf_get_lines(P.src, 0, -1, false), "\n"),
      vim.api.nvim_buf_get_changedtick(P.src),
      vim.api.nvim_buf_line_count(P.src)
  end
  -- The source was unloaded by toggling in ('nohidden'): use the text
  -- captured just before.
  if P.snapshot then
    return P.snapshot.text, P.snapshot.tick, P.snapshot.line_count
  end
end

--- Key identifying what a layout would be made from right now.
local function layout_key(P, geom)
  local tick
  if vim.api.nvim_buf_is_loaded(P.src) then
    tick = vim.api.nvim_buf_get_changedtick(P.src)
  else
    tick = P.snapshot and P.snapshot.tick
  end
  return { tick = tick, geom = geom }
end

local function needs_layout(P, geom)
  local sent = P.layout_sent
  if not sent or not same_geometry(sent.geom, geom) then
    return true
  end
  local key = layout_key(P, geom)
  -- In split mode, source edits re-layout through the debounce timer only.
  if key.tick == sent.tick or P.debouncing then
    return false
  end
  -- A changed tick with the same text (e.g. the source was reloaded after
  -- toggling unloaded it) needs no new layout.
  local text = source_text(P)
  if text == sent.text then
    sent.tick = key.tick
    return false
  end
  return true
end

--- Send a request; `meta` is kept locally until its response arrives.
local function send(P, req, meta)
  P.next_id = P.next_id + 1
  req.id = P.next_id
  P.in_flight = { type = req.type, meta = meta }
  vim.fn.chansend(P.job, vim.json.encode(req) .. "\n")
end

local function send_layout(P, win, geom)
  local text, tick, line_count = source_text(P)
  if not text then
    return false
  end
  -- Shown in place: keep the same source line at the top across the new
  -- layout (e.g. after a resize changed the wrapping).
  local keep_line
  if P.layout and win ~= P.split_win and not P.position_on_layout then
    keep_line = row_to_line(P, vim.fn.getwininfo(win)[1].topline - 1)
  end
  P.layout_sent = { tick = tick, geom = geom, text = text }
  send(P, {
    type = "layout",
    text = text,
    cols = geom.cols,
    cell_w = geom.cell_w,
    cell_h = geom.cell_h,
    strip_rows = geom.strip_rows,
  }, { line_count = line_count, geom = geom, keep_line = keep_line })
  return true
end

--- Send the next request if none is in flight: a layout when the source or
--- the preview window's geometry changed, else the missing strips near the
--- view. Also evicts strips far from the view.
pump = function(P)
  if P.closed or P.in_flight then
    return
  end
  local win = preview_win(P)
  if not win then
    return
  end
  local geom = geometry(P, win)
  if geom.cols < 1 then
    return
  end
  if needs_layout(P, geom) and send_layout(P, win, geom) then
    return
  end
  if not P.layout then
    return
  end
  local missing = plan_loading(P, win)
  if #missing > 0 then
    send(P, { type = "strips", doc = P.layout.doc, indices = missing })
  end
end

local function schedule_layout(P)
  P.debouncing = true
  P.timer:stop()
  P.timer:start(config.debounce_ms, 0, vim.schedule_wrap(function()
    P.debouncing = false
    pump(P)
  end))
end

local function sync_scroll(P)
  local w = source_win(P)
  if not w or not split_active(P) then
    return
  end
  position_preview(P, P.split_win, vim.fn.getwininfo(w)[1].topline)
end

local function apply_layout(P, resp, meta)
  resp.line_count = meta.line_count
  resp.geom = meta.geom
  P.layout = resp
  -- Images of the previous layout stay on screen until replaced, but none
  -- counts as loaded any more.
  P.loaded = {}
  P.strips_failed = nil
  for index in pairs(P.images) do
    if index >= resp.nstrips then
      delete_image(P, index)
    end
  end
  set_highlights(P)
  write_lines(P)
  local win = preview_win(P)
  if P.position_on_layout and win then
    position_preview(P, win, P.position_on_layout)
    P.entered_top = vim.fn.getwininfo(win)[1].topline
    P.position_on_layout = nil
  elseif win and win == P.split_win then
    sync_scroll(P)
  elseif win and meta.keep_line then
    position_preview(P, win, meta.keep_line)
  end
end

local function apply_strips(P, resp)
  local L = P.layout
  if not L or resp.doc ~= L.doc or #vim.fn.win_findbuf(P.buf) == 0 then
    -- Not for the current layout, or the preview was hidden meanwhile (its
    -- images were all deleted): drop it, and the files kitty won't read.
    for _, strip in ipairs(resp.strips or {}) do
      os.remove(strip.path)
    end
    return
  end
  for _, strip in ipairs(resp.strips) do
    local id = P.base_id + strip.index
    vim.api.nvim_set_hl(0, hl_name(id), { fg = string.format("#%06x", id) })
    P.term.request({
      a = "t",
      t = "t",
      f = 24,
      s = L.image_cols * L.geom.cell_w,
      v = strip.rows * L.geom.cell_h,
      i = id,
      data = vim.base64.encode(strip.path),
    })
    P.term.request({ a = "p", U = 1, i = id, p = 1, c = L.image_cols, r = strip.rows })
    P.images[strip.index] = true
    P.loaded[strip.index] = true
  end
end

local close -- forward declaration

local function on_response(P, line)
  if P.closed then
    return
  end
  local req = P.in_flight
  P.in_flight = nil
  local ok, resp = pcall(vim.json.decode, line)
  if not ok or type(resp) ~= "table" then
    notify("bad response from server: " .. line, vim.log.levels.WARN)
    return pump(P)
  end
  if resp.protocol ~= PROTOCOL then
    notify(
      ("dmbdip binary protocol mismatch (got %s, want %d): reinstall dmbdip"):format(
        tostring(resp.protocol == nil and "none" or resp.protocol),
        PROTOCOL
      ),
      vim.log.levels.ERROR
    )
    return close(P)
  end
  if resp.type == "error" then
    -- Repeated errors (e.g. a too narrow window on every edit) are shown once
    -- until a request succeeds again.
    local msg = tostring(resp.message)
    if msg ~= P.last_error then
      P.last_error = msg
      notify(msg, vim.log.levels.WARN)
    end
    if req and req.type == "strips" and P.layout then
      P.strips_failed = P.layout.doc
    end
  elseif resp.type == "layout" and req and req.type == "layout" then
    P.last_error = nil
    apply_layout(P, resp, req.meta)
  elseif resp.type == "strips" then
    P.last_error = nil
    apply_strips(P, resp)
  end
  -- "stale": dropped; the next pump requests strips of the current doc.
  pump(P)
end

---------------------------------------------------------------------------
-- Showing / hiding the preview buffer
---------------------------------------------------------------------------

--- Remember a window's options before it shows the preview. Done before the
--- buffer switch when possible: on re-entry neovim already restores the
--- options the preview buffer last had in that window.
local function save_window_options(P, win)
  if not P.saved_wo[win] then
    local saved = {}
    for k in pairs(PREVIEW_WO) do
      saved[k] = vim.api.nvim_get_option_value(k, { scope = "local", win = win })
    end
    P.saved_wo[win] = saved
  end
end

local function apply_window_options(P, win)
  save_window_options(P, win)
  for k, v in pairs(PREVIEW_WO) do
    vim.api.nvim_set_option_value(k, v, { scope = "local", win = win })
  end
end

local function restore_window_options(P, win)
  local saved = P.saved_wo[win]
  P.saved_wo[win] = nil
  if saved and valid_win(win) then
    for k, v in pairs(saved) do
      pcall(vim.api.nvim_set_option_value, k, v, { scope = "local", win = win })
    end
  end
end

--- The preview buffer was just shown in `win`.
local function on_show(P, win)
  P.win = win
  apply_window_options(P, win)
  -- When toggling in, the caller positions the view first, then pumps, so
  -- the first strips requested are the ones around the mapped position.
  if not P.toggling then
    pump(P)
  end
end

--- Drop images once the preview buffer is shown in no window.
local function check_hidden(P)
  if not P.closed and #vim.fn.win_findbuf(P.buf) == 0 then
    delete_all_images(P)
  end
end

---------------------------------------------------------------------------
-- Lifecycle
---------------------------------------------------------------------------

close = function(P)
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
  delete_all_images(P)
  -- Closing stdin makes the server exit and remove its leftover strip files.
  if P.job then
    pcall(vim.fn.chanclose, P.job, "stdin")
  end
  for win in pairs(P.saved_wo) do
    if valid_win(win) and vim.api.nvim_win_get_buf(win) ~= P.buf then
      restore_window_options(P, win)
    end
  end
  if vim.api.nvim_buf_is_valid(P.buf) then
    -- Windows showing the preview in place go back to the source if it
    -- still exists.
    for _, win in ipairs(vim.fn.win_findbuf(P.buf)) do
      if win ~= P.split_win and vim.api.nvim_buf_is_valid(P.src) and vim.bo[P.src].buflisted then
        pcall(vim.api.nvim_win_set_buf, win, P.src)
        restore_window_options(P, win)
      end
    end
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

local function create(src)
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

  local buf = vim.api.nvim_create_buf(false, true)
  vim.bo[buf].buftype = "nofile"
  vim.bo[buf].bufhidden = "hide"
  vim.bo[buf].swapfile = false
  vim.bo[buf].modifiable = false
  vim.bo[buf].filetype = "dmbdip"
  pcall(vim.api.nvim_buf_set_name, buf, "dmbdip://" .. vim.api.nvim_buf_get_name(src))

  local P = {
    src = src,
    buf = buf,
    term = term,
    base_id = math.random(ID_MIN, ID_MAX - 1),
    timer = assert(vim.uv.new_timer()),
    augroup = vim.api.nvim_create_augroup("dmbdip_preview_" .. src, { clear = true }),
    next_id = 0,
    images = {}, -- strip index -> true: image exists in the terminal
    loaded = {}, -- strip index -> true: image is from the current layout
    saved_wo = {}, -- window -> options to restore when it stops showing the preview
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
  P.job = started and job > 0 and job or nil
  if not P.job then
    notify("could not start " .. tostring(config.cmd) .. (started and "" or (": " .. tostring(job))), vim.log.levels.ERROR)
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
      if split_active(P) then
        schedule_layout(P)
      end
    end,
  })
  au("BufWinEnter", {
    buffer = buf,
    callback = function()
      on_show(P, vim.api.nvim_get_current_win())
    end,
  })
  au("BufWinEnter", {
    callback = function(ev)
      -- A window that showed the preview now shows another buffer.
      local win = vim.api.nvim_get_current_win()
      if ev.buf ~= buf and P.saved_wo[win] then
        restore_window_options(P, win)
      end
      if ev.buf == src then
        P.src_win = win
      end
    end,
  })
  au("BufWinLeave", {
    buffer = buf,
    callback = function()
      vim.schedule(function()
        check_hidden(P)
      end)
    end,
  })
  au("WinEnter", {
    callback = function()
      local win = vim.api.nvim_get_current_win()
      local b = vim.api.nvim_win_get_buf(win)
      if b == buf then
        P.win = win
        pump(P)
      elseif b == src then
        P.src_win = win
        sync_scroll(P)
      end
    end,
  })
  au("WinScrolled", {
    callback = function()
      local ev = vim.v.event
      local sw = source_win(P)
      if sw and ev[tostring(sw)] and split_active(P) then
        sync_scroll(P)
      end
      local pw = preview_win(P)
      if pw and (ev[tostring(pw)] or (sw and ev[tostring(sw)])) then
        pump(P)
      end
    end,
  })
  au("WinResized", {
    callback = function()
      local pw = preview_win(P)
      if pw and vim.tbl_contains(vim.v.event.windows or {}, pw) then
        pump(P)
      end
    end,
  })
  au("VimResized", {
    callback = function()
      pump(P)
    end,
  })
  au("WinClosed", {
    callback = function(ev)
      local win = tonumber(ev.match)
      P.saved_wo[win] = nil
      if win == P.split_win then
        P.split_win = nil
      end
    end,
  })
  au("ColorScheme", {
    callback = function()
      set_highlights(P)
    end,
  })
  -- Teardown on delete/wipe only: toggling in may unload the source
  -- ('nohidden'), and toggling out reloads it.
  au({ "BufWipeout", "BufDelete" }, {
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

  return P
end

---------------------------------------------------------------------------
-- Commands
---------------------------------------------------------------------------

local function toggle_in(src, win)
  local bh = vim.bo[src].bufhidden
  if bh == "delete" or bh == "wipe" then
    notify(("buffer has bufhidden=%s, toggling would delete it: use :DmbdipPreview"):format(bh), vim.log.levels.WARN)
    return
  end
  local P = previews[src] or create(src)
  if not P then
    return
  end
  local src_view = vim.api.nvim_win_call(win, vim.fn.winsaveview)
  P.snapshot = {
    text = table.concat(vim.api.nvim_buf_get_lines(src, 0, -1, false), "\n"),
    tick = vim.api.nvim_buf_get_changedtick(src),
    line_count = vim.api.nvim_buf_line_count(src),
  }
  P.win = win
  save_window_options(P, win)
  P.toggling = true
  local ok, err = pcall(vim.api.nvim_win_set_buf, win, P.buf)
  P.toggling = false
  if not ok then
    P.snapshot = nil
    P.saved_wo[win] = nil
    notify(tostring(err), vim.log.levels.WARN)
    return
  end
  -- BufWinEnter has applied the window options; position, then request.
  P.entered = { view = src_view }
  local current = P.layout
    and not needs_layout(P, geometry(P, win))
    and not (P.in_flight and P.in_flight.type == "layout")
  if current then
    position_preview(P, win, src_view.topline)
    P.entered_top = vim.fn.getwininfo(win)[1].topline
  else
    P.position_on_layout = src_view.topline
    P.entered_top = nil
  end
  pump(P)
end

local function toggle_out(P, win)
  local top = vim.fn.getwininfo(win)[1].topline
  local entered = P.entered
  local src_top
  if entered and (P.entered_top == nil or P.entered_top == top) then
    -- Not scrolled (or not even positioned yet) since toggling in: back to
    -- exactly where we were.
    src_top = entered.view.topline
  else
    src_top = row_to_line(P, top - 1)
  end
  P.position_on_layout = nil
  local ok, err = pcall(vim.api.nvim_win_set_buf, win, P.src)
  if not ok then
    notify(tostring(err), vim.log.levels.WARN)
    return
  end
  P.snapshot = nil
  vim.api.nvim_win_call(win, function()
    vim.fn.winrestview({ topline = src_top, lnum = src_top, col = 0 })
    local cur = entered and entered.view
    if cur then
      local bot = vim.fn.line("w$")
      if cur.lnum >= src_top and cur.lnum <= bot then
        vim.fn.winrestview({ topline = src_top, lnum = cur.lnum, col = cur.col, curswant = cur.curswant })
      end
    end
  end)
  P.entered = nil
  P.entered_top = nil
end

--- `:DmbdipToggle`: swap the current window between a markdown buffer and
--- its preview.
function M.toggle_inplace()
  local win = vim.api.nvim_get_current_win()
  local buf = vim.api.nvim_win_get_buf(win)
  local P = preview_of_buf(buf)
  if P then
    return toggle_out(P, win)
  end
  toggle_in(buf, win)
end

--- `:DmbdipPreview`: open or close the preview split for the current buffer.
function M.toggle_split()
  local win = vim.api.nvim_get_current_win()
  local buf = vim.api.nvim_win_get_buf(win)
  local P = preview_of_buf(buf)
  if P and win ~= P.split_win then
    notify("preview shown in place: use :DmbdipToggle")
    return
  end
  P = P or previews[buf]
  if P and split_active(P) then
    if #vim.api.nvim_list_wins() > 1 then
      vim.api.nvim_win_close(P.split_win, false)
    end
    P.split_win = nil
    return
  end
  P = P or create(buf)
  if not P then
    return
  end
  P.src_win = win
  local split = vim.api.nvim_open_win(P.buf, false, { split = "right", win = win })
  P.split_win = split
  on_show(P, split)
  if P.layout then
    sync_scroll(P)
    pump(P)
  end
end

--- Backwards-compatible name for `:DmbdipPreview`.
M.toggle = M.toggle_split

function M.close_all()
  for _, P in pairs(previews) do
    close(P)
  end
end

-- Test hooks.
M._pump = function(src)
  local P = previews[src]
  if P then
    pump(P)
  end
end
M._sync_split = function(src)
  local P = previews[src]
  if P then
    sync_scroll(P)
  end
end
M._line_to_row = function(src, line)
  return line_to_row(previews[src], line)
end
M._row_to_line = function(src, row)
  return row_to_line(previews[src], row)
end

return M
