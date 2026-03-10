use ab_glyph::{FontVec, PxScale};
use base64::Engine;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, ClearType},
};
use image::{Rgb, RgbImage};
use imageproc::drawing::{draw_filled_rect_mut, draw_line_segment_mut, draw_text_mut, text_size};
use imageproc::rect::Rect;
use pulldown_cmark::{Event as MdEvent, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::io::{self, BufWriter, Write};

const SCROLL_STEP: u32 = 40;
const MARGIN_LEFT: u32 = 20;
const MARGIN_RIGHT: u32 = 20;
const PARAGRAPH_GAP: u32 = 16;

// --- Theme ---

struct Theme {
    bg: Rgb<u8>,
    body_color: Rgb<u8>,
    body_size: f32,
    h1_color: Rgb<u8>,
    h1_size: f32,
    h2_color: Rgb<u8>,
    h2_size: f32,
    h3_color: Rgb<u8>,
    h3_size: f32,
    table_border: Rgb<u8>,
    table_header_bg: Rgb<u8>,
}

fn default_theme() -> Theme {
    Theme {
        bg: Rgb([30, 30, 40]),
        body_color: Rgb([220, 220, 220]),
        body_size: 18.0,
        h1_color: Rgb([100, 160, 255]),
        h1_size: 36.0,
        h2_color: Rgb([80, 200, 200]),
        h2_size: 28.0,
        h3_color: Rgb([120, 220, 120]),
        h3_size: 22.0,
        table_border: Rgb([100, 100, 120]),
        table_header_bg: Rgb([50, 50, 65]),
    }
}

// --- Markdown blocks ---

enum Block {
    Heading { level: HeadingLevel, text: String },
    Paragraph { text: String },
    Table { headers: Vec<String>, rows: Vec<Vec<String>> },
}

fn parse_markdown(source: &str) -> Vec<Block> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(source, options);
    let mut blocks = Vec::new();
    let mut current_text = String::new();
    let mut in_heading: Option<HeadingLevel> = None;
    let mut in_paragraph = false;
    let mut in_table = false;
    let mut table_headers: Vec<String> = Vec::new();
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut in_table_head = false;
    let mut cell_text = String::new();

    for event in parser {
        match event {
            MdEvent::Start(Tag::Heading { level, .. }) => {
                in_heading = Some(level);
                current_text.clear();
            }
            MdEvent::End(TagEnd::Heading(_)) => {
                if let Some(level) = in_heading.take() {
                    blocks.push(Block::Heading {
                        level,
                        text: current_text.trim().to_string(),
                    });
                    current_text.clear();
                }
            }
            MdEvent::Start(Tag::Paragraph) => {
                in_paragraph = true;
                current_text.clear();
            }
            MdEvent::End(TagEnd::Paragraph) => {
                if in_paragraph {
                    in_paragraph = false;
                    let text = current_text.trim().to_string();
                    if !text.is_empty() {
                        blocks.push(Block::Paragraph { text });
                    }
                    current_text.clear();
                }
            }
            MdEvent::Start(Tag::Table(_)) => {
                in_table = true;
                table_headers.clear();
                table_rows.clear();
            }
            MdEvent::End(TagEnd::Table) => {
                in_table = false;
                blocks.push(Block::Table {
                    headers: table_headers.clone(),
                    rows: table_rows.clone(),
                });
            }
            MdEvent::Start(Tag::TableHead) => {
                in_table_head = true;
                current_row.clear();
            }
            MdEvent::End(TagEnd::TableHead) => {
                in_table_head = false;
                table_headers = current_row.clone();
                current_row.clear();
            }
            MdEvent::Start(Tag::TableRow) => {
                current_row.clear();
            }
            MdEvent::End(TagEnd::TableRow) => {
                if !in_table_head {
                    table_rows.push(current_row.clone());
                }
                current_row.clear();
            }
            MdEvent::Start(Tag::TableCell) => {
                cell_text.clear();
            }
            MdEvent::End(TagEnd::TableCell) => {
                current_row.push(cell_text.trim().to_string());
                cell_text.clear();
            }
            MdEvent::Text(t) => {
                if in_table {
                    cell_text.push_str(&t);
                } else {
                    current_text.push_str(&t);
                }
            }
            MdEvent::SoftBreak | MdEvent::HardBreak => {
                if in_table {
                    cell_text.push(' ');
                } else {
                    current_text.push(' ');
                }
            }
            MdEvent::Code(code) => {
                if in_table {
                    cell_text.push_str(&code);
                } else {
                    current_text.push_str(&code);
                }
            }
            _ => {}
        }
    }

    blocks
}

