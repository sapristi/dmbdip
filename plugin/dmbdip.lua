if vim.g.loaded_dmbdip then
  return
end
vim.g.loaded_dmbdip = true

vim.api.nvim_create_user_command("DmbdipPreview", function()
  require("dmbdip").toggle()
end, { desc = "Toggle dmbdip live preview for the current buffer" })

vim.api.nvim_create_autocmd("VimLeavePre", {
  group = vim.api.nvim_create_augroup("dmbdip", { clear = true }),
  callback = function()
    if package.loaded["dmbdip"] then
      require("dmbdip").close_all()
    end
  end,
})
