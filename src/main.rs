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
use std::path::{Path, PathBuf};

const SCROLL_STEP: u32 = 40;
const MARGIN_LEFT: u32 = 20;
const MARGIN_RIGHT: u32 = 20;
const PARAGRAPH_GAP: u32 = 16;
const CURSOR_WIDTH: u32 = 4;
const CURSOR_MARGIN: u32 = 6; // gap between cursor and text
const MAX_CONTENT_WIDTH: u32 = 900;
const H1_EXTRA_MARGIN: u32 = 40;
const BLOCK_INDENT: u32 = 24;

// --- Theme ---

#[derive(Clone, Copy)]
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
            MdEvent::End(TagEnd::Item) => {
                // Commit tight list items that weren't wrapped in Paragraph events
                let s = std::mem::take(&mut spans);
                if !s.is_empty() {
                    blocks.push(Block::Paragraph { spans: s });
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

    // Flush any remaining uncommitted spans
    if !spans.is_empty() {
        blocks.push(Block::Paragraph { spans });
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
        let w = text_size(scale, font, &span.text).0;
        cx += w;
    }
    cx
}

// --- Layout & render ---

fn render_markdown(
    blocks: &[Block],
    headings: &mut [HeadingInfo],
    width: u32,
    fonts: &Fonts,
) -> (RgbImage, Vec<(usize, u32)>, u32) {
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
                y = render_metadata(&mut img, entries, fonts, &theme, y, margin_left + BLOCK_INDENT);
                y += PARAGRAPH_GAP * 2;
            }
            Block::Heading { level, spans } => {
                if matches!(level, HeadingLevel::H1) {
                    y += H1_EXTRA_MARGIN;
                }

                let hi = heading_idx;
                heading_idx += 1;

                let (lines, size, line_height) = wrap_heading_text(
                    &headings[hi], spans, fonts, &theme, content_width,
                );
                let (_, color) = heading_style(level, &theme);
                let scale = PxScale::from(size);
                let heading_total_h = lines.len() as u32 * line_height;

                // Record Y position for navigation
                headings[hi].y_pos = y;
                headings[hi].heading_height = heading_total_h;

                // Draw fold arrow at smaller size to the left of the heading
                let fold_char = if headings[hi].folded { "▶" } else { "▼" };
                let arrow_scale = PxScale::from(size * 0.5);
                let arrow_w = text_size(arrow_scale, &fonts.bold, fold_char).0;
                let arrow_x = margin_left as i32 - arrow_w as i32 - 4;

                let arrow_y_offset = ((size - size * 0.5) * 0.5) as i32;
                draw_text_mut(
                    &mut img,
                    color,
                    arrow_x,
                    y as i32 + arrow_y_offset,
                    arrow_scale,
                    &fonts.bold,
                    fold_char,
                );

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
                let indented_width = content_width - BLOCK_INDENT;
                let lines = wrap_spans(spans, fonts, scale, indented_width);
                let line_height = (theme.body_size * 1.4) as u32;

                for line in &lines {
                    draw_spans(&mut img, line, margin_left + BLOCK_INDENT, y, scale, fonts, &theme);
                    y += line_height;
                }
                y += PARAGRAPH_GAP;
            }
            Block::CodeBlock { text } => {
                y = render_code_block(&mut img, text, fonts, &theme, y, content_width - BLOCK_INDENT, margin_left + BLOCK_INDENT);
                y += PARAGRAPH_GAP;
            }
            Block::Table { headers, rows } => {
                y = render_table(&mut img, headers, rows, fonts, &theme, y, content_width - BLOCK_INDENT, margin_left + BLOCK_INDENT);
                y += PARAGRAPH_GAP * 2;
            }
        }
    }

    (img, block_positions, margin_left)
}

fn wrap_code_lines(text: &str, fonts: &Fonts, scale: PxScale, inner_width: u32) -> Vec<Vec<Vec<Span>>> {
    text.lines()
        .map(|line| {
            wrap_spans(
                &[Span { text: line.to_string(), style: SpanStyle::Code }],
                fonts, scale, inner_width,
            )
        })
        .collect()
}

