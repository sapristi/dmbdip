# File Browser Mode Design

## Summary

Add a two-pane file browser mode to dmbdip. When a directory is passed as argument, display a text-based file listing on the left and a rendered markdown preview on the right.

## Layout

- **Left pane** (~30 chars wide): plain text via crossterm
  - Folders first (with `/` suffix), then `.md` files, alphabetically sorted
  - Cursor: colored background on selected line
- **Right pane** (remaining width): Kitty graphics rendered markdown preview
  - Read-only, shows top of document, no scrolling

## State

New `BrowserState` struct, separate from `AppState`:
- `current_dir: PathBuf` — directory being browsed
- `entries: Vec<Entry>` — sorted list of dirs + .md files
- `cursor: usize` — selected entry index
- `preview: Option<RgbImage>` — rendered markdown preview

## Keybindings

| Key | Action |
|-----|--------|
| Up/Down, j/k | Move cursor |
| Enter | Preview selected .md file in right pane |
| Right / l | Enter subfolder |
| Left / h | Go to parent directory |
| q / Esc | Quit |

## Flow

1. Detect directory argument in `main()`
2. Scan directory → build sorted entry list (dirs first, then .md files)
3. Render file list in left pane using crossterm text output
4. On Enter (markdown file): parse & render with reduced viewport width → display in right pane via Kitty protocol
5. On Right/l (subfolder): change directory, rescan, clear preview
6. On Left/h: navigate to parent, rescan, clear preview
7. On q/Esc: quit

## Preview Rendering

Reuses existing `parse_markdown()` + `render_markdown()` pipeline with viewport width reduced by left pane width. Creates a temporary render context per preview — no scrolling or interaction.

## Scope

- No changes to existing single-file viewer mode
- No auto-preview on cursor move (Enter only)
- No navigation/scrolling within preview
