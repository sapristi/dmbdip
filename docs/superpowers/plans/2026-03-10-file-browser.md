# File Browser Mode Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a two-pane file browser mode (text left, image right) when a directory is passed as argument.

**Architecture:** Detect directory vs file in `main()`, branch into either existing viewer or new browser loop. `BrowserState` manages directory entries and preview state. Left pane uses crossterm text; right pane reuses existing Kitty rendering pipeline with reduced width.

**Tech Stack:** Rust, crossterm (text output + events), existing Kitty graphics pipeline, `std::fs` for directory scanning.

---

## Chunk 1: File Browser Implementation

### Task 1: Add directory detection and `scan_directory()` helper

**Files:**
- Modify: `src/main.rs:1638-1660` (main function, argument handling)

- [ ] **Step 1: Add `scan_directory()` function**

Add above the `main()` function (around line 1636). This scans a directory and returns sorted entries (folders first, then `.md` files):

```rust
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
enum BrowserEntry {
    Dir(String),    // directory name
    File(String),   // .md filename
}

fn scan_directory(dir: &Path) -> Vec<BrowserEntry> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue; // skip hidden
            }
            let ft = entry.file_type();
            if let Ok(ft) = ft {
                if ft.is_dir() {
                    dirs.push(name);
                } else if name.ends_with(".md") || name.ends_with(".MD") {
                    files.push(name);
                }
            }
        }
    }
    dirs.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    files.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    let mut result = Vec::new();
    for d in dirs { result.push(BrowserEntry::Dir(d)); }
    for f in files { result.push(BrowserEntry::File(f)); }
    result
}
```

- [ ] **Step 2: Add directory detection in `main()`**

Modify the argument handling in `main()` (line 1656 area). After `let file_path = &args[1];`, add a check:

```rust
let path = Path::new(file_path);
if path.is_dir() {
    return run_browser(path, &fonts, vp_width, vp_height);
}
```

Move `let fonts = load_fonts();` and viewport size before this check so they're available.

- [ ] **Step 3: Add stub `run_browser()` that just quits on `q`**

```rust
fn run_browser(dir: &Path, fonts: &Fonts, vp_width: u32, vp_height: u32) -> io::Result<()> {
    let entries = scan_directory(dir);
    let stdout = io::stdout();
    terminal::enable_raw_mode()?;
    {
        let mut out = BufWriter::new(stdout.lock());
        execute!(out, terminal::EnterAlternateScreen, cursor::Hide, terminal::Clear(ClearType::All))?;
        // Just show entry count for now
        execute!(out, cursor::MoveTo(0, 0))?;
        write!(out, "Browsing: {} ({} entries). Press q to quit.", dir.display(), entries.len())?;
        out.flush()?;
    }
    loop {
        if let Event::Key(KeyEvent { code: KeyCode::Char('q'), kind: KeyEventKind::Press, .. }) = event::read()? {
            break;
        }
    }
    {
        let mut out = BufWriter::new(stdout.lock());
        execute!(out, cursor::Show, terminal::LeaveAlternateScreen)?;
    }
    terminal::disable_raw_mode()?;
    Ok(())
}
```

- [ ] **Step 4: Test manually**

Run: `cargo build && ./target/debug/dmbdip .`
Expected: Shows "Browsing: . (N entries). Press q to quit." and exits on `q`.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: add directory detection and scan_directory helper"
```

---

### Task 2: Render text-based file listing in left pane

**Files:**
- Modify: `src/main.rs` (inside `run_browser()`)

- [ ] **Step 1: Add `BrowserState` struct and left pane rendering**

```rust
struct BrowserState {
    current_dir: PathBuf,
    entries: Vec<BrowserEntry>,
    cursor: usize,
    preview_img: Option<RgbImage>,
    frame: u32,
}

