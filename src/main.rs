mod browser;
mod constants;
mod fonts;
mod headings;
mod kitty;
mod overlay;
mod parsing;
mod render;
mod state;
mod text;
mod theme;
mod types;

#[cfg(test)]
mod test_helpers;

use crossterm::{
    cursor, execute,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    terminal::{self, ClearType},
};
use std::io::{self, BufWriter, Write};
use std::path::Path;

use constants::*;
use fonts::load_fonts;
use kitty::{display_viewport, get_viewport_pixel_size, kitty_display_raw};
use overlay::{render_help_overlay, render_search_bar};
use state::AppState;

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" {
        eprintln!("dmbdip - Display Markdown But Do it Pretty");
        eprintln!();
        eprintln!("Usage: dmbdip <markdown-file-or-directory>");
        eprintln!();
        eprintln!("Renders a markdown file as an image and displays it in the terminal");
        eprintln!("using the Kitty graphics protocol. When given a directory, opens a");
        eprintln!("file browser showing markdown files and subfolders.");
        eprintln!();
        eprintln!("Keybindings:");
        for &(key, desc) in KEYBINDINGS {
            eprintln!("  {:<20} {}", key, desc);
        }
        std::process::exit(if args.len() < 2 { 1 } else { 0 });
    }

    let file_path = &args[1];
    let path = Path::new(file_path);

    let fonts = load_fonts();
    let (vp_width, vp_height) = get_viewport_pixel_size()?;

    if path.is_dir() {
        return browser::run_browser(path, &fonts, vp_width, vp_height);
    }

    let source = std::fs::read_to_string(file_path)
        .unwrap_or_else(|e| panic!("Cannot read {}: {}", file_path, e));

    eprintln!("Rendering markdown...");
    let mut state = AppState::new(&source, &fonts, vp_width, vp_height);
    eprintln!(
        "Viewport: {}x{} px, Image: {}x{} px, {} headings",
        vp_width,
        vp_height,
        state.img.width(),
        state.img.height(),
        state.headings.len()
    );

    let stdout = io::stdout();

    terminal::enable_raw_mode()?;
    {
        let mut out = BufWriter::new(stdout.lock());
        execute!(
            out,
            terminal::EnterAlternateScreen,
            cursor::Hide,
            terminal::Clear(ClearType::All)
        )?;
        let ci = state.cursor_info();
        display_viewport(
            &mut out,
            &state.img,
            state.scroll_y,
            vp_width,
            vp_height,
            &mut state.frame,
            None,
            None,
            ci,
            &state.search_highlights,
            state.search_current,
        )?;
    }

    loop {
        if let Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            ..
        }) = event::read()?
        {
            let needs_redraw = if state.search_mode {
                match (code, modifiers) {
                    (KeyCode::Esc, _) => {
                        state.search_mode = false;
                        state.search_query.clear();
                        state.search_matches.clear();
                        state.search_highlights.clear();
                        true
                    }
                    (KeyCode::Enter, _) => {
                        state.search_mode = false;
                        state.execute_search(&fonts);
                        true
                    }
                    (KeyCode::Backspace, _) => {
                        state.search_query.pop();
                        true
                    }
                    (KeyCode::Char(c), KeyModifiers::NONE | KeyModifiers::SHIFT) => {
                        state.search_query.push(c);
                        true
                    }
                    _ => false,
                }
            } else {
                match (code, modifiers) {
                    (KeyCode::Char('q'), _)
                    | (KeyCode::Esc, _)
                    | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,

                    (KeyCode::Char('h'), KeyModifiers::NONE) => {
                        let help_img = render_help_overlay(vp_width, vp_height, &fonts);
                        let mut out = BufWriter::new(stdout.lock());
                        let raw = help_img.as_raw();
                        let (new_id, old_id) = state.next_frame();
                        kitty_display_raw(&mut out, raw, vp_width, vp_height, new_id, old_id)?;
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

                    (KeyCode::Char('/'), KeyModifiers::NONE) => {
                        state.search_mode = true;
                        state.search_query.clear();
                        state.search_matches.clear();
                        state.search_highlights.clear();
                        state.search_current = 0;
                        true
                    }

                    (KeyCode::Char('n'), KeyModifiers::NONE) => state.navigate_search(true),
                    (KeyCode::Char('N'), KeyModifiers::SHIFT) => state.navigate_search(false),

                    (KeyCode::Down, KeyModifiers::NONE) => state.navigate_heading(1),
                    (KeyCode::Up, KeyModifiers::NONE) => state.navigate_heading(-1),

                    (KeyCode::Left, KeyModifiers::NONE)
                    | (KeyCode::Right, KeyModifiers::NONE)
                    | (KeyCode::Tab, _) => state.toggle_fold(&fonts),

                    (KeyCode::Char(' '), KeyModifiers::NONE) => {
                        state.scroll(SCROLL_STEP as i32)
                    }
                    (KeyCode::Char(' '), KeyModifiers::CONTROL) => state.scroll(-(SCROLL_STEP as i32)),

                    (KeyCode::Char('j'), _) => state.scroll(SCROLL_STEP as i32),
                    (KeyCode::Char('k'), _) => state.scroll(-(SCROLL_STEP as i32)),

                    (KeyCode::PageDown, _) => state.scroll(vp_height as i32 / 2),
                    (KeyCode::PageUp, _) => state.scroll(-(vp_height as i32 / 2)),

                    (KeyCode::Home, _) => {
                        let changed = state.scroll_y != 0;
                        state.scroll_y = 0;
                        state.sync_cursor_to_scroll();
                        changed
                    }
                    (KeyCode::End, _) => {
                        let max = state.max_scroll();
                        let changed = state.scroll_y != max;
                        state.scroll_y = max;
                        state.sync_cursor_to_scroll();
                        changed
                    }

                    _ => false,
                }
            };

            if needs_redraw {
                let overlay = if state.search_mode {
                    Some(render_search_bar(
                        &state.search_query,
                        None,
                        vp_width,
                        &fonts,
                    ))
                } else if !state.search_matches.is_empty() {
                    Some(render_search_bar(
                        &state.search_query,
                        Some((state.search_current + 1, state.search_matches.len())),
                        vp_width,
                        &fonts,
                    ))
                } else {
                    None
                };

                let ci = state.cursor_info();
                let hl = &state.search_highlights;
                let sc = state.search_current;
                let mut out = BufWriter::new(stdout.lock());
                display_viewport(
                    &mut out,
                    &state.img,
                    state.scroll_y,
                    vp_width,
                    vp_height,
                    &mut state.frame,
                    None,
                    overlay.as_ref(),
                    ci,
                    hl,
                    sc,
                )?;
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