fn wrap_heading_text(
    heading: &HeadingInfo,
    spans: &[Span],
    fonts: &Fonts,
    theme: &Theme,
    content_width: u32,
) -> (Vec<Vec<Span>>, f32, u32) {
    let (size, _) = heading_style(&heading.level, theme);
    let scale = PxScale::from(size);
    let numbered_text = format!("{} {}", heading.number, spans_to_plain(spans));
    let lines = wrap_spans(
        &[Span { text: numbered_text, style: SpanStyle::Bold }],
        fonts, scale, content_width,
    );
    let line_height = (size * 1.3) as u32;
    (lines, size, line_height)
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
                if matches!(level, HeadingLevel::H1) && h > PARAGRAPH_GAP {
                    h += H1_EXTRA_MARGIN;
                }
                let hi = heading_idx;
                heading_idx += 1;
                let (lines, _, line_height) = wrap_heading_text(
                    &headings[hi], spans, fonts, theme, content_width,
                );
                h += lines.len() as u32 * line_height + PARAGRAPH_GAP;
            }
            Block::Paragraph { spans } => {
                let scale = PxScale::from(theme.body_size);
                let indented_width = content_width - BLOCK_INDENT;
                let lines = wrap_spans(spans, fonts, scale, indented_width);
                let line_height = (theme.body_size * 1.4) as u32;
                h += lines.len() as u32 * line_height + PARAGRAPH_GAP;
            }
            Block::CodeBlock { text } => {
                let scale = PxScale::from(theme.body_size);
                let line_height = (theme.body_size * 1.4) as u32;
                let indented_width = content_width - BLOCK_INDENT;
                let wrapped = wrap_code_lines(text, fonts, scale, indented_width - 20);
                let total_lines = wrapped.iter().map(|w| w.len() as u32).sum::<u32>().max(1);
                h += total_lines * line_height + 20 + PARAGRAPH_GAP;
            }
            Block::Table { headers, rows } => {
                h += compute_table_height(headers, rows, fonts, theme, content_width - BLOCK_INDENT);
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

    let wrapped_lines = wrap_code_lines(text, fonts, scale, inner_width);
    let total_lines = wrapped_lines.iter().map(|w| w.len() as u32).sum::<u32>().max(1);
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

const KEYBINDINGS: &[(&str, &str)] = &[
    ("Up / Down", "Navigate between headings"),
    ("Left / Right / Tab", "Toggle fold open/close"),
    ("Space", "Scroll down"),
    ("Ctrl+Space", "Scroll up"),
    ("j / k", "Small scroll steps"),
    ("PgUp / PgDn", "Half-page scroll"),
    ("Home / End", "Jump to top/bottom"),
    ("/", "Search text"),
    ("n / N", "Next/previous search match"),
    ("h", "Show this help"),
    ("q / Esc", "Quit"),
];

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

fn paint_rect(
    data: &mut [u8],
    stride: usize,
    x: u32, y: u32, w: u32, h: u32,
    max_w: u32,
    color: [u8; 3],
    alpha: f32,
) {
    let inv = 1.0 - alpha;
    for row in y as usize..(y + h) as usize {
        for px in 0..w as usize {
            let xi = x as usize + px;
            if xi < max_w as usize {
                let offset = row * stride + xi * 3;
                if offset + 2 < data.len() {
                    if alpha >= 1.0 {
                        data[offset] = color[0];
                        data[offset + 1] = color[1];
                        data[offset + 2] = color[2];
                    } else {
                        data[offset] = (data[offset] as f32 * inv + color[0] as f32 * alpha) as u8;
                        data[offset + 1] = (data[offset + 1] as f32 * inv + color[1] as f32 * alpha) as u8;
                        data[offset + 2] = (data[offset + 2] as f32 * inv + color[2] as f32 * alpha) as u8;
                    }
                }
            }
        }
    }
}

/// cursor_info: Option<(x, y_in_image, height, color)>
fn display_viewport(
    w: &mut impl Write,
    img: &RgbImage,
    scroll_y: u32,
    vp_width: u32,
    vp_height: u32,
    frame: &mut u32,
    overlay: Option<&RgbImage>,
    cursor_info: Option<(u32, u32, u32, [u8; 3])>,
    highlights: &[(u32, u32, u32, u32, usize)],
    current_match: usize,
) -> io::Result<()> {
    let src_w = vp_width.min(img.width());
    let src_h = vp_height.min(img.height().saturating_sub(scroll_y));
    let stride = src_w as usize * 3;

    let raw = img.as_raw();
    let img_stride = img.width() as usize * 3;
    let row_start = scroll_y as usize * img_stride;
    let mut viewport_data = Vec::with_capacity(src_w as usize * src_h as usize * 3);
    for row in 0..src_h as usize {
        let offset = row_start + row * img_stride;
        viewport_data.extend_from_slice(&raw[offset..offset + src_w as usize * 3]);
    }

    // Draw search highlights
    for &(hx, hy, hw, hh, midx) in highlights {
        if hy + hh <= scroll_y || hy >= scroll_y + src_h {
            continue;
        }
        let is_current = midx == current_match;
        let (color, alpha) = if is_current {
            ([255, 180, 50], 0.35)
        } else {
            ([180, 180, 60], 0.20)
        };
        paint_rect(&mut viewport_data, stride, hx, hy.saturating_sub(scroll_y),
            hw, (hy + hh).saturating_sub(scroll_y).min(src_h) - hy.saturating_sub(scroll_y),
            src_w, color, alpha);
    }

    // Draw cursor bar onto viewport data
    if let Some((cx, cy_img, ch, color)) = cursor_info {
        paint_rect(&mut viewport_data, stride, cx, cy_img.saturating_sub(scroll_y),
            CURSOR_WIDTH, (cy_img + ch).saturating_sub(scroll_y).min(src_h) - cy_img.saturating_sub(scroll_y),
            src_w, color, 1.0);
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
    let old_id = if new_id == 1 { 2 } else { 1 };
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
    margin_left: u32,
    theme: Theme,
    search_mode: bool,
    search_query: String,
    search_matches: Vec<usize>, // indices into block_y_positions
    search_current: usize,
    search_highlights: Vec<(u32, u32, u32, u32, usize)>, // (x, y, w, h, match_idx)
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
            margin_left: 0,
            theme: default_theme(),
            search_mode: false,
            search_query: String::new(),
            search_matches: Vec::new(),
            search_current: 0,
            search_highlights: Vec::new(),
        };
        state.rerender(fonts);
        state
    }

    fn rerender(&mut self, fonts: &Fonts) {
        let (img, positions, margin_left) = render_markdown(
            &self.blocks,
            &mut self.headings,
            self.vp_width,
            fonts,
        );
        self.img = img;
        self.block_y_positions = positions;
        self.margin_left = margin_left;
        // Clamp scroll
        let max_scroll = self.max_scroll();
        if self.scroll_y > max_scroll {
            self.scroll_y = max_scroll;
        }
    }

    fn max_scroll(&self) -> u32 {
        self.img.height().saturating_sub(self.vp_height)
    }

    fn navigate_heading(&mut self, direction: i32) -> bool {
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

        // Scroll to place heading at ~1/4 from the top
        let heading = &self.headings[new_idx];
        let target = heading.y_pos.saturating_sub(self.vp_height / 4);
        self.scroll_y = target.min(self.max_scroll());

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

    fn cursor_info(&self) -> Option<(u32, u32, u32, [u8; 3])> {
        let hi = self.current_heading?;
        let heading = &self.headings[hi];
        let (size, _) = heading_style(&heading.level, &self.theme);
        // Place cursor to the left of the fold arrow area
        let arrow_space = (size * 0.5) as u32 + 4;
        let cursor_x = self.margin_left.saturating_sub(arrow_space + CURSOR_MARGIN + CURSOR_WIDTH);
        let c = self.theme.cursor_color.0;
        Some((cursor_x, heading.y_pos, heading.heading_height, c))
    }

    fn next_frame(&mut self) -> (u32, u32) {
        let new_id = self.frame;
        let old_id = if self.frame == 1 { 2 } else { 1 };
        self.frame = old_id;
        (new_id, old_id)
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

    fn execute_search(&mut self, fonts: &Fonts) {
        self.search_matches.clear();
        self.search_highlights.clear();
        self.search_current = 0;
        if self.search_query.is_empty() {
            return;
        }
        let query = self.search_query.to_lowercase();
        let content_width = self.vp_width - 2 * self.margin_left;
        let mut match_idx = 0usize;

        for (pos_idx, &(bi, block_y)) in self.block_y_positions.iter().enumerate() {
            let block = &self.blocks[bi];
            if !block_contains_text(block, &query) {
                continue;
            }
            self.search_matches.push(pos_idx);

            let highlights = compute_block_highlights(
                block, block_y, &query, fonts, &self.theme,
                content_width, self.margin_left, match_idx,
                &self.headings, bi,
            );
            self.search_highlights.extend(highlights);
            match_idx += 1;
        }

        // Scroll to first highlight
        if let Some(&(_, hy, _, _, _)) = self.search_highlights.first() {
            self.scroll_y = hy.saturating_sub(self.vp_height / 4).min(self.max_scroll());
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
        // Scroll to first highlight of the current match
        let target_idx = self.search_current;
        if let Some(&(_, hy, _, _, _)) = self
            .search_highlights
            .iter()
            .find(|h| h.4 == target_idx)
        {
            self.scroll_y = hy.saturating_sub(self.vp_height / 4).min(self.max_scroll());
        }
        true // always redraw to update highlight colors
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

fn find_highlights_in_text(
    text: &str,
    query: &str,
    font: &FontVec,
    scale: PxScale,
    line_height: u32,
    x: u32,
    y: u32,
    match_idx: usize,
) -> Vec<(u32, u32, u32, u32, usize)> {
    let mut highlights = Vec::new();
    let text_lower = text.to_lowercase();
    let mut search_from = 0;
    while search_from < text_lower.len() {
        if let Some(byte_pos) = text_lower[search_from..].find(query) {
            let match_start = search_from + byte_pos;
            let match_end = match_start + query.len();
            let prefix = &text[..match_start];
            let match_text = &text[match_start..match_end];
            let x_offset = text_size(scale, font, prefix).0;
            let match_width = text_size(scale, font, match_text).0;
            highlights.push((x + x_offset, y, match_width, line_height, match_idx));
            search_from = match_end;
        } else {
            break;
        }
    }
    highlights
}

fn compute_block_highlights(
    block: &Block,
    block_y: u32,
    query: &str,
    fonts: &Fonts,
    theme: &Theme,
    content_width: u32,
    margin_left: u32,
    match_idx: usize,
    headings: &[HeadingInfo],
    block_index: usize,
) -> Vec<(u32, u32, u32, u32, usize)> {
    let mut highlights = Vec::new();
    match block {
        Block::Paragraph { spans } => {
            let scale = PxScale::from(theme.body_size);
            let indented_width = content_width - BLOCK_INDENT;
            let lines = wrap_spans(spans, fonts, scale, indented_width);
            let line_height = (theme.body_size * 1.4) as u32;
            let x_start = margin_left + BLOCK_INDENT;
            let mut y = block_y;
            for line in &lines {
                let plain = spans_to_plain(line);
                highlights.extend(find_highlights_in_text(
                    &plain, query, &fonts.regular, scale, line_height,
                    x_start, y, match_idx,
                ));
                y += line_height;
            }
        }
        Block::Heading { level: _, spans } => {
            let hi = headings.iter().find(|h| h.block_index == block_index);
            if let Some(heading) = hi {
                let (lines, size, line_height) = wrap_heading_text(
                    heading, spans, fonts, theme, content_width,
                );
                let scale = PxScale::from(size);
                let mut y = block_y;
                for line in &lines {
                    let line_plain = spans_to_plain(line);
                    highlights.extend(find_highlights_in_text(
                        &line_plain, query, &fonts.bold, scale, line_height,
                        margin_left, y, match_idx,
                    ));
                    y += line_height;
                }
            }
        }
        Block::CodeBlock { text } => {
            let scale = PxScale::from(theme.body_size);
            let indented_width = content_width - BLOCK_INDENT;
            let pad = 10u32;
            let inner_width = indented_width - pad * 2;
            let line_height = (theme.body_size * 1.4) as u32;
            let x_start = margin_left + BLOCK_INDENT + pad;
            let mut y = block_y + pad;
            for wrapped in &wrap_code_lines(text, fonts, scale, inner_width) {
                for line in wrapped {
                    let plain = spans_to_plain(line);
                    highlights.extend(find_highlights_in_text(
                        &plain, query, &fonts.mono, scale, line_height,
                        x_start, y, match_idx,
                    ));
                    y += line_height;
                }
            }
        }
        _ => {}
    }
    highlights
}

fn render_help_overlay(vp_width: u32, vp_height: u32, fonts: &Fonts) -> RgbImage {
    let mut img = RgbImage::from_pixel(vp_width, vp_height, Rgb([30, 30, 40]));
    let content_width = (vp_width - MARGIN_LEFT - MARGIN_RIGHT).min(MAX_CONTENT_WIDTH);
    let margin_left = (vp_width - content_width) / 2;
    let scale = PxScale::from(20.0);
    let title_scale = PxScale::from(28.0);
    let line_height = 32i32;
    let x = margin_left as i32;
    let mut y = (PARAGRAPH_GAP + H1_EXTRA_MARGIN) as i32;

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

    let indent_x = x + BLOCK_INDENT as i32;
    for &(key, desc) in KEYBINDINGS {
        draw_text_mut(
            &mut img,
            Rgb([230, 180, 80]),
            indent_x,
            y,
            scale,
            &fonts.bold,
            key,
        );
        draw_text_mut(
            &mut img,
            Rgb([220, 220, 220]),
            indent_x + 220,
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
        indent_x,
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

// --- File browser ---

#[derive(Clone, Debug)]
enum BrowserEntry {
    Dir(String),
    File(String),
}

fn scan_directory(dir: &Path) -> Vec<BrowserEntry> {
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
                } else if name.ends_with(".md") || name.ends_with(".MD") {
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

struct BrowserState {
    current_dir: PathBuf,
    entries: Vec<BrowserEntry>,
    cursor: usize,
    preview_img: Option<RgbImage>,
    frame: u32,
}

const BROWSER_LEFT_COLS: u16 = 35;

fn draw_file_list(out: &mut impl Write, state: &BrowserState, term_rows: u16) -> io::Result<()> {
    let max_display = (term_rows.saturating_sub(2)) as usize;
    // Header: current directory
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
                BrowserEntry::File(name) => ("   ", name.clone(), "\x1b[36m", "\x1b[7;36m"),
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

fn browser_clear_preview(out: &mut impl Write) -> io::Result<()> {
    write!(
        out,
        "\x1b_Ga=d,d=I,i=1,q=2\x1b\\\x1b_Ga=d,d=I,i=2,q=2\x1b\\"
    )?;
    execute!(out, terminal::Clear(ClearType::All))
}

fn run_browser(dir: &Path, fonts: &Fonts, vp_width: u32, vp_height: u32) -> io::Result<()> {
    let (_term_cols, term_rows) = terminal::size()?;
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
        execute!(
            out,
            terminal::EnterAlternateScreen,
            cursor::Hide,
            terminal::Clear(ClearType::All)
        )?;
        draw_file_list(&mut out, &state, term_rows)?;
    }

    loop {
        if let Event::Key(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            ..
        }) = event::read()?
        {
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
                    if let Some(BrowserEntry::Dir(name)) = state.entries.get(state.cursor).cloned()
                    {
                        let new_dir = state.current_dir.join(&name);
                        state.entries = scan_directory(&new_dir);
                        state.current_dir = new_dir.canonicalize().unwrap_or(new_dir);
                        state.cursor = 0;
                        state.preview_img = None;
                        let mut out = BufWriter::new(stdout.lock());
                        browser_clear_preview(&mut out)?;
                    }
                    true
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    if let Some(parent) = state.current_dir.parent() {
                        let parent = parent.to_path_buf();
                        state.entries = scan_directory(&parent);
                        state.current_dir = parent.canonicalize().unwrap_or(parent);
                        state.cursor = 0;
                        state.preview_img = None;
                        let mut out = BufWriter::new(stdout.lock());
                        browser_clear_preview(&mut out)?;
                    }
                    true
                }
                KeyCode::Enter => {
                    if let Some(BrowserEntry::File(name)) =
                        state.entries.get(state.cursor).cloned()
                    {
                        let file_path = state.current_dir.join(&name);
                        if let Ok(source) = std::fs::read_to_string(&file_path) {
                            let preview_width =
                                vp_width.saturating_sub(BROWSER_LEFT_COLS as u32 * 8);
                            let blocks = parse_markdown(&source);
                            let mut headings = build_headings(&blocks);
                            let (img, _, _) =
                                render_markdown(&blocks, &mut headings, preview_width, fonts);
                            let preview_h = vp_height.min(img.height());
                            let src_w = preview_width.min(img.width());
                            let raw = img.as_raw();
                            let img_stride = img.width() as usize * 3;
                            let mut viewport_data =
                                Vec::with_capacity(src_w as usize * preview_h as usize * 3);
                            for row in 0..preview_h as usize {
                                let offset = row * img_stride;
                                viewport_data.extend_from_slice(
                                    &raw[offset..offset + src_w as usize * 3],
                                );
                            }
                            let mut out = BufWriter::new(stdout.lock());
                            let new_id = state.frame;
                            let old_id = if new_id == 1 { 2 } else { 1 };
                            state.frame = old_id;
                            kitty_display_at(
                                &mut out,
                                &viewport_data,
                                src_w,
                                preview_h,
                                BROWSER_LEFT_COLS + 1,
                                new_id,
                                old_id,
                            )?;
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
        write!(
            out,
            "\x1b_Ga=d,d=I,i=1,q=2\x1b\\\x1b_Ga=d,d=I,i=2,q=2\x1b\\"
        )?;
        execute!(out, cursor::Show, terminal::LeaveAlternateScreen)?;
    }
    terminal::disable_raw_mode()?;
    Ok(())
}

// --- Main ---

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
        return run_browser(path, &fonts, vp_width, vp_height);
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
                // Search input mode
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

                    // Help overlay
                    (KeyCode::Char('h'), KeyModifiers::NONE) => {
                        let help_img = render_help_overlay(vp_width, vp_height, &fonts);
                        let mut out = BufWriter::new(stdout.lock());
                        let raw = help_img.as_raw();
                        let (new_id, old_id) = state.next_frame();
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
                        state.search_highlights.clear();
                        state.search_current = 0;
                        true
                    }

                    // Search navigation
                    (KeyCode::Char('n'), KeyModifiers::NONE) => state.navigate_search(true),
                    (KeyCode::Char('N'), KeyModifiers::SHIFT) => state.navigate_search(false),

                    // Up/Down: navigate between headings
                    (KeyCode::Down, KeyModifiers::NONE) => state.navigate_heading(1),
                    (KeyCode::Up, KeyModifiers::NONE) => state.navigate_heading(-1),

                    // Left/Right: toggle fold
                    (KeyCode::Left, KeyModifiers::NONE)
                    | (KeyCode::Right, KeyModifiers::NONE)
                    | (KeyCode::Tab, _) => state.toggle_fold(&fonts),

                    // Space: scroll down, Ctrl+Space: scroll up
                    (KeyCode::Char(' '), KeyModifiers::NONE) => {
                        state.scroll(SCROLL_STEP as i32)
                    }
                    (KeyCode::Char(' '), KeyModifiers::CONTROL) => state.scroll(-(SCROLL_STEP as i32)),

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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_fonts() -> Fonts {
        load_fonts()
    }

    const SAMPLE_MD: &str = "\
# Hello World

Some body text with **bold** and *italic* words.

## Section One

Paragraph under section one.

### Subsection 1.1

Details here.

## Section Two

Another paragraph.

```rust
fn main() {
    println!(\"hello\");
}
```

| Key | Value |
|-----|-------|
| a   | 1     |
| b   | 2     |
";

    const SAMPLE_WITH_META: &str = "\
---
title: Test Doc
author: Tester
---

# Title

Body text.
";

    // --- Parsing ---

    #[test]
    fn parse_basic_structure() {
        let blocks = parse_markdown(SAMPLE_MD);
        // Should have: H1, Paragraph, H2, Paragraph, H3, Paragraph, H2, Paragraph, CodeBlock, Table
        assert!(blocks.len() >= 8, "expected at least 8 blocks, got {}", blocks.len());
        assert!(matches!(blocks[0], Block::Heading { level: HeadingLevel::H1, .. }));
    }

    #[test]
    fn parse_inline_styles() {
        let blocks = parse_markdown("Hello **bold** and *italic* and `code`.");
        assert_eq!(blocks.len(), 1);
        if let Block::Paragraph { spans } = &blocks[0] {
            let styles: Vec<_> = spans.iter().map(|s| &s.style).collect();
            assert!(styles.contains(&&SpanStyle::Bold));
            assert!(styles.contains(&&SpanStyle::Italic));
            assert!(styles.contains(&&SpanStyle::Code));
        } else {
            panic!("expected Paragraph");
        }
    }

    #[test]
    fn parse_metadata_block() {
        let blocks = parse_markdown(SAMPLE_WITH_META);
        assert!(matches!(&blocks[0], Block::Metadata { entries } if entries.len() == 2));
        if let Block::Metadata { entries } = &blocks[0] {
            assert_eq!(entries[0], ("title".to_string(), "Test Doc".to_string()));
            assert_eq!(entries[1], ("author".to_string(), "Tester".to_string()));
        }
    }

    #[test]
    fn parse_code_block() {
        let blocks = parse_markdown("```\nline1\nline2\n```\n");
        assert_eq!(blocks.len(), 1);
        if let Block::CodeBlock { text } = &blocks[0] {
            assert!(text.contains("line1"));
            assert!(text.contains("line2"));
        } else {
            panic!("expected CodeBlock");
        }
    }

    #[test]
    fn parse_table() {
        let blocks = parse_markdown("| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n");
        assert_eq!(blocks.len(), 1);
        if let Block::Table { headers, rows } = &blocks[0] {
            assert_eq!(headers.len(), 2);
            assert_eq!(rows.len(), 2);
        } else {
            panic!("expected Table");
        }
    }

    // --- Headings ---

    #[test]
    fn heading_numbering() {
        let blocks = parse_markdown(SAMPLE_MD);
        let headings = build_headings(&blocks);
        assert_eq!(headings.len(), 4); // H1, H2, H3, H2
        assert_eq!(headings[0].number, "1.");
        assert_eq!(headings[1].number, "1.1.");
        assert_eq!(headings[2].number, "1.1.1.");
        assert_eq!(headings[3].number, "1.2.");
    }

    #[test]
    fn heading_level_indices() {
        assert_eq!(heading_level_index(&HeadingLevel::H1), 0);
        assert_eq!(heading_level_index(&HeadingLevel::H3), 2);
        assert_eq!(heading_level_index(&HeadingLevel::H6), 5);
    }

    // --- Folding ---

    #[test]
    fn fold_hides_children() {
        let blocks = parse_markdown(SAMPLE_MD);
        let mut headings = build_headings(&blocks);
        // Fold the first H2 ("Section One" at index 1)
        headings[1].folded = true;
        let h2_bi = headings[1].block_index;
        let h3_bi = headings[2].block_index; // subsection is inside

        // Block right after the folded H2 should be hidden
        assert!(is_block_folded(h2_bi + 1, &headings));
        // The H3 inside should also be hidden
        assert!(is_block_folded(h3_bi, &headings));
        // The second H2 should NOT be hidden (same level = new section)
        assert!(!is_block_folded(headings[3].block_index, &headings));
    }

    #[test]
    fn fold_h1_hides_everything() {
        let blocks = parse_markdown(SAMPLE_MD);
        let mut headings = build_headings(&blocks);
        headings[0].folded = true;
        // Everything after H1 should be folded
        for bi in (headings[0].block_index + 1)..blocks.len() {
            assert!(is_block_folded(bi, &headings), "block {} should be folded", bi);
        }
    }

    // --- Rendering pipeline ---

    #[test]
    fn render_produces_valid_image() {
        let fonts = test_fonts();
        let blocks = parse_markdown(SAMPLE_MD);
        let mut headings = build_headings(&blocks);
        let (img, positions, margin_left) = render_markdown(&blocks, &mut headings, 800, &fonts);

        assert!(img.width() == 800);
        assert!(img.height() > 100, "image too short: {}", img.height());
        assert!(!positions.is_empty());
        assert!(margin_left > 0);
    }

    #[test]
    fn render_headings_have_positions() {
        let fonts = test_fonts();
        let blocks = parse_markdown(SAMPLE_MD);
        let mut headings = build_headings(&blocks);
        render_markdown(&blocks, &mut headings, 800, &fonts);

        // All headings should have y_pos set (monotonically increasing)
        for i in 1..headings.len() {
            assert!(
                headings[i].y_pos > headings[i - 1].y_pos,
                "heading {} y_pos ({}) should be > heading {} y_pos ({})",
                i, headings[i].y_pos, i - 1, headings[i - 1].y_pos,
            );
        }
        // All should have nonzero heading_height
        for h in &headings {
            assert!(h.heading_height > 0);
        }
    }

    #[test]
    fn render_folded_is_shorter() {
        let fonts = test_fonts();
        let blocks = parse_markdown(SAMPLE_MD);

        let mut headings_open = build_headings(&blocks);
        let (img_open, _, _) = render_markdown(&blocks, &mut headings_open, 800, &fonts);

        let mut headings_folded = build_headings(&blocks);
        headings_folded[0].folded = true;
        let (img_folded, _, _) = render_markdown(&blocks, &mut headings_folded, 800, &fonts);

        assert!(
            img_folded.height() < img_open.height(),
            "folded ({}) should be shorter than open ({})",
            img_folded.height(), img_open.height(),
        );
    }

    // --- AppState integration ---

    #[test]
    fn app_state_navigation() {
        let fonts = test_fonts();
        let mut state = AppState::new(SAMPLE_MD, &fonts, 800, 600);

        assert_eq!(state.current_heading, Some(0));
        assert!(state.navigate_heading(1));
        assert_eq!(state.current_heading, Some(1));
        assert!(state.navigate_heading(1));
        assert_eq!(state.current_heading, Some(2));
        // Navigate back
        assert!(state.navigate_heading(-1));
        assert_eq!(state.current_heading, Some(1));
    }

    #[test]
    fn app_state_navigation_bounds() {
        let fonts = test_fonts();
        let mut state = AppState::new(SAMPLE_MD, &fonts, 800, 600);

        // Can't go before first heading
        assert!(!state.navigate_heading(-1));
        assert_eq!(state.current_heading, Some(0));

        // Navigate to last, then can't go further
        while state.navigate_heading(1) {}
        let last = state.current_heading.unwrap();
        assert!(!state.navigate_heading(1));
        assert_eq!(state.current_heading, Some(last));
    }

    #[test]
    fn app_state_fold_toggle() {
        let fonts = test_fonts();
        let mut state = AppState::new(SAMPLE_MD, &fonts, 800, 600);
        let h_before = state.img.height();

        state.toggle_fold(&fonts);
        assert!(state.headings[0].folded);
        assert!(state.img.height() < h_before);

        state.toggle_fold(&fonts);
        assert!(!state.headings[0].folded);
        assert_eq!(state.img.height(), h_before);
    }

    #[test]
    fn app_state_scroll() {
        let fonts = test_fonts();
        let mut state = AppState::new(SAMPLE_MD, &fonts, 800, 200); // small viewport

        assert_eq!(state.scroll_y, 0);
        assert!(state.scroll(100));
        assert_eq!(state.scroll_y, 100);
        // Can't scroll past max
        state.scroll(999999);
        assert!(state.scroll_y <= state.max_scroll());
        // Can't scroll negative
        assert!(!state.scroll(-999999) || state.scroll_y == 0);
    }

    #[test]
    fn app_state_cursor_info() {
        let fonts = test_fonts();
        let state = AppState::new(SAMPLE_MD, &fonts, 800, 600);
        let ci = state.cursor_info();
        assert!(ci.is_some());
        let (x, y, h, color) = ci.unwrap();
        assert!(x < 800);
        assert!(h > 0);
        // Cursor color should match theme
        assert_eq!(color, state.theme.cursor_color.0);
        let _ = y; // just verify it exists
    }

    // --- Search ---

    #[test]
    fn search_finds_text() {
        let fonts = test_fonts();
        let mut state = AppState::new(SAMPLE_MD, &fonts, 800, 600);

        state.search_query = "bold".to_string();
        state.execute_search(&fonts);
        assert!(!state.search_matches.is_empty(), "should find 'bold'");
        assert!(!state.search_highlights.is_empty());
    }

    #[test]
    fn search_case_insensitive() {
        let fonts = test_fonts();
        let mut state = AppState::new(SAMPLE_MD, &fonts, 800, 600);

        state.search_query = "HELLO".to_string();
        state.execute_search(&fonts);
        assert!(!state.search_matches.is_empty(), "should find 'HELLO' case-insensitively");
    }

    #[test]
    fn search_no_match() {
        let fonts = test_fonts();
        let mut state = AppState::new(SAMPLE_MD, &fonts, 800, 600);

        state.search_query = "zzzznonexistent".to_string();
        state.execute_search(&fonts);
        assert!(state.search_matches.is_empty());
        assert!(state.search_highlights.is_empty());
    }

    #[test]
    fn search_navigation_wraps() {
        let fonts = test_fonts();
        let mut state = AppState::new(SAMPLE_MD, &fonts, 800, 600);

        state.search_query = "section".to_string();
        state.execute_search(&fonts);
        if state.search_matches.len() >= 2 {
            let first = state.search_current;
            state.navigate_search(true);
            assert_ne!(state.search_current, first);
            // Wrap backwards past 0
            state.search_current = 0;
            state.navigate_search(false);
            assert_eq!(state.search_current, state.search_matches.len() - 1);
        }
    }

    #[test]
    fn search_in_code_block() {
        let fonts = test_fonts();
        let mut state = AppState::new(SAMPLE_MD, &fonts, 800, 600);

        state.search_query = "println".to_string();
        state.execute_search(&fonts);
        assert!(!state.search_matches.is_empty(), "should find text in code blocks");
    }

    #[test]
    fn search_in_table() {
        let fonts = test_fonts();
        let md = "| Name | Age |\n|------|-----|\n| Alice | 30 |\n";
        let mut state = AppState::new(md, &fonts, 800, 600);

        state.search_query = "alice".to_string();
        state.execute_search(&fonts);
        assert!(!state.search_matches.is_empty(), "should find text in tables");
    }

    // --- Word wrapping ---

    #[test]
    fn wrap_spans_respects_width() {
        let fonts = test_fonts();
        let scale = PxScale::from(18.0);
        let spans = vec![Span {
            text: "This is a fairly long sentence that should wrap at some point when rendered".to_string(),
            style: SpanStyle::Normal,
        }];
        let lines = wrap_spans(&spans, &fonts, scale, 200);
        assert!(lines.len() > 1, "should wrap into multiple lines for narrow width");

        let lines_wide = wrap_spans(&spans, &fonts, scale, 2000);
        assert_eq!(lines_wide.len(), 1, "should fit in one line for wide width");
    }

    // --- Metadata parsing ---

    #[test]
    fn metadata_missing() {
        let (entries, rest) = parse_metadata("# Just a heading\n");
        assert!(entries.is_empty());
        assert_eq!(rest, "# Just a heading\n");
    }

    #[test]
    fn metadata_unclosed() {
        let (entries, rest) = parse_metadata("---\nkey: val\nno closing fence\n");
        assert!(entries.is_empty());
        assert!(rest.contains("key: val"));
    }

    // --- Display viewport ---

    #[test]
    fn display_viewport_does_not_panic() {
        let fonts = test_fonts();
        let mut state = AppState::new(SAMPLE_MD, &fonts, 800, 600);
        let mut buf: Vec<u8> = Vec::new();
        let ci = state.cursor_info();
        let result = display_viewport(
            &mut buf, &state.img, state.scroll_y,
            state.vp_width, state.vp_height, &mut state.frame,
            None, ci, &state.search_highlights, state.search_current,
        );
        assert!(result.is_ok());
        assert!(!buf.is_empty(), "should produce kitty protocol output");
    }

    #[test]
    fn display_viewport_with_search_overlay() {
        let fonts = test_fonts();
        let mut state = AppState::new(SAMPLE_MD, &fonts, 800, 600);
        state.search_query = "bold".to_string();
        state.execute_search(&fonts);

        let search_bar = render_search_bar(
            &state.search_query,
            Some((1, state.search_matches.len())),
            800, &fonts,
        );
        let mut buf: Vec<u8> = Vec::new();
        let ci = state.cursor_info();
        let result = display_viewport(
            &mut buf, &state.img, state.scroll_y,
            state.vp_width, state.vp_height, &mut state.frame,
            Some(&search_bar), ci,
            &state.search_highlights, state.search_current,
        );
        assert!(result.is_ok());
    }

    // --- Resize ---

    #[test]
    fn render_at_different_widths() {
        let fonts = test_fonts();
        for width in [400, 800, 1200, 1920] {
            let blocks = parse_markdown(SAMPLE_MD);
            let mut headings = build_headings(&blocks);
            let (img, _, _) = render_markdown(&blocks, &mut headings, width, &fonts);
            assert_eq!(img.width(), width);
            assert!(img.height() > 0);
        }
    }

    // --- Edge cases ---

    #[test]
    fn empty_document() {
        let fonts = test_fonts();
        let state = AppState::new("", &fonts, 800, 600);
        assert!(state.headings.is_empty());
        assert!(state.current_heading.is_none());
        assert!(state.img.height() > 0);
    }

    #[test]
    fn headings_only() {
        let fonts = test_fonts();
        let md = "# One\n## Two\n## Three\n";
        let state = AppState::new(md, &fonts, 800, 600);
        assert_eq!(state.headings.len(), 3);
    }

    #[test]
    fn navigation_skips_folded_headings() {
        let fonts = test_fonts();
        let md = "# Top\n## A\n### A.1\n## B\n";
        let mut state = AppState::new(md, &fonts, 800, 600);

        // Fold "## A" (index 1) — should hide "### A.1" (index 2)
        state.current_heading = Some(1);
        state.toggle_fold(&fonts);

        // Navigate forward from "## A" should skip "### A.1" and go to "## B"
        state.navigate_heading(1);
        assert_eq!(state.current_heading, Some(3));
    }
}