const BROWSER_LEFT_COLS: u16 = 35; // character columns for left pane
```

- [ ] **Step 2: Add `draw_file_list()` function**

Uses crossterm to render the file list in the left pane area:

```rust
fn draw_file_list(out: &mut impl Write, state: &BrowserState, term_rows: u16) -> io::Result<()> {
    let max_display = (term_rows.saturating_sub(2)) as usize; // leave room for header + status
    // Header: current directory
    execute!(out, cursor::MoveTo(0, 0))?;
    let dir_str = state.current_dir.display().to_string();
    let header = if dir_str.len() > BROWSER_LEFT_COLS as usize - 1 {
        format!("…{}", &dir_str[dir_str.len() - (BROWSER_LEFT_COLS as usize - 2)..])
    } else {
        dir_str
    };
    write!(out, "\x1b[1;36m{:<width$}\x1b[0m", header, width = BROWSER_LEFT_COLS as usize)?;

    // Compute scroll window
    let total = state.entries.len();
    let scroll_offset = if state.cursor >= max_display {
        state.cursor - max_display + 1
    } else {
        0
    };

    for i in 0..max_display {
        let idx = scroll_offset + i;
        execute!(out, cursor::MoveTo(0, (i + 1) as u16))?;
        if idx < total {
            let is_selected = idx == state.cursor;
            let (prefix, name_display, color) = match &state.entries[idx] {
                BrowserEntry::Dir(name) => ("📁 ", format!("{}/", name), "\x1b[1;34m"),
                BrowserEntry::File(name) => ("   ", name.clone(), "\x1b[0;37m"),
            };
            if is_selected {
                write!(out, "\x1b[7m{}{}{:<width$}\x1b[0m", color, prefix, name_display,
                    width = BROWSER_LEFT_COLS as usize - prefix.len())?;
            } else {
                write!(out, "{}{}{:<width$}\x1b[0m", color, prefix, name_display,
                    width = BROWSER_LEFT_COLS as usize - prefix.len())?;
            }
        } else {
            write!(out, "{:<width$}", "", width = BROWSER_LEFT_COLS as usize)?;
        }
    }
    out.flush()
}
```

- [ ] **Step 3: Wire `draw_file_list` into `run_browser` with cursor navigation**

Replace the stub loop with full navigation:

```rust
fn run_browser(dir: &Path, fonts: &Fonts, vp_width: u32, vp_height: u32) -> io::Result<()> {
    let (term_cols, term_rows) = terminal::size()?;
    let mut state = BrowserState {
        current_dir: dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf()),
        entries: scan_directory(dir),
        cursor: 0,
        preview_img: None,
        frame: 1,
    };

    let stdout = io::stdout();
    terminal::enable_raw_mode()?;
    {
        let mut out = BufWriter::new(stdout.lock());
        execute!(out, terminal::EnterAlternateScreen, cursor::Hide, terminal::Clear(ClearType::All))?;
        draw_file_list(&mut out, &state, term_rows)?;
    }

    loop {
        if let Event::Key(KeyEvent { code, kind: KeyEventKind::Press, .. }) = event::read()? {
            let needs_redraw = match code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Down | KeyCode::Char('j') => {
                    if state.cursor + 1 < state.entries.len() {
                        state.cursor += 1;
                    }
                    true
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if state.cursor > 0 {
                        state.cursor -= 1;
                    }
                    true
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    // Enter subfolder
                    if let Some(BrowserEntry::Dir(name)) = state.entries.get(state.cursor) {
                        let new_dir = state.current_dir.join(name);
                        state.entries = scan_directory(&new_dir);
                        state.current_dir = new_dir.canonicalize().unwrap_or(new_dir);
                        state.cursor = 0;
                        state.preview_img = None;
                        // Clear right pane (delete kitty images)
                        let mut out = BufWriter::new(stdout.lock());
                        write!(out, "\x1b_Ga=d,d=I,i=1,q=2\x1b\\\x1b_Ga=d,d=I,i=2,q=2\x1b\\")?;
                        execute!(out, terminal::Clear(ClearType::All))?;
                    }
                    true
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    // Go to parent
                    if let Some(parent) = state.current_dir.parent() {
                        let parent = parent.to_path_buf();
                        state.entries = scan_directory(&parent);
                        state.current_dir = parent.canonicalize().unwrap_or(parent);
                        state.cursor = 0;
                        state.preview_img = None;
                        let mut out = BufWriter::new(stdout.lock());
                        write!(out, "\x1b_Ga=d,d=I,i=1,q=2\x1b\\\x1b_Ga=d,d=I,i=2,q=2\x1b\\")?;
                        execute!(out, terminal::Clear(ClearType::All))?;
                    }
                    true
                }
                KeyCode::Enter => {
                    // Preview markdown file
                    if let Some(BrowserEntry::File(name)) = state.entries.get(state.cursor) {
                        let file_path = state.current_dir.join(name);
                        if let Ok(source) = std::fs::read_to_string(&file_path) {
                            let preview_width = vp_width.saturating_sub(BROWSER_LEFT_COLS as u32 * 8);
                            let blocks = parse_markdown(&source);
                            let headings = build_headings(&blocks);
                            let theme = default_theme();
                            let (img, _, _) = render_markdown(
                                &blocks, &headings, fonts, preview_width, &theme,
                            );
                            // Display preview in right pane area
                            let preview_h = vp_height.min(img.height());
                            let src_w = preview_width.min(img.width());
                            let raw = img.as_raw();
                            let img_stride = img.width() as usize * 3;
                            let mut viewport_data = Vec::with_capacity(src_w as usize * preview_h as usize * 3);
                            for row in 0..preview_h as usize {
                                let offset = row * img_stride;
                                viewport_data.extend_from_slice(&raw[offset..offset + src_w as usize * 3]);
                            }
                            let mut out = BufWriter::new(stdout.lock());
                            // Position cursor at top of right pane
                            execute!(out, cursor::MoveTo(BROWSER_LEFT_COLS + 1, 0))?;
                            let new_id = state.frame;
                            let old_id = if new_id == 1 { 2 } else { 1 };
                            state.frame = old_id;
                            // Use placement with offset
                            kitty_display_raw(&mut out, &viewport_data, src_w, preview_h, new_id, old_id)?;
                            state.preview_img = Some(img);
                        }
                    }
                    true
                }
                _ => false,
            };
            if needs_redraw {
                let mut out = BufWriter::new(stdout.lock());
                draw_file_list(&mut out, &state, term_rows)?;
            }
        }
    }

    {
        let mut out = BufWriter::new(stdout.lock());
        write!(out, "\x1b_Ga=d,d=I,i=1,q=2\x1b\\\x1b_Ga=d,d=I,i=2,q=2\x1b\\")?;
        execute!(out, cursor::Show, terminal::LeaveAlternateScreen)?;
    }
    terminal::disable_raw_mode()?;
    Ok(())
}
```

- [ ] **Step 4: Test manually**

Run: `cargo build && ./target/debug/dmbdip .`
Expected: File list with cursor navigation. Arrow keys move cursor. Right enters folders, Left goes to parent.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: add file browser with text listing and cursor navigation"
```

