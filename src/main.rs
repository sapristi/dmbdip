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
const CURSOR_WIDTH: u32 = 4;
const CURSOR_MARGIN: u32 = 6; // gap between cursor and text
const MAX_CONTENT_WIDTH: u32 = 900;

// --- Theme ---

struct Theme {
    bg: Rgb<u8>,
    body_color: Rgb<u8>,
    body_size: f32,
    code_color: Rgb<u8>,
    code_bg: Rgb<u8>,
    cursor_color: Rgb<u8>,
    h1_color: Rgb<u8>,
    h1_size: f32,
    h2_color: Rgb<u8>,
    h2_size: f32,
    h3_color: Rgb<u8>,
    h3_size: f32,
    meta_key_color: Rgb<u8>,
    meta_val_color: Rgb<u8>,
    table_border: Rgb<u8>,
    table_header_bg: Rgb<u8>,
}

fn default_theme() -> Theme {
    Theme {
        bg: Rgb([30, 30, 40]),
        body_color: Rgb([220, 220, 220]),
        body_size: 18.0,
        code_color: Rgb([230, 180, 80]),
        code_bg: Rgb([45, 45, 58]),
        cursor_color: Rgb([255, 180, 50]),
        h1_color: Rgb([100, 160, 255]),
        h1_size: 36.0,
        h2_color: Rgb([80, 200, 200]),
        h2_size: 28.0,
        h3_color: Rgb([120, 220, 120]),
        h3_size: 22.0,
        meta_key_color: Rgb([180, 140, 255]),
        meta_val_color: Rgb([200, 200, 200]),
        table_border: Rgb([100, 100, 120]),
        table_header_bg: Rgb([50, 50, 65]),
    }
}

// --- Fonts ---

struct Fonts {
    regular: FontVec,
    bold: FontVec,
    italic: FontVec,
    mono: FontVec,
}

fn load_fonts() -> Fonts {
    let load = |paths: &[&str], name: &str| -> FontVec {
        for path in paths {
            if let Ok(data) = std::fs::read(path) {
                if let Ok(font) = FontVec::try_from_vec(data) {
                    return font;
                }
            }
        }
        panic!("Could not find {} font.", name);
    };

    Fonts {
        regular: load(
            &[
                "/usr/share/fonts/TTF/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            ],
            "DejaVu Sans",
        ),
        bold: load(
            &[
                "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            ],
            "DejaVu Sans Bold",
        ),
        italic: load(
            &[
                "/usr/share/fonts/TTF/DejaVuSans-Oblique.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Oblique.ttf",
            ],
            "DejaVu Sans Oblique",
        ),
        mono: load(
            &[
                "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            ],
            "DejaVu Sans Mono",
        ),
    }
}

// --- Inline spans ---

#[derive(Clone, Debug, PartialEq)]
enum SpanStyle {
    Normal,
    Bold,
    Italic,
    Code,
}

#[derive(Clone, Debug)]
struct Span {
    text: String,
    style: SpanStyle,
}

impl Span {
    fn font<'a>(&self, fonts: &'a Fonts) -> &'a FontVec {
        match self.style {
            SpanStyle::Normal => &fonts.regular,
            SpanStyle::Bold => &fonts.bold,
            SpanStyle::Italic => &fonts.italic,
            SpanStyle::Code => &fonts.mono,
        }
    }

    fn color(&self, theme: &Theme) -> Rgb<u8> {
        match self.style {
            SpanStyle::Code => theme.code_color,
            _ => theme.body_color,
        }
    }
}

// --- Markdown blocks ---

#[derive(Clone)]
enum Block {
    Heading {
        level: HeadingLevel,
        spans: Vec<Span>,
    },
    Paragraph {
        spans: Vec<Span>,
    },
    CodeBlock {
        text: String,
    },
    Table {
        headers: Vec<Vec<Span>>,
        rows: Vec<Vec<Vec<Span>>>,
    },
    Metadata {
        entries: Vec<(String, String)>,
    },
}

fn parse_metadata(source: &str) -> (Vec<(String, String)>, &str) {
    let trimmed = source.trim_start();
    if !trimmed.starts_with("---") {
        return (Vec::new(), source);
    }

    let after_first = &trimmed[3..];
    if let Some(end) = after_first.find("\n---") {
        let meta_block = &after_first[..end];
        let rest = &after_first[end + 4..];
        let rest = rest.strip_prefix('\n').unwrap_or(rest);

        let entries: Vec<(String, String)> = meta_block
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() {
                    return None;
                }
                let (key, val) = line.split_once(':')?;
                Some((key.trim().to_string(), val.trim().to_string()))
            })
            .collect();

        (entries, rest)
    } else {
        (Vec::new(), source)
    }
}