// --- Word wrapping ---

fn wrap_text(text: &str, font: &FontVec, scale: PxScale, max_width: u32) -> Vec<String> {
    let mut lines = Vec::new();
    let space_w = text_size(scale, font, " ").0;

    for paragraph_line in text.lines() {
        let words: Vec<&str> = paragraph_line.split_whitespace().collect();
        if words.is_empty() {
            lines.push(String::new());
            continue;
        }

        let mut current_line = String::new();
        let mut current_width: u32 = 0;

        for word in words {
            let word_w = text_size(scale, font, word).0;

            if current_line.is_empty() {
                current_line = word.to_string();
                current_width = word_w;
            } else if current_width + space_w + word_w <= max_width {
                current_line.push(' ');
                current_line.push_str(word);
                current_width += space_w + word_w;
            } else {
                lines.push(current_line);
                current_line = word.to_string();
                current_width = word_w;
            }
        }
        if !current_line.is_empty() {
            lines.push(current_line);
        }
    }

    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

// --- Layout & render ---

fn render_markdown(source: &str, width: u32, font: &FontVec) -> RgbImage {
    let theme = default_theme();
    let blocks = parse_markdown(source);
    let content_width = width - MARGIN_LEFT - MARGIN_RIGHT;

    // First pass: compute total height
    let total_height = compute_total_height(&blocks, font, &theme, content_width);

    let mut img = RgbImage::from_pixel(width, total_height.max(1), theme.bg);
    let mut y: u32 = PARAGRAPH_GAP;

    for block in &blocks {
        match block {
            Block::Heading { level, text } => {
                let (size, color) = heading_style(level, &theme);
                let scale = PxScale::from(size);
                let lines = wrap_text(text, font, scale, content_width);
                let line_height = (size * 1.3) as u32;

                for line in &lines {
                    draw_text_mut(
                        &mut img,
                        color,
                        MARGIN_LEFT as i32,
                        y as i32,
                        scale,
                        font,
                        line,
                    );
                    y += line_height;
                }
                y += PARAGRAPH_GAP;
            }
            Block::Paragraph { text } => {
                let scale = PxScale::from(theme.body_size);
                let lines = wrap_text(text, font, scale, content_width);
                let line_height = (theme.body_size * 1.4) as u32;

                for line in &lines {
                    draw_text_mut(
                        &mut img,
                        theme.body_color,
                        MARGIN_LEFT as i32,
                        y as i32,
                        scale,
                        font,
                        line,
                    );
                    y += line_height;
                }
                y += PARAGRAPH_GAP;
            }
            Block::Table { headers, rows } => {
                y = render_table(&mut img, headers, rows, font, &theme, y, content_width);
                y += PARAGRAPH_GAP;
            }
        }
    }

    img
}

fn heading_style(level: &HeadingLevel, theme: &Theme) -> (f32, Rgb<u8>) {
    match level {
        HeadingLevel::H1 => (theme.h1_size, theme.h1_color),
        HeadingLevel::H2 => (theme.h2_size, theme.h2_color),
        _ => (theme.h3_size, theme.h3_color),
    }
}

fn compute_total_height(
    blocks: &[Block],
    font: &FontVec,
    theme: &Theme,
    content_width: u32,
) -> u32 {
    let mut h: u32 = PARAGRAPH_GAP;

    for block in blocks {
        match block {
            Block::Heading { level, text } => {
                let (size, _) = heading_style(level, theme);
                let scale = PxScale::from(size);
                let lines = wrap_text(text, font, scale, content_width);
                let line_height = (size * 1.3) as u32;
                h += lines.len() as u32 * line_height + PARAGRAPH_GAP;
            }
            Block::Paragraph { text } => {
                let scale = PxScale::from(theme.body_size);
                let lines = wrap_text(text, font, scale, content_width);
                let line_height = (theme.body_size * 1.4) as u32;
                h += lines.len() as u32 * line_height + PARAGRAPH_GAP;
            }
            Block::Table { headers: _, rows } => {
                let row_height = (theme.body_size * 1.6) as u32;
                let num_rows = 1 + rows.len() as u32; // header + data rows
                h += num_rows * row_height + PARAGRAPH_GAP + 2; // +2 for borders
            }
        }
    }

    h + PARAGRAPH_GAP
}

fn render_table(
    img: &mut RgbImage,
    headers: &[String],
    rows: &[Vec<String>],
    font: &FontVec,
    theme: &Theme,
    start_y: u32,
    content_width: u32,
) -> u32 {
    let ncols = headers.len().max(1);
    let col_width = content_width / ncols as u32;
    let row_height = (theme.body_size * 1.6) as u32;
    let scale = PxScale::from(theme.body_size);
    let cell_padding: u32 = 6;
    let table_width = col_width * ncols as u32;

    let mut y = start_y;

    // Header background
    draw_filled_rect_mut(
        img,
        Rect::at(MARGIN_LEFT as i32, y as i32).of_size(table_width, row_height),
        theme.table_header_bg,
    );

    // Header text
    for (ci, header) in headers.iter().enumerate() {
        let x = MARGIN_LEFT + ci as u32 * col_width + cell_padding;
        let text_y = y + (row_height.saturating_sub(theme.body_size as u32)) / 2;
        draw_text_mut(
            img,
            theme.h2_color,
            x as i32,
            text_y as i32,
            scale,
            font,
            header,
        );
    }

    // Horizontal line under header
    let line_y = (y + row_height) as f32;
    draw_line_segment_mut(
        img,
        (MARGIN_LEFT as f32, line_y),
        ((MARGIN_LEFT + table_width) as f32, line_y),
        theme.table_border,
    );

    y += row_height;

    // Data rows
    for row in rows {
        for (ci, cell) in row.iter().enumerate() {
            let x = MARGIN_LEFT + ci as u32 * col_width + cell_padding;
            let text_y = y + (row_height.saturating_sub(theme.body_size as u32)) / 2;
            draw_text_mut(
                img,
                theme.body_color,
                x as i32,
                text_y as i32,
                scale,
                font,
                cell,
            );
        }

        // Row separator
        let line_y = (y + row_height) as f32;
        draw_line_segment_mut(
            img,
            (MARGIN_LEFT as f32, line_y),
            ((MARGIN_LEFT + table_width) as f32, line_y),
            theme.table_border,
        );

        y += row_height;
    }

    // Vertical column separators
    for ci in 0..=ncols {
        let x = (MARGIN_LEFT + ci as u32 * col_width) as f32;
        draw_line_segment_mut(
            img,
            (x, start_y as f32),
            (x, y as f32),
            theme.table_border,
        );
    }

    y
}

// --- Kitty protocol ---

/// Double-buffer: display on new_id, then delete old_id, so there's no blank frame.
fn kitty_display_raw(
    w: &mut impl Write,
    data: &[u8],
    width: u32,
    height: u32,
    new_id: u32,
    old_id: u32,
) -> io::Result<()> {
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);

    let chunk_size = 4096;
    let bytes = b64.as_bytes();
    let total_chunks = (bytes.len() + chunk_size - 1) / chunk_size;

    // Move cursor home
    write!(w, "\x1b[H")?;

    // Transmit + display new image
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

    // Delete old image after new one is displayed
    write!(w, "\x1b_Ga=d,d=I,i={old_id},q=2\x1b\\")?;

    w.flush()
}

