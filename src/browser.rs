use crossterm::{
    cursor, execute,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal::{self, ClearType},
};
use std::time::Duration;
use image::RgbImage;
use std::io::{self, BufWriter, Write};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::constants::{LayoutParams, BROWSER_KEYBINDINGS};
use crate::fonts::Fonts;
use crate::headings::build_headings;
use crate::kitty::{display_viewport, get_viewport_pixel_size};
use crate::overlay::{render_help_overlay, render_help_overlay_with, render_search_bar};
use crate::parsing::parse_markdown;
use crate::render::render_preview;
use crate::source_render::render_source_preview;
use crate::source_state::SourceViewState;
use crate::state::AppState;
use crate::theme::Theme;

const RESIZE_DEBOUNCE: Duration = Duration::from_millis(100);

#[derive(Clone, Debug)]
enum BrowserEntry {
    Dir(String),
    File(String),
}

fn scan_directory(dir: &Path, extra_extensions: &[String]) -> Vec<BrowserEntry> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            if let Ok(ft) = entry.file_type() {
                if ft.is_dir() {
                    dirs.push(name);
                } else if is_supported_file(&name, extra_extensions) {
                    files.push(name);
                }
            }
        }
    }
    dirs.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    files.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
    let mut result = Vec::new();
    for d in dirs {
        result.push(BrowserEntry::Dir(d));
    }
    for f in files {
        result.push(BrowserEntry::File(f));
    }
    result
}

fn is_supported_file(name: &str, extra_extensions: &[String]) -> bool {
    if name.ends_with(".md") || name.ends_with(".MD") {
        return true;
    }
    if let Some(ext) = name.rsplit('.').next() {
        extra_extensions.iter().any(|e| e.eq_ignore_ascii_case(ext))
    } else {
        false
    }
}

fn is_markdown(name: &str) -> bool {
    name.ends_with(".md") || name.ends_with(".MD")
}

fn file_extension(name: &str) -> String {
    name.rsplit('.').next().unwrap_or("").to_string()
}

enum DocContent {
    Markdown(AppState),
    Source(SourceViewState),
}

struct PreviewCache {
    entries: Vec<(PathBuf, RgbImage)>,
    capacity: usize,
}