fn parse_markdown(source: &str) -> Vec<Block> {
    let (metadata, source) = parse_metadata(source);

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(source, options);
    let mut blocks = Vec::new();

    if !metadata.is_empty() {
        blocks.push(Block::Metadata { entries: metadata });
    }

    let mut spans: Vec<Span> = Vec::new();
    let mut style_stack: Vec<SpanStyle> = vec![SpanStyle::Normal];
    let mut in_heading: Option<HeadingLevel> = None;
    let mut in_paragraph = false;

    let mut in_table = false;
    let mut table_headers: Vec<Vec<Span>> = Vec::new();
    let mut table_rows: Vec<Vec<Vec<Span>>> = Vec::new();
    let mut current_row: Vec<Vec<Span>> = Vec::new();
    let mut in_table_head = false;
    let mut cell_spans: Vec<Span> = Vec::new();

    let mut in_code_block = false;
    let mut code_text = String::new();

    let current_style = |stack: &[SpanStyle]| stack.last().cloned().unwrap_or(SpanStyle::Normal);

    for event in parser {
        match event {
            MdEvent::Start(Tag::Heading { level, .. }) => {
                in_heading = Some(level);
                spans.clear();
            }
            MdEvent::End(TagEnd::Heading(_)) => {
                if let Some(level) = in_heading.take() {
                    blocks.push(Block::Heading {
                        level,
                        spans: std::mem::take(&mut spans),
                    });
                }
            }
            MdEvent::Start(Tag::Paragraph) => {
                if !in_table {
                    in_paragraph = true;
                    spans.clear();
                }
            }
            MdEvent::End(TagEnd::Paragraph) => {
                if in_paragraph {
                    in_paragraph = false;
                    let s = std::mem::take(&mut spans);
                    if !s.is_empty() {
                        blocks.push(Block::Paragraph { spans: s });
                    }
                }
            }
            MdEvent::Start(Tag::Strong) => style_stack.push(SpanStyle::Bold),
            MdEvent::End(TagEnd::Strong) => {
                style_stack.pop();
            }
            MdEvent::Start(Tag::Emphasis) => style_stack.push(SpanStyle::Italic),
            MdEvent::End(TagEnd::Emphasis) => {
                style_stack.pop();
            }
            MdEvent::Start(Tag::CodeBlock(_)) => {
                in_code_block = true;
                code_text.clear();
            }
            MdEvent::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                blocks.push(Block::CodeBlock {
                    text: std::mem::take(&mut code_text),
                });
            }
            MdEvent::Code(code) => {
                let target = if in_table { &mut cell_spans } else { &mut spans };
                target.push(Span {
                    text: code.to_string(),
                    style: SpanStyle::Code,
                });
            }
            MdEvent::Start(Tag::Table(_)) => {
                in_table = true;
                table_headers.clear();
                table_rows.clear();
            }
            MdEvent::End(TagEnd::Table) => {
                in_table = false;
                blocks.push(Block::Table {
                    headers: std::mem::take(&mut table_headers),
                    rows: std::mem::take(&mut table_rows),
                });
            }
            MdEvent::Start(Tag::TableHead) => {
                in_table_head = true;
                current_row.clear();
            }
            MdEvent::End(TagEnd::TableHead) => {
                in_table_head = false;
                table_headers = std::mem::take(&mut current_row);
            }
            MdEvent::Start(Tag::TableRow) => {
                current_row.clear();
            }
            MdEvent::End(TagEnd::TableRow) => {
                if !in_table_head {
                    table_rows.push(std::mem::take(&mut current_row));
                }
            }
            MdEvent::Start(Tag::TableCell) => {
                cell_spans.clear();
            }
            MdEvent::End(TagEnd::TableCell) => {
                current_row.push(std::mem::take(&mut cell_spans));
            }
            MdEvent::Text(t) => {
                if in_code_block {
                    code_text.push_str(&t);
                } else if in_table {
                    cell_spans.push(Span {
                        text: t.to_string(),
                        style: current_style(&style_stack),
                    });
                } else {
                    spans.push(Span {
                        text: t.to_string(),
                        style: current_style(&style_stack),
                    });
                }
            }
            MdEvent::SoftBreak | MdEvent::HardBreak => {
                let target = if in_table { &mut cell_spans } else { &mut spans };
                target.push(Span {
                    text: " ".to_string(),
                    style: SpanStyle::Normal,
                });
            }
            _ => {}
        }
    }

    blocks
}

// --- Section tree ---

#[derive(Clone)]
struct HeadingInfo {
    block_index: usize,
    level: HeadingLevel,
    number: String,
    folded: bool,
    /// Y position of this heading in the rendered image (set during render)
    y_pos: u32,
    /// Height of the heading line itself
    heading_height: u32,
}

/// Build heading info list and assign hierarchical numbers.
fn build_headings(blocks: &[Block]) -> Vec<HeadingInfo> {
    let mut headings = Vec::new();
    let mut counters = [0u32; 6];

    for (bi, block) in blocks.iter().enumerate() {
        if let Block::Heading { level, .. } = block {
            let idx = heading_level_index(level);
            counters[idx] += 1;
            for c in &mut counters[idx + 1..] {
                *c = 0;
            }
            let parts: Vec<String> = counters[..=idx].iter().map(|c| c.to_string()).collect();
            headings.push(HeadingInfo {
                block_index: bi,
                level: *level,
                number: format!("{}.", parts.join(".")),
                folded: false,
                y_pos: 0,
                heading_height: 0,
            });
        }
    }
    headings
}

/// Check if block at `block_index` is hidden due to a folded heading.
fn is_block_folded(block_index: usize, headings: &[HeadingInfo]) -> bool {
    // Find the heading that owns this block (the last heading before this block)
    // A block is folded if any ancestor heading is folded and the block is inside that section.
    for (hi, heading) in headings.iter().enumerate() {
        if !heading.folded {
            continue;
        }
        // This heading is folded. All blocks after it until the next heading at same or higher level are hidden.
        let fold_level = heading_level_index(&heading.level);
        let start = heading.block_index + 1;

        // Find end of this section
        let end = headings
            .iter()
            .skip(hi + 1)
            .find(|h| heading_level_index(&h.level) <= fold_level)
            .map(|h| h.block_index)
            .unwrap_or(usize::MAX);

        if block_index >= start && block_index < end {
            return true;
        }
    }
    false
}