fn redraw(
    w: &mut impl Write,
    img: &RgbImage,
    scroll_y: u32,
    vp_width: u32,
    vp_height: u32,
    frame: &mut u32,
) -> io::Result<()> {
    let src_w = vp_width.min(img.width());
    let src_h = vp_height.min(img.height().saturating_sub(scroll_y));
    let stride = img.width() as usize * 3;

    let raw = img.as_raw();
    let row_start = scroll_y as usize * stride;
    let mut viewport_data = Vec::with_capacity(src_w as usize * src_h as usize * 3);
    for row in 0..src_h as usize {
        let offset = row_start + row * stride;
        viewport_data.extend_from_slice(&raw[offset..offset + src_w as usize * 3]);
    }

    let new_id = *frame;
    let old_id = if *frame == 1 { 2 } else { 1 };
    *frame = old_id; // alternate for next call

    kitty_display_raw(w, &viewport_data, src_w, src_h, new_id, old_id)
}

// --- Viewport ---

fn get_viewport_pixel_size() -> io::Result<(u32, u32)> {
    let size = terminal::window_size()?;
    if size.width > 0 && size.height > 0 {
        Ok((size.width as u32, size.height as u32))
    } else {
        let (cols, rows) = terminal::size()?;
        Ok((cols as u32 * 8, rows as u32 * 16))
    }
}