impl PreviewCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            capacity,
        }
    }

    fn get(&self, path: &Path) -> Option<&RgbImage> {
        self.entries
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, img)| img)
    }

    fn insert(&mut self, path: PathBuf, img: RgbImage) {
        self.entries.retain(|(p, _)| p != &path);
        self.entries.insert(0, (path, img));
        if self.entries.len() > self.capacity {
            self.entries.pop();
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

struct SavedPosition {
    scroll_y: u32,
    current_heading: Option<usize>,
    folded: Vec<bool>,
}

struct BrowserState {
    current_dir: PathBuf,
    entries: Vec<BrowserEntry>,
    cursor: usize,
    doc_state: Option<DocContent>,
    doc_mode: bool,
    doc_path: Option<PathBuf>,
    preview_cache: PreviewCache,
    preview_frame: u32,
    position_cache: HashMap<PathBuf, SavedPosition>,
    file_list_visible: bool,
    extra_extensions: Vec<String>,
}

const BROWSER_LEFT_COLS: u16 = 35;
const DOC_EXTRA_MARGIN: u32 = 40;

fn draw_file_list(out: &mut impl Write, state: &BrowserState, term_rows: u16) -> io::Result<()> {
    let max_display = (term_rows.saturating_sub(2)) as usize;
    execute!(out, cursor::MoveTo(0, 0))?;
    let dir_str = state.current_dir.display().to_string();
    let header = if dir_str.len() > BROWSER_LEFT_COLS as usize - 1 {
        format!(
            "\u{2026}{}",
            &dir_str[dir_str.len() - (BROWSER_LEFT_COLS as usize - 2)..]
        )
    } else {
        dir_str
    };
    write!(
        out,
        "\x1b[1;36m{:<width$}\x1b[0m",
        header,
        width = BROWSER_LEFT_COLS as usize
    )?;

    let scroll_offset = if state.cursor >= max_display {
        state.cursor - max_display + 1
    } else {
        0
    };

    let total = state.entries.len();
    for i in 0..max_display {
        let idx = scroll_offset + i;
        execute!(out, cursor::MoveTo(0, (i + 1) as u16))?;
        if idx < total {
            let is_selected = idx == state.cursor;
            let (prefix, name_display, color, sel_color) = match &state.entries[idx] {
                BrowserEntry::Dir(name) => (" > ", format!("{}/", name), "\x1b[1;34m", "\x1b[7;1;34m"),
                BrowserEntry::File(name) => {
                    if is_selected && state.doc_mode {
                        ("   ", name.clone(), "\x1b[36m", "\x1b[7;36m")
                    } else {
                        ("   ", name.clone(), "\x1b[37m", "\x1b[7;37m")
                    }
                }
            };
            let display_width = BROWSER_LEFT_COLS as usize;
            let mut line = format!("{}{}", prefix, name_display);
            if line.len() > display_width {
                line.truncate(display_width);
            }
            if is_selected {
                write!(
                    out,
                    "{}{:<width$}\x1b[0m",
                    sel_color,
                    line,
                    width = display_width
                )?;
            } else {
                write!(
                    out,
                    "{}{:<width$}\x1b[0m",
                    color,
                    line,
                    width = display_width
                )?;
            }
        } else {
            write!(
                out,
                "{:<width$}",
                "",
                width = BROWSER_LEFT_COLS as usize
            )?;
        }
    }
    out.flush()
}

fn browser_clear_preview(out: &mut impl Write) -> io::Result<()> {
    write!(
        out,
        "\x1b_Ga=d,d=I,i=1,q=2\x1b\\\x1b_Ga=d,d=I,i=2,q=2\x1b\\"
    )?;
    execute!(out, terminal::Clear(ClearType::All))
}

fn show_preview(
    out: &mut impl Write,
    state: &mut BrowserState,
    fonts: &Fonts,
    preview_width: u32,
    vp_height: u32,
    theme: &Theme,
    layout: &LayoutParams,
) -> io::Result<()> {
    match state.entries.get(state.cursor) {
        Some(BrowserEntry::File(name)) if is_supported_file(name, &state.extra_extensions) => {
            let file_path = state.current_dir.join(name);
            if state.preview_cache.get(&file_path).is_none() {
                if let Ok(source) = std::fs::read_to_string(&file_path) {
                    let img = if is_markdown(name) {
                        let blocks = parse_markdown(&source);
                        let headings = build_headings(&blocks);
                        render_preview(&blocks, &headings, preview_width, vp_height, fonts, theme, layout)
                    } else {
                        let ext = file_extension(name);
                        render_source_preview(&source, &ext, preview_width, vp_height, fonts, theme, layout)
                    };
                    state.preview_cache.insert(file_path.clone(), img);
                }
            }
            if let Some(img) = state.preview_cache.get(&file_path) {
                display_viewport(
                    out,
                    img,
                    0,
                    preview_width,
                    vp_height,
                    &mut state.preview_frame,
                    Some(BROWSER_LEFT_COLS + 1),
                    None,
                    None,
                    &[],
                    0,
                )?;
            }
        }
        _ => {
            write!(
                out,
                "\x1b_Ga=d,d=I,i=1,q=2\x1b\\\x1b_Ga=d,d=I,i=2,q=2\x1b\\"
            )?;
            out.flush()?;
        }
    }
    Ok(())
}

fn doc_width(vp_width: u32, file_list_visible: bool) -> u32 {
    if file_list_visible {
        vp_width.saturating_sub(BROWSER_LEFT_COLS as u32 * 8)
    } else {
        vp_width + DOC_EXTRA_MARGIN * 2
    }
}

fn doc_col(file_list_visible: bool) -> Option<u16> {
    if file_list_visible {
        Some(BROWSER_LEFT_COLS + 1)
    } else {
        None
    }
}

pub(crate) fn run_browser(
    dir: &Path,
    initial_file: Option<&Path>,
    fonts: &Fonts,
    mut vp_width: u32,
    mut vp_height: u32,
    theme: &Theme,
    layout: &LayoutParams,
    extra_extensions: &[String],
) -> io::Result<()> {
    let (_term_cols, mut term_rows) = terminal::size()?;
    let canonical_dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let entries = scan_directory(&canonical_dir, extra_extensions);

    let mut state = BrowserState {
        current_dir: canonical_dir,
        entries,
        cursor: 0,
        doc_state: None,
        doc_mode: false,
        doc_path: None,
        preview_cache: PreviewCache::new(8),
        preview_frame: 1,
        position_cache: HashMap::new(),
        file_list_visible: initial_file.is_none(),
        extra_extensions: extra_extensions.to_vec(),
    };

    if let Some(file_path) = initial_file {
        let file_path = file_path.canonicalize().unwrap_or_else(|_| file_path.to_path_buf());
        let file_name = file_path.file_name().unwrap_or_default().to_string_lossy().to_string();
        if let Some(idx) = state.entries.iter().position(|e| match e {
            BrowserEntry::File(name) => name == &file_name,
            _ => false,
        }) {
            state.cursor = idx;
        }
        if let Ok(source) = std::fs::read_to_string(&file_path) {
            let w = doc_width(vp_width, state.file_list_visible);
            if is_markdown(&file_name) {
                let ds = AppState::new(&source, fonts, w, vp_height, *theme, *layout);
                state.doc_state = Some(DocContent::Markdown(ds));
            } else {
                let ext = file_extension(&file_name);
                let ss = SourceViewState::new(&source, &ext, fonts, w, vp_height, *theme, *layout);
                state.doc_state = Some(DocContent::Source(ss));
            }
            state.doc_mode = true;
            state.doc_path = Some(file_path);
        }
    }

    let stdout = io::stdout();
    terminal::enable_raw_mode()?;
    let mut preview_width = vp_width.saturating_sub(BROWSER_LEFT_COLS as u32 * 8);
    {
        let mut out = BufWriter::new(stdout.lock());
        execute!(
            out,
            terminal::EnterAlternateScreen,
            cursor::Hide,
            terminal::Clear(ClearType::All)
        )?;
        if state.doc_mode {
            let w = doc_width(vp_width, state.file_list_visible);
            let col = doc_col(state.file_list_visible);
            match state.doc_state {
                Some(DocContent::Markdown(ref mut ds)) => {
                    let ci = ds.cursor_info();
                    display_viewport(
                        &mut out, &ds.img, ds.scroll_y, w, vp_height,
                        &mut ds.frame, col, None, ci,
                        &ds.search_highlights, ds.search_current,
                    )?;
                }
                Some(DocContent::Source(ref mut ss)) => {
                    display_viewport(
                        &mut out, &ss.img, ss.scroll_y, w, vp_height,
                        &mut ss.frame, col, None, None,
                        &ss.search_highlights, ss.search_current,
                    )?;
                }
                None => {}
            }
        } else {
            draw_file_list(&mut out, &state, term_rows)?;
            show_preview(&mut out, &mut state, fonts, preview_width, vp_height, theme, layout)?;
        }
    }

    let mut open_in_editor = false;
    loop {
        if open_in_editor {
            open_in_editor = false;
            if let Some(ref path) = state.doc_path {
                let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
                terminal::disable_raw_mode()?;
                {
                    let mut out = BufWriter::new(stdout.lock());
                    execute!(out, terminal::LeaveAlternateScreen, cursor::Show)?;
                }
                let _status = std::process::Command::new(&editor)
                    .arg(path)
                    .status();
                {
                    let mut out = BufWriter::new(stdout.lock());
                    execute!(out, terminal::EnterAlternateScreen, cursor::Hide)?;
                }
                terminal::enable_raw_mode()?;
                state.preview_cache.entries.retain(|(p, _)| p != path);
                if state.doc_mode {
                    let cur_w = doc_width(vp_width, state.file_list_visible);
                    let file_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                    if let Ok(content) = std::fs::read_to_string(path) {
                        if is_markdown(&file_name) {
                            let new_ds = AppState::new(&content, fonts, cur_w, vp_height, *theme, *layout);
                            state.doc_state = Some(DocContent::Markdown(new_ds));
                        } else {
                            let ext = file_extension(&file_name);
                            let new_ss = SourceViewState::new(&content, &ext, fonts, cur_w, vp_height, *theme, *layout);
                            state.doc_state = Some(DocContent::Source(new_ss));
                        }
                    }
                    let col = doc_col(state.file_list_visible);
                    let mut out = BufWriter::new(stdout.lock());
                    execute!(out, terminal::Clear(ClearType::All))?;
                    match state.doc_state {
                        Some(DocContent::Markdown(ref mut ds)) => {
                            let ci = ds.cursor_info();
                            display_viewport(
                                &mut out, &ds.img, ds.scroll_y, cur_w, vp_height,
                                &mut ds.frame, col, None, ci, &ds.search_highlights, ds.search_current,
                            )?;
                        }
                        Some(DocContent::Source(ref mut ss)) => {
                            display_viewport(
                                &mut out, &ss.img, ss.scroll_y, cur_w, vp_height,
                                &mut ss.frame, col, None, None,
                                &ss.search_highlights, ss.search_current,
                            )?;
                        }
                        None => {}
                    }
                } else {
                    state.doc_path = None;
                    let mut out = BufWriter::new(stdout.lock());
                    execute!(out, terminal::Clear(ClearType::All))?;
                    draw_file_list(&mut out, &state, term_rows)?;
                    show_preview(&mut out, &mut state, fonts, preview_width, vp_height, theme, layout)?;
                }
            }
            continue;
        }

        let event = event::read()?;

        if matches!(event, Event::Resize(_, _)) {
            // Debounce: drain any queued resize events within the window
            while event::poll(RESIZE_DEBOUNCE)? {
                let ev = event::read()?;
                if !matches!(ev, Event::Resize(_, _)) {
                    break;
                }
            }
            let (new_w, new_h) = get_viewport_pixel_size()?;
            let (_, new_rows) = terminal::size()?;
            vp_width = new_w;
            vp_height = new_h;
            term_rows = new_rows;
            preview_width = vp_width.saturating_sub(BROWSER_LEFT_COLS as u32 * 8);

            if state.doc_mode {
                let cur_w = doc_width(vp_width, state.file_list_visible);
                let col = doc_col(state.file_list_visible);
                match state.doc_state {
                    Some(DocContent::Markdown(ref mut ds)) => {
                        ds.vp_width = cur_w;
                        ds.vp_height = vp_height;
                        ds.rerender(fonts);
                        if !ds.search_query.is_empty() && !ds.search_matches.is_empty() {
                            ds.execute_search(fonts);
                        }
                        let ci = ds.cursor_info();
                        let mut out = BufWriter::new(stdout.lock());
                        execute!(out, terminal::Clear(ClearType::All))?;
                        display_viewport(
                            &mut out, &ds.img, ds.scroll_y, cur_w, vp_height,
                            &mut ds.frame, col, None, ci,
                            &ds.search_highlights, ds.search_current,
                        )?;
                    }
                    Some(DocContent::Source(ref mut ss)) => {
                        ss.vp_width = cur_w;
                        ss.vp_height = vp_height;
                        ss.rerender(fonts);
                        if !ss.search_query.is_empty() && !ss.search_matches.is_empty() {
                            ss.execute_search(fonts);
                        }
                        let mut out = BufWriter::new(stdout.lock());
                        execute!(out, terminal::Clear(ClearType::All))?;
                        display_viewport(
                            &mut out, &ss.img, ss.scroll_y, cur_w, vp_height,
                            &mut ss.frame, col, None, None,
                            &ss.search_highlights, ss.search_current,
                        )?;
                    }
                    None => {}
                }
            } else {
                state.preview_cache.clear();
                let mut out = BufWriter::new(stdout.lock());
                execute!(out, terminal::Clear(ClearType::All))?;
                draw_file_list(&mut out, &state, term_rows)?;
                show_preview(&mut out, &mut state, fonts, preview_width, vp_height, theme, layout)?;
            }
            continue;
        }

        if let Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            ..
        }) = event
        {
            if state.doc_mode {
                let cur_width = doc_width(vp_width, state.file_list_visible);
                let col = doc_col(state.file_list_visible);

                match state.doc_state {
                    Some(DocContent::Markdown(ref mut ds)) => {
                    let needs_redraw = if ds.search_mode {
                        match (code, modifiers) {
                            (KeyCode::Esc, _) => {
                                ds.search_mode = false;
                                ds.search_query.clear();
                                ds.search_matches.clear();
                                ds.search_highlights.clear();
                                true
                            }
                            (KeyCode::Enter, _) => {
                                ds.search_mode = false;
                                ds.execute_search(fonts);
                                true
                            }
                            (KeyCode::Backspace, _) => {
                                ds.search_query.pop();
                                true
                            }
                            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                                ds.search_query.push(c);
                                true
                            }
                            _ => false,
                        }
                    } else {
                        match (code, modifiers) {
                            (KeyCode::Char('q'), _)
                            | (KeyCode::Esc, _)
                            | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,

                            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                                ds.search_mode = true;
                                ds.search_query.clear();
                                ds.search_matches.clear();
                                ds.search_highlights.clear();
                                ds.search_current = 0;
                                true
                            }
                            (KeyCode::Char('n'), KeyModifiers::NONE) => ds.navigate_search(true),
                            (KeyCode::Char('N'), KeyModifiers::SHIFT) => ds.navigate_search(false),
                            (KeyCode::Down, KeyModifiers::NONE) => ds.navigate_heading(1),
                            (KeyCode::Up, KeyModifiers::NONE) => ds.navigate_heading(-1),

                            (KeyCode::Tab, _) => ds.toggle_fold(fonts),

                            (KeyCode::Right, KeyModifiers::NONE) => {
                                if state.file_list_visible {
                                    state.file_list_visible = false;
                                    let new_width = doc_width(vp_width, false);
                                    ds.vp_width = new_width;
                                    ds.rerender(fonts);
                                    if !ds.search_query.is_empty() && !ds.search_matches.is_empty() {
                                        ds.execute_search(fonts);
                                    }
                                    let mut out = BufWriter::new(stdout.lock());
                                    execute!(out, terminal::Clear(ClearType::All))?;
                                    let ci = ds.cursor_info();
                                    display_viewport(
                                        &mut out,
                                        &ds.img,
                                        ds.scroll_y,
                                        new_width,
                                        vp_height,
                                        &mut ds.frame,
                                        doc_col(false),
                                        None,
                                        ci,
                                        &ds.search_highlights,
                                        ds.search_current,
                                    )?;
                                    continue;
                                } else {
                                    false
                                }
                            }

                            (KeyCode::Left, KeyModifiers::NONE) => {
                                state.position_cache.insert(
                                    state.doc_path.clone().unwrap(),
                                    SavedPosition {
                                        scroll_y: ds.scroll_y,
                                        current_heading: ds.current_heading,
                                        folded: ds.headings.iter().map(|h| h.folded).collect(),
                                    },
                                );
                                state.doc_mode = false;
                                state.file_list_visible = true;
                                let mut out = BufWriter::new(stdout.lock());
                                browser_clear_preview(&mut out)?;
                                draw_file_list(&mut out, &state, term_rows)?;
                                show_preview(&mut out, &mut state, fonts, preview_width, vp_height, theme, layout)?;
                                continue;
                            }

                            (KeyCode::Char(' '), KeyModifiers::NONE) => {
                                ds.scroll(layout.scroll_step as i32)
                            }
                            (KeyCode::Char(' '), KeyModifiers::CONTROL) => {
                                ds.scroll(-(layout.scroll_step as i32))
                            }
                            (KeyCode::Char('j'), _) => ds.scroll(layout.scroll_step as i32),
                            (KeyCode::Char('k'), _) => ds.scroll(-(layout.scroll_step as i32)),
                            (KeyCode::PageDown, _) => ds.scroll(vp_height as i32 / 2),
                            (KeyCode::PageUp, _) => ds.scroll(-(vp_height as i32 / 2)),
                            (KeyCode::Home, _) => {
                                let changed = ds.scroll_y != 0;
                                ds.scroll_y = 0;
                                ds.sync_cursor_to_scroll();
                                changed
                            }
                            (KeyCode::End, _) => {
                                let max = ds.max_scroll();
                                let changed = ds.scroll_y != max;
                                ds.scroll_y = max;
                                ds.sync_cursor_to_scroll();
                                changed
                            }
                            (KeyCode::Char('h'), KeyModifiers::NONE) => {
                                let help_img =
                                    render_help_overlay(cur_width, vp_height, fonts);
                                let mut out = BufWriter::new(stdout.lock());
                                display_viewport(
                                    &mut out,
                                    &help_img,
                                    0,
                                    cur_width,
                                    vp_height,
                                    &mut ds.frame,
                                    col,
                                    None,
                                    None,
                                    &[],
                                    0,
                                )?;
                                loop {
                                    if let Event::Key(KeyEvent {
                                        kind: KeyEventKind::Press,
                                        ..
                                    }) = event::read()?
                                    {
                                        break;
                                    }
                                }
                                true
                            }
                            (KeyCode::Char('e'), KeyModifiers::NONE) => {
                                if state.doc_path.is_some() {
                                    open_in_editor = true;
                                    continue;
                                }
                                false
                            }
                            _ => false,
                        }
                    };

                    if needs_redraw {
                        let overlay = if ds.search_mode {
                            Some(render_search_bar(
                                &ds.search_query,
                                None,
                                cur_width,
                                fonts,
                            ))
                        } else if !ds.search_matches.is_empty() {
                            Some(render_search_bar(
                                &ds.search_query,
                                Some((ds.search_current + 1, ds.search_matches.len())),
                                cur_width,
                                fonts,
                            ))
                        } else {
                            None
                        };
                        let ci = ds.cursor_info();
                        let hl = &ds.search_highlights;
                        let sc = ds.search_current;
                        let mut out = BufWriter::new(stdout.lock());
                        display_viewport(
                            &mut out,
                            &ds.img,
                            ds.scroll_y,
                            cur_width,
                            vp_height,
                            &mut ds.frame,
                            col,
                            overlay.as_ref(),
                            ci,
                            hl,
                            sc,
                        )?;
                    }
                    } // end Markdown
                    Some(DocContent::Source(ref mut ss)) => {
                    let needs_redraw = if ss.search_mode {
                        match (code, modifiers) {
                            (KeyCode::Esc, _) => {
                                ss.search_mode = false;
                                ss.search_query.clear();
                                ss.search_matches.clear();
                                ss.search_highlights.clear();
                                true
                            }
                            (KeyCode::Enter, _) => {
                                ss.search_mode = false;
                                ss.execute_search(fonts);
                                true
                            }
                            (KeyCode::Backspace, _) => {
                                ss.search_query.pop();
                                true
                            }
                            (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                                ss.search_query.push(c);
                                true
                            }
                            _ => false,
                        }
                    } else {
                        match (code, modifiers) {
                            (KeyCode::Char('q'), _)
                            | (KeyCode::Esc, _)
                            | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,

                            (KeyCode::Char('/'), KeyModifiers::NONE) => {
                                ss.search_mode = true;
                                ss.search_query.clear();
                                ss.search_matches.clear();
                                ss.search_highlights.clear();
                                ss.search_current = 0;
                                true
                            }
                            (KeyCode::Char('n'), KeyModifiers::NONE) => ss.navigate_search(true),
                            (KeyCode::Char('N'), KeyModifiers::SHIFT) => ss.navigate_search(false),

                            (KeyCode::Right, KeyModifiers::NONE) => {
                                if state.file_list_visible {
                                    state.file_list_visible = false;
                                    let new_width = doc_width(vp_width, false);
                                    ss.vp_width = new_width;
                                    ss.rerender(fonts);
                                    if !ss.search_query.is_empty() && !ss.search_matches.is_empty() {
                                        ss.execute_search(fonts);
                                    }
                                    let mut out = BufWriter::new(stdout.lock());
                                    execute!(out, terminal::Clear(ClearType::All))?;
                                    display_viewport(
                                        &mut out, &ss.img, ss.scroll_y, new_width, vp_height,
                                        &mut ss.frame, doc_col(false), None, None,
                                        &ss.search_highlights, ss.search_current,
                                    )?;
                                    continue;
                                } else {
                                    false
                                }
                            }

                            (KeyCode::Left, KeyModifiers::NONE) => {
                                if let Some(ref path) = state.doc_path {
                                    state.position_cache.insert(path.clone(), SavedPosition {
                                        scroll_y: ss.scroll_y,
                                        current_heading: None,
                                        folded: Vec::new(),
                                    });
                                }
                                state.doc_mode = false;
                                state.file_list_visible = true;
                                let mut out = BufWriter::new(stdout.lock());
                                browser_clear_preview(&mut out)?;
                                draw_file_list(&mut out, &state, term_rows)?;
                                show_preview(&mut out, &mut state, fonts, preview_width, vp_height, theme, layout)?;
                                continue;
                            }

                            (KeyCode::Char(' '), KeyModifiers::NONE) => {
                                ss.scroll(layout.scroll_step as i32)
                            }
                            (KeyCode::Char(' '), KeyModifiers::CONTROL) => {
                                ss.scroll(-(layout.scroll_step as i32))
                            }
                            (KeyCode::Char('j'), _) => ss.scroll(layout.scroll_step as i32),
                            (KeyCode::Char('k'), _) => ss.scroll(-(layout.scroll_step as i32)),
                            (KeyCode::Down, _) => ss.scroll(layout.scroll_step as i32),
                            (KeyCode::Up, _) => ss.scroll(-(layout.scroll_step as i32)),
                            (KeyCode::PageDown, _) => ss.scroll(vp_height as i32 / 2),
                            (KeyCode::PageUp, _) => ss.scroll(-(vp_height as i32 / 2)),
                            (KeyCode::Home, _) => {
                                let changed = ss.scroll_y != 0;
                                ss.scroll_y = 0;
                                changed
                            }
                            (KeyCode::End, _) => {
                                let max = ss.max_scroll();
                                let changed = ss.scroll_y != max;
                                ss.scroll_y = max;
                                changed
                            }
                            (KeyCode::Char('h'), KeyModifiers::NONE) => {
                                let help_img =
                                    render_help_overlay(cur_width, vp_height, fonts);
                                let mut out = BufWriter::new(stdout.lock());
                                display_viewport(
                                    &mut out, &help_img, 0, cur_width, vp_height,
                                    &mut ss.frame, col, None, None, &[], 0,
                                )?;
                                loop {
                                    if let Event::Key(KeyEvent {
                                        kind: KeyEventKind::Press,
                                        ..
                                    }) = event::read()?
                                    {
                                        break;
                                    }
                                }
                                true
                            }
                            (KeyCode::Char('e'), KeyModifiers::NONE) => {
                                if state.doc_path.is_some() {
                                    open_in_editor = true;
                                    continue;
                                }
                                false
                            }
                            _ => false,
                        }
                    };

                    if needs_redraw {
                        let overlay = if ss.search_mode {
                            Some(render_search_bar(
                                &ss.search_query,
                                None,
                                cur_width,
                                fonts,
                            ))
                        } else if !ss.search_matches.is_empty() {
                            Some(render_search_bar(
                                &ss.search_query,
                                Some((ss.search_current + 1, ss.search_matches.len())),
                                cur_width,
                                fonts,
                            ))
                        } else {
                            None
                        };
                        let mut out = BufWriter::new(stdout.lock());
                        display_viewport(
                            &mut out, &ss.img, ss.scroll_y, cur_width, vp_height,
                            &mut ss.frame, col, overlay.as_ref(), None,
                            &ss.search_highlights, ss.search_current,
                        )?;
                    }
                    } // end Source
                    None => {}
                }
            } else {
                let needs_redraw = match (code, modifiers) {
                    (KeyCode::Char('q'), _)
                    | (KeyCode::Esc, _)
                    | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,
                    (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                        if state.cursor + 1 < state.entries.len() {
                            state.cursor += 1;
                        }
                        true
                    }
                    (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                        if state.cursor > 0 {
                            state.cursor -= 1;
                        }
                        true
                    }
                    (KeyCode::Right, _) | (KeyCode::Enter, _) => {
                        match state.entries.get(state.cursor).cloned() {
                            Some(BrowserEntry::Dir(name)) => {
                                let new_dir = state.current_dir.join(&name);
                                state.entries = scan_directory(&new_dir, &state.extra_extensions);
                                state.current_dir = new_dir.canonicalize().unwrap_or(new_dir);
                                state.cursor = 0;
                                state.doc_state = None;
                                state.preview_cache.clear();
                                state.position_cache.clear();
                                let mut out = BufWriter::new(stdout.lock());
                                browser_clear_preview(&mut out)?;
                            }
                            Some(BrowserEntry::File(name)) => {
                                let file_path = state.current_dir.join(&name);
                                if let Ok(source) = std::fs::read_to_string(&file_path) {
                                    state.file_list_visible = false;
                                    let w = doc_width(vp_width, false);

                                    if is_markdown(&name) {
                                        let mut ds =
                                            AppState::new(&source, fonts, w, vp_height, *theme, *layout);
                                        if let Some(saved) = state.position_cache.get(&file_path) {
                                            ds.scroll_y = saved.scroll_y;
                                            ds.current_heading = saved.current_heading;
                                            for (i, &folded) in saved.folded.iter().enumerate() {
                                                if i < ds.headings.len() {
                                                    ds.headings[i].folded = folded;
                                                }
                                            }
                                            ds.rerender(fonts);
                                        }
                                        let mut out = BufWriter::new(stdout.lock());
                                        execute!(out, terminal::Clear(ClearType::All))?;
                                        let ci = ds.cursor_info();
                                        display_viewport(
                                            &mut out, &ds.img, ds.scroll_y, w, vp_height,
                                            &mut ds.frame, doc_col(false), None, ci,
                                            &ds.search_highlights, ds.search_current,
                                        )?;
                                        state.doc_state = Some(DocContent::Markdown(ds));
                                    } else {
                                        let ext = file_extension(&name);
                                        let mut ss = SourceViewState::new(
                                            &source, &ext, fonts, w, vp_height, *theme, *layout,
                                        );
                                        if let Some(saved) = state.position_cache.get(&file_path) {
                                            ss.scroll_y = saved.scroll_y.min(ss.max_scroll());
                                        }
                                        let mut out = BufWriter::new(stdout.lock());
                                        execute!(out, terminal::Clear(ClearType::All))?;
                                        display_viewport(
                                            &mut out, &ss.img, ss.scroll_y, w, vp_height,
                                            &mut ss.frame, doc_col(false), None, None,
                                            &ss.search_highlights, ss.search_current,
                                        )?;
                                        state.doc_state = Some(DocContent::Source(ss));
                                    }
                                    state.doc_mode = true;
                                    state.doc_path = Some(file_path);
                                }
                                continue;
                            }
                            None => {}
                        }
                        true
                    }
                    (KeyCode::Left, _) => {
                        if let Some(parent) = state.current_dir.parent() {
                            let parent = parent.to_path_buf();
                            state.entries = scan_directory(&parent, &state.extra_extensions);
                            state.current_dir = parent.canonicalize().unwrap_or(parent);
                            state.cursor = 0;
                            state.doc_state = None;
                            state.preview_cache.clear();
                            state.position_cache.clear();
                            let mut out = BufWriter::new(stdout.lock());
                            browser_clear_preview(&mut out)?;
                        }
                        true
                    }
                    (KeyCode::Char('e'), _) => {
                        if let Some(BrowserEntry::File(name)) = state.entries.get(state.cursor).cloned() {
                            state.doc_path = Some(state.current_dir.join(&name));
                            open_in_editor = true;
                            continue;
                        }
                        false
                    }
                    (KeyCode::Char('h'), _) => {
                        let help_img = render_help_overlay_with(
                            preview_width,
                            vp_height,
                            fonts,
                            "Browser Keybindings",
                            BROWSER_KEYBINDINGS,
                        );
                        let mut out = BufWriter::new(stdout.lock());
                        display_viewport(
                            &mut out,
                            &help_img,
                            0,
                            preview_width,
                            vp_height,
                            &mut state.preview_frame,
                            Some(BROWSER_LEFT_COLS + 1),
                            None,
                            None,
                            &[],
                            0,
                        )?;
                        loop {
                            if let Event::Key(KeyEvent {
                                kind: KeyEventKind::Press,
                                ..
                            }) = event::read()?
                            {
                                break;
                            }
                        }
                        true
                    }
                    _ => false,
                };
                if needs_redraw {
                    let mut out = BufWriter::new(stdout.lock());
                    draw_file_list(&mut out, &state, term_rows)?;
                    show_preview(&mut out, &mut state, fonts, preview_width, vp_height, theme, layout)?;
                }
            }
        }
    }

    {
        let mut out = BufWriter::new(stdout.lock());
        write!(
            out,
            "\x1b_Ga=d,d=I,i=1,q=2\x1b\\\x1b_Ga=d,d=I,i=2,q=2\x1b\\"
        )?;
        execute!(out, cursor::Show, terminal::LeaveAlternateScreen)?;
    }
    terminal::disable_raw_mode()?;
    Ok(())
}