fn heading_level_index(level: &HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 0,
        HeadingLevel::H2 => 1,
        HeadingLevel::H3 => 2,
        HeadingLevel::H4 => 3,
        HeadingLevel::H5 => 4,
        HeadingLevel::H6 => 5,
    }
}

// --- Span text helpers ---

fn spans_to_plain(spans: &[Span]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

// --- Word wrapping with spans ---

fn split_preserving_indent(text: &str) -> (usize, Vec<&str>) {
    let leading = text.len() - text.trim_start().len();
    let words: Vec<&str> = text.split_whitespace().collect();
    (leading, words)
}

fn wrap_spans(spans: &[Span], fonts: &Fonts, scale: PxScale, max_width: u32) -> Vec<Vec<Span>> {
    let mut lines: Vec<Vec<Span>> = Vec::new();
    let mut current_line: Vec<Span> = Vec::new();
    let mut current_width: u32 = 0;
    let space_w = text_size(scale, &fonts.regular, " ").0;

    for (si, span) in spans.iter().enumerate() {
        let font = span.font(fonts);
        let (leading_spaces, words) = split_preserving_indent(&span.text);

        if words.is_empty() {
            if !current_line.is_empty() {
                current_width += space_w;
                current_line.push(Span {
                    text: " ".to_string(),
                    style: SpanStyle::Normal,
                });
            }
            continue;
        }

        let prev_ends_space = if si > 0 {
            spans[si - 1].text.ends_with(char::is_whitespace)
        } else {
            false
        };
        let need_sep_before_first =
            !current_line.is_empty() && (leading_spaces > 0 || prev_ends_space);

        for (wi, word) in words.iter().enumerate() {
            let word_w = text_size(scale, font, word).0;

            let need_space = if wi == 0 {
                need_sep_before_first
            } else {
                true
            };

            let total = current_width + (if need_space { space_w } else { 0 }) + word_w;
            if current_width > 0 && total > max_width {
                lines.push(std::mem::take(&mut current_line));
                current_width = 0;
                if span.style == SpanStyle::Code && wi == 0 && leading_spaces > 0 {
                    let indent: String = " ".repeat(leading_spaces);
                    let indent_w = text_size(scale, font, &indent).0;
                    current_line.push(Span {
                        text: indent,
                        style: span.style.clone(),
                    });
                    current_width = indent_w;
                }
            } else if need_space && current_width > 0 {
                current_width += space_w;
                if let Some(last) = current_line.last_mut() {
                    if last.style == span.style {
                        last.text.push(' ');
                    } else {
                        current_line.push(Span {
                            text: " ".to_string(),
                            style: SpanStyle::Normal,
                        });
                    }
                }
            }

            if wi == 0 && current_width == 0 && leading_spaces > 0 {
                let indent: String = " ".repeat(leading_spaces);
                let indent_w = text_size(scale, font, &indent).0;
                current_line.push(Span {
                    text: format!("{}{}", indent, word),
                    style: span.style.clone(),
                });
                current_width = indent_w + word_w;
                continue;
            }

            if let Some(last) = current_line.last_mut() {
                if last.style == span.style {
                    last.text.push_str(word);
                } else {
                    current_line.push(Span {
                        text: word.to_string(),
                        style: span.style.clone(),
                    });
                }
            } else {
                current_line.push(Span {
                    text: word.to_string(),
                    style: span.style.clone(),
                });
            }
            current_width += word_w;
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        lines.push(Vec::new());
    }
    lines
}

// --- Drawing helpers ---

fn draw_spans(
    img: &mut RgbImage,
    spans: &[Span],
    x: u32,
    y: u32,
    scale: PxScale,
    fonts: &Fonts,
    theme: &Theme,
) -> u32 {
    let mut cx = x;
    for span in spans {
        let font = span.font(fonts);
        let color = span.color(theme);

        if span.style == SpanStyle::Code {
            let (tw, th) = text_size(scale, font, &span.text);
            let pad = 2;
            draw_filled_rect_mut(
                img,
                Rect::at(cx as i32 - pad as i32, y as i32).of_size(tw + pad * 2, th + pad),
                theme.code_bg,
            );
        }

        draw_text_mut(img, color, cx as i32, y as i32, scale, font, &span.text);
        let w = text_size(scale, span.font(fonts), &span.text).0;
        cx += w;
    }
    cx
}

// --- Layout & render ---

fn render_markdown(
    blocks: &[Block],
    headings: &mut [HeadingInfo],
    current_heading: Option<usize>,
    width: u32,
    fonts: &Fonts,
) -> (RgbImage, Vec<(usize, u32)>) {
    let theme = default_theme();
    let content_width = (width - MARGIN_LEFT - MARGIN_RIGHT).min(MAX_CONTENT_WIDTH);
    let margin_left = (width - content_width) / 2;

    let total_height = compute_total_height(blocks, headings, fonts, &theme, content_width);

    let mut img = RgbImage::from_pixel(width, total_height.max(1), theme.bg);
    let mut y: u32 = PARAGRAPH_GAP;
    let mut heading_idx: usize = 0;
    let mut block_positions: Vec<(usize, u32)> = Vec::new();

    for (bi, block) in blocks.iter().enumerate() {
        if is_block_folded(bi, headings) {
            // Still need to advance heading_idx
            if matches!(block, Block::Heading { .. }) {
                heading_idx += 1;
            }
            continue;
        }

        block_positions.push((bi, y));

        match block {
            Block::Metadata { entries } => {
                y = render_metadata(&mut img, entries, fonts, &theme, y, margin_left);
                y += PARAGRAPH_GAP * 2;
            }
            Block::Heading { level, spans } => {
                let (size, color) = heading_style(level, &theme);
                let scale = PxScale::from(size);

                let hi = heading_idx;
                heading_idx += 1;

                let number = &headings[hi].number;
                let plain = spans_to_plain(spans);

                // Add fold indicator for headings that have content
                let fold_prefix = if headings[hi].folded { "▶ " } else { "▼ " };
                let numbered_text = format!("{}{} {}", fold_prefix, number, plain);

                let lines = wrap_spans(
                    &[Span {
                        text: numbered_text,
                        style: SpanStyle::Bold,
                    }],
                    fonts,
                    scale,
                    content_width,
                );
                let line_height = (size * 1.3) as u32;
                let heading_total_h =
                    lines.len() as u32 * line_height;

                // Record Y position for navigation
                headings[hi].y_pos = y;
                headings[hi].heading_height = heading_total_h;

                // Draw cursor indicator if this is the current heading
                if current_heading == Some(hi) {
                    draw_filled_rect_mut(
                        &mut img,
                        Rect::at((margin_left - CURSOR_MARGIN - CURSOR_WIDTH) as i32, y as i32)
                            .of_size(CURSOR_WIDTH, heading_total_h),
                        theme.cursor_color,
                    );
                }

                for line in &lines {
                    for span in line {
                        draw_text_mut(
                            &mut img,
                            color,
                            margin_left as i32,
                            y as i32,
                            scale,
                            &fonts.bold,
                            &span.text,
                        );
                    }
                    y += line_height;
                }
                y += PARAGRAPH_GAP;
            }
            Block::Paragraph { spans } => {
                let scale = PxScale::from(theme.body_size);
                let lines = wrap_spans(spans, fonts, scale, content_width);
                let line_height = (theme.body_size * 1.4) as u32;

                for line in &lines {
                    draw_spans(&mut img, line, margin_left, y, scale, fonts, &theme);
                    y += line_height;
                }
                y += PARAGRAPH_GAP;
            }
            Block::CodeBlock { text } => {
                y = render_code_block(&mut img, text, fonts, &theme, y, content_width, margin_left);
                y += PARAGRAPH_GAP;
            }
            Block::Table { headers, rows } => {
                y = render_table(&mut img, headers, rows, fonts, &theme, y, content_width, margin_left);
                y += PARAGRAPH_GAP * 2;
            }
        }
    }

    (img, block_positions)
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
    headings: &[HeadingInfo],
    fonts: &Fonts,
    theme: &Theme,
    content_width: u32,
) -> u32 {
    let mut h: u32 = PARAGRAPH_GAP;
    let mut heading_idx: usize = 0;

    for (bi, block) in blocks.iter().enumerate() {
        if is_block_folded(bi, headings) {
            if matches!(block, Block::Heading { .. }) {
                heading_idx += 1;
            }
            continue;
        }

        match block {
            Block::Metadata { entries } => {
                let line_height = (theme.body_size * 1.5) as u32;
                h += entries.len() as u32 * line_height + PARAGRAPH_GAP * 2;
            }
            Block::Heading { level, spans } => {
                let (size, _) = heading_style(level, theme);
                let scale = PxScale::from(size);
                let hi = heading_idx;
                heading_idx += 1;
                let number = &headings[hi].number;
                let plain = spans_to_plain(spans);
                let fold_prefix = if headings[hi].folded { "▶ " } else { "▼ " };
                let numbered_text = format!("{}{} {}", fold_prefix, number, plain);
                let lines = wrap_spans(
                    &[Span {
                        text: numbered_text,
                        style: SpanStyle::Bold,
                    }],
                    fonts,
                    scale,
                    content_width,
                );
                let line_height = (size * 1.3) as u32;
                h += lines.len() as u32 * line_height + PARAGRAPH_GAP;
            }
            Block::Paragraph { spans } => {
                let scale = PxScale::from(theme.body_size);
                let lines = wrap_spans(spans, fonts, scale, content_width);
                let line_height = (theme.body_size * 1.4) as u32;
                h += lines.len() as u32 * line_height + PARAGRAPH_GAP;
            }
            Block::CodeBlock { text } => {
                let scale = PxScale::from(theme.body_size);
                let mono_lines: Vec<&str> = text.lines().collect();
                let line_height = (theme.body_size * 1.4) as u32;
                let mut total_lines = 0u32;
                for line in &mono_lines {
                    let wrapped = wrap_spans(
                        &[Span {
                            text: line.to_string(),
                            style: SpanStyle::Code,
                        }],
                        fonts,
                        scale,
                        content_width - 20,
                    );
                    total_lines += wrapped.len() as u32;
                }
                if total_lines == 0 {
                    total_lines = 1;
                }
                h += total_lines * line_height + 20 + PARAGRAPH_GAP;
            }
            Block::Table { headers, rows } => {
                h += compute_table_height(headers, rows, fonts, theme, content_width);
                h += PARAGRAPH_GAP * 2;
            }
        }
    }

    h + PARAGRAPH_GAP
}

fn render_metadata(
    img: &mut RgbImage,
    entries: &[(String, String)],
    fonts: &Fonts,
    theme: &Theme,
    start_y: u32,
    margin_left: u32,
) -> u32 {
    let scale = PxScale::from(theme.body_size);
    let line_height = (theme.body_size * 1.5) as u32;
    let mut y = start_y;

    for (key, val) in entries {
        let key_text = format!("{}: ", key);
        let key_w = text_size(scale, &fonts.bold, &key_text).0;
        draw_text_mut(
            img,
            theme.meta_key_color,
            margin_left as i32,
            y as i32,
            scale,
            &fonts.bold,
            &key_text,
        );
        draw_text_mut(
            img,
            theme.meta_val_color,
            (margin_left + key_w) as i32,
            y as i32,
            scale,
            &fonts.regular,
            val,
        );
        y += line_height;
    }

    y
}

fn render_code_block(
    img: &mut RgbImage,
    text: &str,
    fonts: &Fonts,
    theme: &Theme,
    start_y: u32,
    content_width: u32,
    margin_left: u32,
) -> u32 {
    let scale = PxScale::from(theme.body_size);
    let line_height = (theme.body_size * 1.4) as u32;
    let pad = 10u32;
    let inner_width = content_width - pad * 2;

    let source_lines: Vec<&str> = text.lines().collect();
    let mut total_lines = 0u32;
    let mut wrapped_lines: Vec<Vec<Vec<Span>>> = Vec::new();
    for line in &source_lines {
        let w = wrap_spans(
            &[Span {
                text: line.to_string(),
                style: SpanStyle::Code,
            }],
            fonts,
            scale,
            inner_width,
        );
        total_lines += w.len() as u32;
        wrapped_lines.push(w);
    }
    if total_lines == 0 {
        total_lines = 1;
    }
    let block_height = total_lines * line_height + pad * 2;

    draw_filled_rect_mut(
        img,
        Rect::at(margin_left as i32, start_y as i32).of_size(
            content_width,
            block_height,
        ),
        theme.code_bg,
    );

    let mut y = start_y + pad;
    for lines in &wrapped_lines {
        for line in lines {
            draw_spans(img, line, margin_left + pad, y, scale, fonts, theme);
            y += line_height;
        }
    }

    start_y + block_height
}

fn compute_table_height(
    headers: &[Vec<Span>],
    rows: &[Vec<Vec<Span>>],
    fonts: &Fonts,
    theme: &Theme,
    content_width: u32,
) -> u32 {
    let ncols = headers.len().max(1);
    let col_width = content_width / ncols as u32;
    let scale = PxScale::from(theme.body_size);
    let line_height = (theme.body_size * 1.4) as u32;
    let cell_pad_y: u32 = 4;
    let cell_text_width = col_width.saturating_sub(12);

    let mut header_h = line_height + cell_pad_y * 2;
    for cell in headers {
        let wrapped = wrap_spans(cell, fonts, scale, cell_text_width);
        let h = wrapped.len() as u32 * line_height + cell_pad_y * 2;
        header_h = header_h.max(h);
    }

    let mut total = header_h;
    for row in rows {
        let mut row_h = line_height + cell_pad_y * 2;
        for cell in row {
            let wrapped = wrap_spans(cell, fonts, scale, cell_text_width);
            let h = wrapped.len() as u32 * line_height + cell_pad_y * 2;
            row_h = row_h.max(h);
        }
        total += row_h;
    }

    total + 2
}

fn render_table(
    img: &mut RgbImage,
    headers: &[Vec<Span>],
    rows: &[Vec<Vec<Span>>],
    fonts: &Fonts,
    theme: &Theme,
    start_y: u32,
    content_width: u32,
    margin_left: u32,
) -> u32 {
    let ncols = headers.len().max(1);
    let col_width = content_width / ncols as u32;
    let scale = PxScale::from(theme.body_size);
    let line_height = (theme.body_size * 1.4) as u32;
    let cell_padding: u32 = 6;
    let cell_pad_y: u32 = 4;
    let table_width = col_width * ncols as u32;
    let cell_text_width = col_width.saturating_sub(cell_padding * 2);

    let mut y = start_y;

    let mut header_h = line_height + cell_pad_y * 2;
    let mut header_wrapped: Vec<Vec<Vec<Span>>> = Vec::new();
    for cell in headers {
        let wrapped = wrap_spans(cell, fonts, scale, cell_text_width);
        let h = wrapped.len() as u32 * line_height + cell_pad_y * 2;
        header_h = header_h.max(h);
        header_wrapped.push(wrapped);
    }

    draw_filled_rect_mut(
        img,
        Rect::at(margin_left as i32, y as i32).of_size(table_width, header_h),
        theme.table_header_bg,
    );

    for (ci, wrapped) in header_wrapped.iter().enumerate() {
        let x = margin_left + ci as u32 * col_width + cell_padding;
        let mut ty = y + cell_pad_y;
        for line in wrapped {
            for span in line {
                draw_text_mut(
                    img,
                    theme.h2_color,
                    x as i32,
                    ty as i32,
                    scale,
                    &fonts.bold,
                    &span.text,
                );
            }
            ty += line_height;
        }
    }

    let line_y = (y + header_h) as f32;
    draw_line_segment_mut(
        img,
        (margin_left as f32, line_y),
        ((margin_left + table_width) as f32, line_y),
        theme.table_border,
    );

    y += header_h;

    for row in rows {
        let mut row_h = line_height + cell_pad_y * 2;
        let mut row_wrapped: Vec<Vec<Vec<Span>>> = Vec::new();
        for cell in row {
            let wrapped = wrap_spans(cell, fonts, scale, cell_text_width);
            let h = wrapped.len() as u32 * line_height + cell_pad_y * 2;
            row_h = row_h.max(h);
            row_wrapped.push(wrapped);
        }

        for (ci, wrapped) in row_wrapped.iter().enumerate() {
            let x = margin_left + ci as u32 * col_width + cell_padding;
            let mut ty = y + cell_pad_y;
            for line in wrapped {
                draw_spans(img, line, x, ty, scale, fonts, theme);
                ty += line_height;
            }
        }

        y += row_h;

        draw_line_segment_mut(
            img,
            (margin_left as f32, y as f32),
            ((margin_left + table_width) as f32, y as f32),
            theme.table_border,
        );
    }

    for ci in 0..=ncols {
        let x = (margin_left + ci as u32 * col_width) as f32;
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

    write!(w, "\x1b[H")?;

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

fn display_viewport(
    w: &mut impl Write,
    img: &RgbImage,
    scroll_y: u32,
    vp_width: u32,
    vp_height: u32,
    frame: &mut u32,
    overlay: Option<&RgbImage>,
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

    // Draw overlay bar at bottom if present
    if let Some(overlay_img) = overlay {
        let overlay_h = overlay_img.height() as usize;
        let overlay_start = (src_h as usize).saturating_sub(overlay_h);
        let overlay_raw = overlay_img.as_raw();
        let copy_w = src_w.min(overlay_img.width()) as usize * 3;
        for row in 0..overlay_h.min(src_h as usize) {
            let dst_offset = (overlay_start + row) * src_w as usize * 3;
            let src_offset = row * overlay_img.width() as usize * 3;
            if dst_offset + copy_w <= viewport_data.len()
                && src_offset + copy_w <= overlay_raw.len()
            {
                viewport_data[dst_offset..dst_offset + copy_w]
                    .copy_from_slice(&overlay_raw[src_offset..src_offset + copy_w]);
            }
        }
    }

    let new_id = *frame;
    let old_id = if *frame == 1 { 2 } else { 1 };
    *frame = old_id;

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

// --- App state ---

struct AppState {
    blocks: Vec<Block>,
    headings: Vec<HeadingInfo>,
    current_heading: Option<usize>, // index into headings vec
    scroll_y: u32,
    vp_width: u32,
    vp_height: u32,
    frame: u32,
    img: RgbImage,
    block_y_positions: Vec<(usize, u32)>, // (block_index, y_pos)
    search_mode: bool,
    search_query: String,
    search_matches: Vec<usize>, // indices into block_y_positions
    search_current: usize,
}

impl AppState {
    fn new(source: &str, fonts: &Fonts, vp_width: u32, vp_height: u32) -> Self {
        let blocks = parse_markdown(source);
        let headings = build_headings(&blocks);
        let current_heading = if headings.is_empty() { None } else { Some(0) };

        let mut state = AppState {
            blocks,
            headings,
            current_heading,
            scroll_y: 0,
            vp_width,
            vp_height,
            frame: 1,
            img: RgbImage::new(1, 1), // placeholder
            block_y_positions: Vec::new(),
            search_mode: false,
            search_query: String::new(),
            search_matches: Vec::new(),
            search_current: 0,
        };
        state.rerender(fonts);
        state
    }

    fn rerender(&mut self, fonts: &Fonts) {
        let (img, positions) = render_markdown(
            &self.blocks,
            &mut self.headings,
            self.current_heading,
            self.vp_width,
            fonts,
        );
        self.img = img;
        self.block_y_positions = positions;
        // Clamp scroll
        let max_scroll = self.max_scroll();
        if self.scroll_y > max_scroll {
            self.scroll_y = max_scroll;
        }
    }

    fn max_scroll(&self) -> u32 {
        self.img.height().saturating_sub(self.vp_height)
    }

    fn navigate_heading(&mut self, direction: i32, fonts: &Fonts) -> bool {
        if self.headings.is_empty() {
            return false;
        }

        let current = self.current_heading.unwrap_or(0);
        let new_idx = if direction > 0 {
            // Find next visible heading
            let mut idx = current + 1;
            while idx < self.headings.len() {
                if !is_block_folded(self.headings[idx].block_index, &self.headings) {
                    break;
                }
                idx += 1;
            }
            if idx < self.headings.len() {
                idx
            } else {
                return false;
            }
        } else {
            if current == 0 {
                return false;
            }
            // Find previous visible heading
            let mut idx = current - 1;
            loop {
                if !is_block_folded(self.headings[idx].block_index, &self.headings) {
                    break;
                }
                if idx == 0 {
                    return false;
                }
                idx -= 1;
            }
            idx
        };

        self.current_heading = Some(new_idx);
        self.rerender(fonts);

        // Scroll to make heading visible
        let heading = &self.headings[new_idx];
        if heading.y_pos < self.scroll_y {
            self.scroll_y = heading.y_pos.saturating_sub(PARAGRAPH_GAP);
        } else if heading.y_pos + heading.heading_height > self.scroll_y + self.vp_height {
            self.scroll_y = (heading.y_pos + heading.heading_height)
                .saturating_sub(self.vp_height)
                .min(self.max_scroll());
        }

        true
    }

    fn toggle_fold(&mut self, fonts: &Fonts) -> bool {
        if let Some(hi) = self.current_heading {
            self.headings[hi].folded = !self.headings[hi].folded;
            self.rerender(fonts);
            true
        } else {
            false
        }
    }

    fn scroll(&mut self, delta: i32) -> bool {
        let max = self.max_scroll();
        let new_y = if delta > 0 {
            (self.scroll_y + delta as u32).min(max)
        } else {
            self.scroll_y.saturating_sub((-delta) as u32)
        };
        if new_y != self.scroll_y {
            self.scroll_y = new_y;
            true
        } else {
            false
        }
    }

    fn execute_search(&mut self) {
        self.search_matches.clear();
        self.search_current = 0;
        if self.search_query.is_empty() {
            return;
        }
        let query = self.search_query.to_lowercase();
        for (pos_idx, &(bi, _y)) in self.block_y_positions.iter().enumerate() {
            if block_contains_text(&self.blocks[bi], &query) {
                self.search_matches.push(pos_idx);
            }
        }
        // Scroll to first match
        if !self.search_matches.is_empty() {
            let pos_idx = self.search_matches[0];
            let (_bi, y) = self.block_y_positions[pos_idx];
            self.scroll_y = y.saturating_sub(PARAGRAPH_GAP).min(self.max_scroll());
        }
    }

    fn navigate_search(&mut self, forward: bool) -> bool {
        if self.search_matches.is_empty() {
            return false;
        }
        if forward {
            self.search_current = (self.search_current + 1) % self.search_matches.len();
        } else {
            self.search_current = if self.search_current == 0 {
                self.search_matches.len() - 1
            } else {
                self.search_current - 1
            };
        }
        let pos_idx = self.search_matches[self.search_current];
        let (_bi, y) = self.block_y_positions[pos_idx];
        let old_scroll = self.scroll_y;
        self.scroll_y = y.saturating_sub(PARAGRAPH_GAP).min(self.max_scroll());
        self.scroll_y != old_scroll
    }
}

fn block_contains_text(block: &Block, query: &str) -> bool {
    match block {
        Block::Heading { spans, .. } | Block::Paragraph { spans } => {
            spans_to_plain(spans).to_lowercase().contains(query)
        }
        Block::CodeBlock { text } => text.to_lowercase().contains(query),
        Block::Table { headers, rows } => {
            headers
                .iter()
                .any(|h| spans_to_plain(h).to_lowercase().contains(query))
                || rows.iter().any(|row| {
                    row.iter()
                        .any(|cell| spans_to_plain(cell).to_lowercase().contains(query))
                })
        }
        Block::Metadata { entries } => entries.iter().any(|(k, v)| {
            k.to_lowercase().contains(query) || v.to_lowercase().contains(query)
        }),
    }
}

fn render_help_overlay(vp_width: u32, vp_height: u32, fonts: &Fonts) -> RgbImage {
    let mut img = RgbImage::from_pixel(vp_width, vp_height, Rgb([30, 30, 40]));
    let scale = PxScale::from(20.0);
    let title_scale = PxScale::from(28.0);
    let line_height = 32i32;
    let x = 40i32;
    let mut y = 40i32;

    draw_text_mut(
        &mut img,
        Rgb([100, 160, 255]),
        x,
        y,
        title_scale,
        &fonts.bold,
        "Keybindings",
    );
    y += 50;

    let bindings = [
        ("Up / Down", "Navigate between headings"),
        ("Left / Right / Tab", "Toggle fold open/close"),
        ("Space", "Scroll down"),
        ("Shift+Space", "Scroll up"),
        ("j / k", "Small scroll steps"),
        ("PgUp / PgDn", "Half-page scroll"),
        ("Home / End", "Jump to top/bottom"),
        ("/", "Search text"),
        ("n / N", "Next/previous search match"),
        ("h", "Show this help"),
        ("q / Esc", "Quit"),
    ];

    for (key, desc) in &bindings {
        draw_text_mut(
            &mut img,
            Rgb([230, 180, 80]),
            x,
            y,
            scale,
            &fonts.bold,
            key,
        );
        draw_text_mut(
            &mut img,
            Rgb([220, 220, 220]),
            x + 220,
            y,
            scale,
            &fonts.regular,
            desc,
        );
        y += line_height;
    }

    y += 20;
    draw_text_mut(
        &mut img,
        Rgb([140, 140, 160]),
        x,
        y,
        scale,
        &fonts.regular,
        "Press any key to dismiss",
    );

    img
}

fn render_search_bar(
    query: &str,
    match_info: Option<(usize, usize)>,
    width: u32,
    fonts: &Fonts,
) -> RgbImage {
    let bar_height = 30u32;
    let scale = PxScale::from(18.0);
    let mut bar = RgbImage::from_pixel(width, bar_height, Rgb([50, 50, 65]));

    // Draw top border
    for x in 0..width {
        bar.put_pixel(x, 0, Rgb([80, 80, 100]));
    }

    let display = format!("/{}", query);
    draw_text_mut(
        &mut bar,
        Rgb([220, 220, 220]),
        10,
        5,
        scale,
        &fonts.regular,
        &display,
    );

    if let Some((current, total)) = match_info {
        let info = format!("{}/{}", current, total);
        let (tw, _) = text_size(scale, &fonts.regular, &info);
        draw_text_mut(
            &mut bar,
            Rgb([180, 180, 180]),
            (width - tw - 10) as i32,
            5,
            scale,
            &fonts.regular,
            &info,
        );
    }

    bar
}

// --- Main ---

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 || args[1] == "--help" || args[1] == "-h" {
        eprintln!("mdbdp - Display Markdown But Do it Pretty");
        eprintln!();
        eprintln!("Usage: mdbdp <markdown-file>");
        eprintln!();
        eprintln!("Renders a markdown file as an image and displays it in the terminal");
        eprintln!("using the Kitty graphics protocol.");
        eprintln!();
        eprintln!("Keybindings:");
        eprintln!("  Up/Down        Navigate between headings");
        eprintln!("  Left/Right/Tab Toggle fold open/close");
        eprintln!("  Space          Scroll down");
        eprintln!("  Shift+Space    Scroll up");
        eprintln!("  j/k            Small scroll steps");
        eprintln!("  PgUp/PgDn      Half-page scroll");
        eprintln!("  Home/End       Jump to top/bottom");
        eprintln!("  /              Search text");
        eprintln!("  n/N            Next/previous search match");
        eprintln!("  h              Show help overlay");
        eprintln!("  q/Esc          Quit");
        std::process::exit(if args.len() < 2 { 1 } else { 0 });
    }

    let file_path = &args[1];

    let source = std::fs::read_to_string(file_path)
        .unwrap_or_else(|e| panic!("Cannot read {}: {}", file_path, e));

    let fonts = load_fonts();
    let (vp_width, vp_height) = get_viewport_pixel_size()?;

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
        display_viewport(
            &mut out,
            &state.img,
            state.scroll_y,
            vp_width,
            vp_height,
            &mut state.frame,
            None,
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
                // Search input mode
                match (code, modifiers) {
                    (KeyCode::Esc, _) => {
                        state.search_mode = false;
                        state.search_query.clear();
                        state.search_matches.clear();
                        true
                    }
                    (KeyCode::Enter, _) => {
                        state.search_mode = false;
                        state.execute_search();
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

                    // Help overlay
                    (KeyCode::Char('h'), KeyModifiers::NONE) => {
                        let help_img = render_help_overlay(vp_width, vp_height, &fonts);
                        let mut out = BufWriter::new(stdout.lock());
                        let raw = help_img.as_raw();
                        let new_id = state.frame;
                        let old_id = if state.frame == 1 { 2 } else { 1 };
                        state.frame = old_id;
                        kitty_display_raw(&mut out, raw, vp_width, vp_height, new_id, old_id)?;
                        // Wait for any key to dismiss
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

                    // Enter search mode
                    (KeyCode::Char('/'), KeyModifiers::NONE) => {
                        state.search_mode = true;
                        state.search_query.clear();
                        state.search_matches.clear();
                        state.search_current = 0;
                        true
                    }

                    // Search navigation
                    (KeyCode::Char('n'), KeyModifiers::NONE) => state.navigate_search(true),
                    (KeyCode::Char('N'), KeyModifiers::SHIFT) => state.navigate_search(false),

                    // Up/Down: navigate between headings
                    (KeyCode::Down, KeyModifiers::NONE) => state.navigate_heading(1, &fonts),
                    (KeyCode::Up, KeyModifiers::NONE) => state.navigate_heading(-1, &fonts),

                    // Left/Right: toggle fold
                    (KeyCode::Left, KeyModifiers::NONE)
                    | (KeyCode::Right, KeyModifiers::NONE)
                    | (KeyCode::Tab, _) => state.toggle_fold(&fonts),

                    // Space: scroll down, Shift+Space (any modifier): scroll up
                    (KeyCode::Char(' '), KeyModifiers::NONE) => {
                        state.scroll(SCROLL_STEP as i32)
                    }
                    (KeyCode::Char(' '), _) => state.scroll(-(SCROLL_STEP as i32)),

                    // j/k: small scroll steps
                    (KeyCode::Char('j'), _) => state.scroll(SCROLL_STEP as i32),
                    (KeyCode::Char('k'), _) => state.scroll(-(SCROLL_STEP as i32)),

                    // PgDn/PgUp
                    (KeyCode::PageDown, _) => state.scroll(vp_height as i32 / 2),
                    (KeyCode::PageUp, _) => state.scroll(-(vp_height as i32 / 2)),

                    (KeyCode::Home, _) => {
                        let changed = state.scroll_y != 0;
                        state.scroll_y = 0;
                        changed
                    }
                    (KeyCode::End, _) => {
                        let max = state.max_scroll();
                        let changed = state.scroll_y != max;
                        state.scroll_y = max;
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

                let mut out = BufWriter::new(stdout.lock());
                display_viewport(
                    &mut out,
                    &state.img,
                    state.scroll_y,
                    vp_width,
                    vp_height,
                    &mut state.frame,
                    overlay.as_ref(),
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