// --- Main ---

fn load_font() -> FontVec {
    let font_paths = [
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/dejavu-sans-fonts/DejaVuSans.ttf",
    ];

    for path in &font_paths {
        if let Ok(data) = std::fs::read(path) {
            if let Ok(font) = FontVec::try_from_vec(data) {
                return font;
            }
        }
    }

    panic!("Could not find DejaVu Sans font. Install dejavu-fonts.");
}

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let file_path = args.get(1).expect("Usage: mdbdp <markdown-file>");

    let source = std::fs::read_to_string(file_path)
        .unwrap_or_else(|e| panic!("Cannot read {}: {}", file_path, e));

    let font = load_font();
    let (vp_width, vp_height) = get_viewport_pixel_size()?;

    eprintln!("Rendering markdown...");
    let img = render_markdown(&source, vp_width, &font);
    let img_height = img.height();
    eprintln!(
        "Viewport: {}x{} px, Image: {}x{} px",
        vp_width,
        vp_height,
        img.width(),
        img_height
    );

    let max_scroll = img_height.saturating_sub(vp_height);
    let mut scroll_y: u32 = 0;
    let mut frame: u32 = 1;
    let stdout = io::stdout();

    terminal::enable_raw_mode()?;
    {
        let mut out = BufWriter::new(stdout.lock());
        execute!(out, terminal::EnterAlternateScreen, cursor::Hide, terminal::Clear(ClearType::All))?;
        redraw(&mut out, &img, scroll_y, vp_width, vp_height, &mut frame)?;
    }

    loop {
        if let Event::Key(KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            ..
        }) = event::read()?
        {
            let new_scroll = match (code, modifiers) {
                (KeyCode::Char('q'), _)
                | (KeyCode::Esc, _)
                | (KeyCode::Char('c'), KeyModifiers::CONTROL) => break,

                (KeyCode::Down | KeyCode::Char('j'), _) => {
                    Some((scroll_y + SCROLL_STEP).min(max_scroll))
                }
                (KeyCode::Up | KeyCode::Char('k'), _) => {
                    Some(scroll_y.saturating_sub(SCROLL_STEP))
                }
                (KeyCode::PageDown | KeyCode::Char(' '), _) => {
                    Some((scroll_y + vp_height / 2).min(max_scroll))
                }
                (KeyCode::PageUp, _) => Some(scroll_y.saturating_sub(vp_height / 2)),
                (KeyCode::Home, _) => Some(0),
                (KeyCode::End, _) => Some(max_scroll),
                _ => None,
            };

            if let Some(new_y) = new_scroll {
                if new_y != scroll_y {
                    scroll_y = new_y;
                    let mut out = BufWriter::new(stdout.lock());
                    redraw(&mut out, &img, scroll_y, vp_width, vp_height, &mut frame)?;
                }
            }
        }
    }

    {
        let mut out = BufWriter::new(stdout.lock());
        // Delete both image IDs
        write!(out, "\x1b_Ga=d,d=I,i=1,q=2\x1b\\\x1b_Ga=d,d=I,i=2,q=2\x1b\\")?;
        execute!(
            out,
            cursor::Show,
            terminal::LeaveAlternateScreen
        )?;
    }
    terminal::disable_raw_mode()?;

    Ok(())
}