---

### Task 3: Wire up markdown preview in right pane

**Files:**
- Modify: `src/main.rs` (refine preview rendering in `run_browser`)

The Enter key handler from Task 2 already includes preview logic. This task focuses on fixing the Kitty image placement to render in the right pane (not overlapping the left pane).

- [ ] **Step 1: Fix Kitty image placement for right pane**

The Kitty graphics protocol places images at the current cursor position. We need to position the cursor at column `BROWSER_LEFT_COLS + 1` before sending the image. The `kitty_display_raw` function starts with `\x1b[H` which moves to (0,0) — we need a variant that positions at a specific column.

Add a new function:

```rust
fn kitty_display_at(
    w: &mut impl Write,
    data: &[u8],
    width: u32,
    height: u32,
    col: u16,
    new_id: u32,
    old_id: u32,
) -> io::Result<()> {
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    let chunk_size = 4096;
    let bytes = b64.as_bytes();
    let total_chunks = (bytes.len() + chunk_size - 1) / chunk_size;

    // Position at top of right pane
    write!(w, "\x1b[1;{}H", col + 1)?;

    for (i, chunk) in bytes.chunks(chunk_size).enumerate() {
        let chunk_str = std::str::from_utf8(chunk).unwrap();
        let is_last = i == total_chunks - 1;
        let m = if is_last { 0 } else { 1 };

        if i == 0 {
            write!(
                w,
                "\x1b_Ga=T,i={new_id},f=24,s={width},v={height},q=2,C=1,m={m};{chunk_str}\x1b\\"
            )?;
        } else {
            write!(w, "\x1b_Gm={m};{chunk_str}\x1b\\")?;
        }
    }

    write!(w, "\x1b_Ga=d,d=I,i={old_id},q=2\x1b\\")?;
    w.flush()
}
```

- [ ] **Step 2: Update Enter handler to use `kitty_display_at`**

In the Enter branch, replace the `kitty_display_raw` call with:

```rust
kitty_display_at(&mut out, &viewport_data, src_w, preview_h, BROWSER_LEFT_COLS + 1, new_id, old_id)?;
```

- [ ] **Step 3: Test manually**

Run: `cargo build && ./target/debug/dmbdip .`
Expected: Navigate to a `.md` file, press Enter. Preview appears in the right portion of the terminal. Left pane text stays visible.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat: render markdown preview in right pane of file browser"
```

---

### Task 4: Update help text and polish

**Files:**
- Modify: `src/main.rs` (help text, usage string)

- [ ] **Step 1: Update usage/help text**

Change the usage line (around line 1644):

```rust
eprintln!("Usage: dmbdip <markdown-file-or-directory>");
eprintln!();
eprintln!("Renders a markdown file as an image and displays it in the terminal");
eprintln!("using the Kitty graphics protocol. When given a directory, opens a");
eprintln!("file browser showing markdown files and subfolders.");
```

- [ ] **Step 2: Update DEVELOPMENT.md with completed task**

Add to the Completed Tasks list:
```
- [x] Task 9: File browser mode with two-pane layout
```

- [ ] **Step 3: Test both modes manually**

Run: `cargo build && ./target/debug/dmbdip sample.md` — existing viewer works unchanged
Run: `cargo build && ./target/debug/dmbdip .` — browser mode with navigation and preview

- [ ] **Step 4: Commit**

```bash
git add src/main.rs DEVELOPMENT.md
git commit -m "feat: update help text for directory browsing support"
```
