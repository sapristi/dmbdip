if vim.g.loaded_dmbdip then
  return
end
vim.g.loaded_dmbdip = true

vim.api.nvim_create_user_command("DmbdipToggle", function()
  require("dmbdip").toggle_inplace()
end, { desc = "Swap the current window between a markdown buffer and its dmbdip preview" })

vim.api.nvim_create_user_command("DmbdipPreview", function()
  require("dmbdip").toggle_split()
end, { desc = "Toggle the dmbdip live preview split for the current buffer" })

vim.api.nvim_create_user_command("DmbdipNextHeading", function()
  require("dmbdip").next_heading()
end, { desc = "Move to the next visible heading in the dmbdip preview" })

vim.api.nvim_create_user_command("DmbdipPrevHeading", function()
  require("dmbdip").prev_heading()
end, { desc = "Move to the previous visible heading in the dmbdip preview" })

vim.api.nvim_create_user_command("DmbdipFold", function()
  require("dmbdip").toggle_fold()
end, { desc = "Fold or unfold the current section in the dmbdip preview" })

vim.api.nvim_create_autocmd("VimLeavePre", {
  group = vim.api.nvim_create_augroup("dmbdip", { clear = true }),
  callback = function()
    if package.loaded["dmbdip"] then
      require("dmbdip").close_all()
    end
  end,
})
