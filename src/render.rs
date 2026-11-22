use ab_glyph::PxScale;
use image::{Rgb, RgbImage};
use imageproc::drawing::{draw_filled_rect_mut, draw_line_segment_mut, draw_text_mut, text_size};
use imageproc::rect::Rect;
use pulldown_cmark::HeadingLevel;

use crate::constants::{LayoutParams, LIST_INDENT_PER_LEVEL};
use crate::fonts::Fonts;
use crate::headings::is_block_folded;
use crate::text::{draw_spans, spans_to_plain, wrap_spans};
use crate::theme::Theme;
use crate::types::{Block, HeadingInfo, ListItem, ListMarker, Span, SpanStyle};

const PREVIEW_MARGIN: u32 = 10;

pub(crate) fn render_preview(
    blocks: &[Block],
    headings: &[HeadingInfo],
    width: u32,
    max_height: u32,
    fonts: &Fonts,
    theme: &Theme,
    layout: &LayoutParams,
    mermaid_cache: &mut crate::mermaid::MermaidCache,
) -> RgbImage {
    let content_width = (width - PREVIEW_MARGIN * 2).min(layout.max_content_width);
    let margin_left = PREVIEW_MARGIN;

    let mut img = RgbImage::from_pixel(width, max_height, theme.bg);
    let mut y: u32 = layout.paragraph_gap;
    let mut heading_idx: usize = 0;

    for (_bi, block) in blocks.iter().enumerate() {
        if y >= max_height {
            break;
        }

        match block {
            Block::Metadata { entries } => {
                y = render_metadata(&mut img, entries, fonts, theme, y, margin_left + layout.block_indent, content_width - layout.block_indent);
                y += layout.paragraph_gap * 2;
            }
            Block::Heading { level, spans } => {
                if matches!(level, HeadingLevel::H1) {
                    y += layout.h1_extra_margin;
                }

                let hi = heading_idx;
                heading_idx += 1;
                if hi >= headings.len() {
                    continue;
                }

                let (lines, size, line_height) = wrap_heading_text(
                    &headings[hi], spans, fonts, theme, content_width,
                );
                let (_, color) = heading_style(level, theme);
                let scale = PxScale::from(size);

                for line in &lines {
                    if y >= max_height {
                        break;
                    }
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
                y += layout.paragraph_gap;
            }
            Block::Paragraph { spans } => {
                let scale = PxScale::from(theme.body_size);
                let indented_width = content_width - layout.block_indent;
                let lines = wrap_spans(spans, fonts, scale, indented_width);
                let line_height = (theme.body_size * 1.4) as u32;

                for line in &lines {
                    if y >= max_height {
                        break;
                    }
                    draw_spans(&mut img, line, margin_left + layout.block_indent, y, scale, fonts, theme);
                    y += line_height;
                }
                y += layout.paragraph_gap;
            }
            Block::CodeBlock { text } => {
                y = render_code_block(&mut img, text, fonts, theme, y, content_width - layout.block_indent, margin_left + layout.block_indent);
                y += layout.paragraph_gap;
            }
            Block::Table { headers, rows } => {
                y = render_table(&mut img, headers, rows, fonts, theme, y, content_width - layout.block_indent, margin_left + layout.block_indent);
                y += layout.paragraph_gap * 2;
            }
            Block::List { items } => {
                y = render_list(&mut img, items, fonts, theme, y, content_width, margin_left, layout);
                y += layout.paragraph_gap;
            }
            Block::Mermaid { source } => {
                let indented_width = content_width - layout.block_indent;
                let dark = crate::mermaid::is_dark_bg(theme.bg);
                let diagram = mermaid_cache.get_or_render(
                    source, indented_width, dark, fonts, theme,
                );
                let dw = diagram.width();
                let dh = diagram.height();
                let x_off = indented_width.saturating_sub(dw) / 2;
                let x = margin_left + layout.block_indent + x_off;
                image::imageops::overlay(&mut img, &*diagram, x as i64, y as i64);
                y += dh + layout.paragraph_gap;
            }
        }
    }

    img
}

/// Vertical slot of one rendered (unfolded) block. A slot runs from `y` to
/// the next block's `y`, so it includes the block's trailing gap; an H1's
/// extra top margin belongs to its own slot. The last slot runs to the end of
/// the content height.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BlockLayout {
    pub(crate) block_index: usize,
    /// Index into `headings` when the block is a heading.
    pub(crate) heading_index: Option<usize>,
    pub(crate) y: u32,
    pub(crate) height: u32,
}

/// Column geometry for a render width: `(content_width, margin_left)`.
fn content_geometry(width: u32, layout: &LayoutParams) -> (u32, u32) {
    let content_width = (width - layout.margin_left - layout.margin_right).min(layout.max_content_width);
    (content_width, (width - content_width) / 2)
}

/// Lay out the unfolded blocks without drawing: each block's slot, and the
/// content height (without the TUI's `vp_height / 2` bottom padding).
pub(crate) fn layout_blocks(
    blocks: &[Block],
    headings: &[HeadingInfo],
    width: u32,
    fonts: &Fonts,
    theme: &Theme,
    layout: &LayoutParams,
    mermaid_cache: &mut crate::mermaid::MermaidCache,
) -> (Vec<BlockLayout>, u32) {
    let (content_width, _) = content_geometry(width, layout);
    let mut y: u32 = layout.paragraph_gap;
    // `compute_table_height` measures 2 px more than `render_table` advances;
    // the page height keeps those pixels (at the end) so TUI heights stay as
    // they always were.
    let mut slack: u32 = 0;
    let mut heading_idx: usize = 0;
    let mut out: Vec<BlockLayout> = Vec::new();

    for (bi, block) in blocks.iter().enumerate() {
        let heading_index = if matches!(block, Block::Heading { .. }) {
            heading_idx += 1;
            Some(heading_idx - 1)
        } else {
            None
        };
        if is_block_folded(bi, headings) {
            continue;
        }

        let advance = match block {
            Block::Metadata { entries } => {
                let scale = PxScale::from(theme.body_size);
                let line_height = (theme.body_size * 1.5) as u32;
                let indented_width = content_width - layout.block_indent;
                let total_lines: u32 = entries.iter().map(|(k, v)| {
                    metadata_entry_lines(k, v, fonts, scale, indented_width).len() as u32
                }).sum();
                total_lines * line_height + layout.paragraph_gap * 2
            }
            Block::Heading { level, spans } => {
                let margin = if matches!(level, HeadingLevel::H1) { layout.h1_extra_margin } else { 0 };
                let hi = heading_index.expect("heading block has an index");
                let (lines, _, line_height) = wrap_heading_text(
                    &headings[hi], spans, fonts, theme, content_width,
                );
                margin + lines.len() as u32 * line_height + layout.paragraph_gap
            }
            Block::Paragraph { spans } => {
                let scale = PxScale::from(theme.body_size);
                let indented_width = content_width - layout.block_indent;
                let lines = wrap_spans(spans, fonts, scale, indented_width);
                let line_height = (theme.body_size * 1.4) as u32;
                lines.len() as u32 * line_height + layout.paragraph_gap
            }
            Block::CodeBlock { text } => {
                let scale = PxScale::from(theme.body_size);
                let line_height = (theme.body_size * 1.4) as u32;
                let indented_width = content_width - layout.block_indent;
                let wrapped = wrap_code_lines(text, fonts, scale, indented_width - 20);
                let total_lines = wrapped.iter().map(|w| w.len() as u32).sum::<u32>().max(1);
                total_lines * line_height + 20 + layout.paragraph_gap
            }
            Block::Table { headers, rows } => {
                slack += 2;
                compute_table_height(headers, rows, fonts, theme, content_width - layout.block_indent) - 2
                    + layout.paragraph_gap * 2
            }
            Block::List { items } => {
                compute_list_height(items, fonts, theme, content_width, layout) + layout.paragraph_gap
            }
            Block::Mermaid { source } => {
                let indented_width = content_width - layout.block_indent;
                let dark = crate::mermaid::is_dark_bg(theme.bg);
                let diagram = mermaid_cache.get_or_render(
                    source, indented_width, dark, fonts, theme,
                );
                diagram.height() + layout.paragraph_gap
            }
        };
        out.push(BlockLayout { block_index: bi, heading_index, y, height: advance });
        y += advance;
    }

    let content_height = y + slack + layout.paragraph_gap;
    if let Some(last) = out.last_mut() {
        last.height = content_height - last.y;
    }
    (out, content_height)
}

/// Draw one block at `y` (the top of its slot) onto `img`; returns the y
/// where the next block starts. For a heading, records its position in
/// `heading` as `y_base + ` its y in `img`.
fn draw_block(
    img: &mut RgbImage,
    block: &Block,
    heading: Option<&mut HeadingInfo>,
    y: u32,
    y_base: u32,
    width: u32,
    fonts: &Fonts,
    theme: &Theme,
    layout: &LayoutParams,
    mermaid_cache: &mut crate::mermaid::MermaidCache,
    fold_arrows: bool,
) -> u32 {
    let (content_width, margin_left) = content_geometry(width, layout);
    let mut y = y;
    match block {
        Block::Metadata { entries } => {
            y = render_metadata(img, entries, fonts, theme, y, margin_left + layout.block_indent, content_width - layout.block_indent);
            y += layout.paragraph_gap * 2;
        }
        Block::Heading { level, spans } => {
            if matches!(level, HeadingLevel::H1) {
                y += layout.h1_extra_margin;
            }
            let heading = heading.expect("heading block needs its HeadingInfo");

            let (lines, size, line_height) = wrap_heading_text(
                heading, spans, fonts, theme, content_width,
            );
            let (_, color) = heading_style(level, theme);
            let scale = PxScale::from(size);
            let heading_total_h = lines.len() as u32 * line_height;

            heading.y_pos = y_base + y;
            heading.heading_height = heading_total_h;

            if fold_arrows {
                let fold_char = if heading.folded { "▶" } else { "▼" };
                let arrow_scale = PxScale::from(size * 0.5);
                let arrow_w = text_size(arrow_scale, &fonts.bold, fold_char).0;
                let arrow_x = margin_left as i32 - arrow_w as i32 - 4;

                let arrow_y_offset = ((size - size * 0.5) * 0.5) as i32;
                draw_text_mut(
                    img,
                    color,
                    arrow_x,
                    y as i32 + arrow_y_offset,
                    arrow_scale,
                    &fonts.bold,
                    fold_char,
                );
            }

            for line in &lines {
                for span in line {
                    draw_text_mut(
                        img,
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
            y += layout.paragraph_gap;
        }
        Block::Paragraph { spans } => {
            let scale = PxScale::from(theme.body_size);
            let indented_width = content_width - layout.block_indent;
            let lines = wrap_spans(spans, fonts, scale, indented_width);
            let line_height = (theme.body_size * 1.4) as u32;

            for line in &lines {
                draw_spans(img, line, margin_left + layout.block_indent, y, scale, fonts, theme);
                y += line_height;
            }
            y += layout.paragraph_gap;
        }
        Block::CodeBlock { text } => {
            y = render_code_block(img, text, fonts, theme, y, content_width - layout.block_indent, margin_left + layout.block_indent);
            y += layout.paragraph_gap;
        }
        Block::Table { headers, rows } => {
            y = render_table(img, headers, rows, fonts, theme, y, content_width - layout.block_indent, margin_left + layout.block_indent);
            y += layout.paragraph_gap * 2;
        }
        Block::List { items } => {
            y = render_list(img, items, fonts, theme, y, content_width, margin_left, layout);
            y += layout.paragraph_gap;
        }
        Block::Mermaid { source } => {
            let indented_width = content_width - layout.block_indent;
            let dark = crate::mermaid::is_dark_bg(theme.bg);
            let diagram = mermaid_cache.get_or_render(
                source, indented_width, dark, fonts, theme,
            );
            let dw = diagram.width();
            let dh = diagram.height();
            // Center within the indented column.
            let x_off = indented_width.saturating_sub(dw) / 2;
            let x = margin_left + layout.block_indent + x_off;
            image::imageops::overlay(img, &*diagram, x as i64, y as i64);
            y += dh + layout.paragraph_gap;
        }
    }
    y
}

pub(crate) fn render_markdown(
    blocks: &[Block],
    headings: &mut [HeadingInfo],
    width: u32,
    vp_height: u32,
    fonts: &Fonts,
    theme: &Theme,
    layout: &LayoutParams,
    mermaid_cache: &mut crate::mermaid::MermaidCache,
    fold_arrows: bool,
) -> (RgbImage, Vec<(usize, u32)>, u32) {
    let (_, margin_left) = content_geometry(width, layout);
    let (layouts, content_height) = layout_blocks(blocks, headings, width, fonts, theme, layout, mermaid_cache);
    let total_height = content_height + vp_height / 2;

    let mut img = RgbImage::from_pixel(width, total_height.max(1), theme.bg);
    for l in &layouts {
        let heading = l.heading_index.map(|hi| &mut headings[hi]);
        let end = draw_block(
            &mut img, &blocks[l.block_index], heading, l.y, 0, width, fonts, theme, layout,
            mermaid_cache, fold_arrows,
        );
        debug_assert!(end <= l.y + l.height, "block {} drew past its slot", l.block_index);
    }
    let block_positions = layouts.iter().map(|l| (l.block_index, l.y)).collect();

    (img, block_positions, margin_left)
}

/// Render the page rows `[y0, y0 + height)` (full render width), drawing only
/// the blocks whose slot overlaps them. Pixel-identical to the same rows of
/// `render_markdown` with `vp_height = 0`; rows past the content are `bg`.
#[allow(dead_code)] // used by serve mode (protocol v2)
pub(crate) fn render_region(
    blocks: &[Block],
    headings: &mut [HeadingInfo],
    width: u32,
    fonts: &Fonts,
    theme: &Theme,
    layout: &LayoutParams,
    mermaid_cache: &mut crate::mermaid::MermaidCache,
    fold_arrows: bool,
    layouts: &[BlockLayout],
    y0: u32,
    height: u32,
) -> RgbImage {
    let mut img = RgbImage::from_pixel(width, height, theme.bg);
    let y1 = y0 + height;
    let row_bytes = width as usize * 3;
    // Slots are sorted by y: skip to the first one that ends after y0.
    let first = layouts.partition_point(|l| l.y + l.height <= y0);
    for l in &layouts[first..] {
        if l.y >= y1 {
            break;
        }
        if l.height == 0 {
            continue;
        }
        let mut scratch = RgbImage::from_pixel(width, l.height, theme.bg);
        let heading = l.heading_index.map(|hi| &mut headings[hi]);
        draw_block(
            &mut scratch, &blocks[l.block_index], heading, 0, l.y, width, fonts, theme, layout,
            mermaid_cache, fold_arrows,
        );
        let from = l.y.max(y0);
        let to = (l.y + l.height).min(y1);
        let src = &scratch.as_raw()[(from - l.y) as usize * row_bytes..(to - l.y) as usize * row_bytes];
        let dst_start = (from - y0) as usize * row_bytes;
        let dst: &mut [u8] = &mut img;
        dst[dst_start..dst_start + src.len()].copy_from_slice(src);
    }
    img
}

pub(crate) fn wrap_code_lines(text: &str, fonts: &Fonts, scale: PxScale, inner_width: u32) -> Vec<Vec<Vec<Span>>> {
    text.lines()
        .map(|line| wrap_code_line(line, fonts, scale, inner_width))
        .collect()
}

fn wrap_code_line(line: &str, fonts: &Fonts, scale: PxScale, max_width: u32) -> Vec<Vec<Span>> {
    if line.is_empty() {
        return vec![vec![Span { text: String::new(), style: SpanStyle::Code }]];
    }

    let full_w = text_size(scale, &fonts.mono, line).0;
    if full_w <= max_width {
        return vec![vec![Span { text: line.to_string(), style: SpanStyle::Code }]];
    }

    // Character-wrap, preserving whitespace exactly.
    let mut out: Vec<Vec<Span>> = Vec::new();
    let mut current = String::new();
    let mut current_w: u32 = 0;
    let mut buf = [0u8; 4];
    for ch in line.chars() {
        let s = ch.encode_utf8(&mut buf);
        let w = text_size(scale, &fonts.mono, s).0;
        if current_w > 0 && current_w + w > max_width {
            out.push(vec![Span { text: std::mem::take(&mut current), style: SpanStyle::Code }]);
            current_w = 0;
        }
        current.push(ch);
        current_w += w;
    }
    if !current.is_empty() {
        out.push(vec![Span { text: current, style: SpanStyle::Code }]);
    }
    out
}

pub(crate) fn wrap_heading_text(
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

pub(crate) fn heading_style(level: &HeadingLevel, theme: &Theme) -> (f32, Rgb<u8>) {
    match level {
        HeadingLevel::H1 => (theme.h1_size, theme.h1_color),
        HeadingLevel::H2 => (theme.h2_size, theme.h2_color),
        _ => (theme.h3_size, theme.h3_color),
    }
}

fn wrap_plain_text(text: &str, font: &ab_glyph::FontVec, scale: PxScale, max_width: u32) -> Vec<String> {
    let space_w = text_size(scale, font, " ").0;
    let mut lines: Vec<String> = Vec::new();
    let mut current_line = String::new();
    let mut current_width: u32 = 0;

    for word in text.split_whitespace() {
        let word_w = text_size(scale, font, word).0;
        let needed = if current_line.is_empty() { 0 } else { space_w } + word_w;
        if !current_line.is_empty() && current_width + needed > max_width {
            lines.push(std::mem::take(&mut current_line));
            current_width = 0;
        }
        if !current_line.is_empty() {
            current_line.push(' ');
            current_width += space_w;
        }
        current_line.push_str(word);
        current_width += word_w;
    }
    if !current_line.is_empty() {
        lines.push(current_line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn metadata_entry_lines(
    key: &str,
    val: &str,
    fonts: &Fonts,
    scale: PxScale,
    content_width: u32,
) -> Vec<String> {
    let key_text = format!("{}: ", key);
    let key_w = text_size(scale, &fonts.bold, &key_text).0;
    let val_width = content_width.saturating_sub(key_w);
    wrap_plain_text(val, &fonts.regular, scale, val_width)
}

fn render_metadata(
    img: &mut RgbImage,
    entries: &[(String, String)],
    fonts: &Fonts,
    theme: &Theme,
    start_y: u32,
    margin_left: u32,
    content_width: u32,
) -> u32 {
    let scale = PxScale::from(theme.body_size);
    let line_height = (theme.body_size * 1.5) as u32;
    let mut y = start_y;

    for (key, val) in entries {
        let key_text = format!("{}: ", key);
        let key_w = text_size(scale, &fonts.bold, &key_text).0;
        let val_lines = metadata_entry_lines(key, val, fonts, scale, content_width);

        draw_text_mut(
            img,
            theme.meta_key_color,
            margin_left as i32,
            y as i32,
            scale,
            &fonts.bold,
            &key_text,
        );

        for line in &val_lines {
            draw_text_mut(
                img,
                theme.meta_val_color,
                (margin_left + key_w) as i32,
                y as i32,
                scale,
                &fonts.regular,
                line,
            );
            y += line_height;
        }
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

fn marker_text(marker: &ListMarker) -> String {
    match marker {
        ListMarker::Bullet => "\u{2022}  ".to_string(),
        ListMarker::Ordered(n) => format!("{}. ", n),
    }
}

fn compute_list_height(
    items: &[ListItem],
    fonts: &Fonts,
    theme: &Theme,
    content_width: u32,
    layout: &LayoutParams,
) -> u32 {
    let scale = PxScale::from(theme.body_size);
    let line_height = (theme.body_size * 1.4) as u32;
    let mut h: u32 = 0;

    for item in items {
        let indent = layout.block_indent + item.depth * LIST_INDENT_PER_LEVEL;
        let mt = marker_text(&item.marker);
        let marker_w = text_size(scale, &fonts.regular, &mt).0;
        let text_width = content_width.saturating_sub(indent + marker_w);
        let lines = wrap_spans(&item.spans, fonts, scale, text_width);
        h += lines.len().max(1) as u32 * line_height;
    }

    h
}

fn render_list(
    img: &mut RgbImage,
    items: &[ListItem],
    fonts: &Fonts,
    theme: &Theme,
    start_y: u32,
    content_width: u32,
    margin_left: u32,
    layout: &LayoutParams,
) -> u32 {
    let scale = PxScale::from(theme.body_size);
    let line_height = (theme.body_size * 1.4) as u32;
    let mut y = start_y;

    for item in items {
        let indent = layout.block_indent + item.depth * LIST_INDENT_PER_LEVEL;
        let x = margin_left + indent;
        let mt = marker_text(&item.marker);
        let marker_w = text_size(scale, &fonts.regular, &mt).0;

        draw_text_mut(
            img,
            theme.body_color,
            x as i32,
            y as i32,
            scale,
            &fonts.regular,
            &mt,
        );

        let text_width = content_width.saturating_sub(indent + marker_w);
        let lines = wrap_spans(&item.spans, fonts, scale, text_width);
        for (li, line) in lines.iter().enumerate() {
            let lx = if li == 0 { x + marker_w } else { x + marker_w };
            draw_spans(img, line, lx, y, scale, fonts, theme);
            y += line_height;
        }
        if lines.is_empty() {
            y += line_height;
        }
    }

    y
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headings::build_headings;
    use crate::mermaid::MermaidCache;
    use crate::parsing::parse_markdown;
    use crate::test_helpers::{test_fonts, SAMPLE_MD};
    use crate::theme::default_theme;

    #[test]
    fn render_produces_valid_image() {
        let fonts = test_fonts();
        let theme = default_theme();
        let layout = LayoutParams::default();
        let (blocks, _) = parse_markdown(SAMPLE_MD);
        let mut headings = build_headings(&blocks);
        let mut mermaid_cache = MermaidCache::new();
        let (img, positions, margin_left) = render_markdown(&blocks, &mut headings, 800, 600, &fonts, &theme, &layout, &mut mermaid_cache, true);

        assert!(img.width() == 800);
        assert!(img.height() > 100, "image too short: {}", img.height());
        assert!(!positions.is_empty());
        assert!(margin_left > 0);
    }

    #[test]
    fn render_headings_have_positions() {
        let fonts = test_fonts();
        let theme = default_theme();
        let layout = LayoutParams::default();
        let (blocks, _) = parse_markdown(SAMPLE_MD);
        let mut headings = build_headings(&blocks);
        let mut mermaid_cache = MermaidCache::new();
        render_markdown(&blocks, &mut headings, 800, 600, &fonts, &theme, &layout, &mut mermaid_cache, true);

        for i in 1..headings.len() {
            assert!(
                headings[i].y_pos > headings[i - 1].y_pos,
                "heading {} y_pos ({}) should be > heading {} y_pos ({})",
                i, headings[i].y_pos, i - 1, headings[i - 1].y_pos,
            );
        }
        for h in &headings {
            assert!(h.heading_height > 0);
        }
    }

    #[test]
    fn render_folded_is_shorter() {
        let fonts = test_fonts();
        let theme = default_theme();
        let layout = LayoutParams::default();
        let (blocks, _) = parse_markdown(SAMPLE_MD);

        let mut headings_open = build_headings(&blocks);
        let mut mermaid_cache = MermaidCache::new();
        let (img_open, _, _) = render_markdown(&blocks, &mut headings_open, 800, 600, &fonts, &theme, &layout, &mut mermaid_cache, true);

        let mut headings_folded = build_headings(&blocks);
        headings_folded[0].folded = true;
        let mut mermaid_cache = MermaidCache::new();
        let (img_folded, _, _) = render_markdown(&blocks, &mut headings_folded, 800, 600, &fonts, &theme, &layout, &mut mermaid_cache, true);

        assert!(
            img_folded.height() < img_open.height(),
            "folded ({}) should be shorter than open ({})",
            img_folded.height(), img_open.height(),
        );
    }

    #[test]
    fn render_preview_produces_valid_image() {
        let fonts = test_fonts();
        let theme = default_theme();
        let layout = LayoutParams::default();
        let (blocks, _) = parse_markdown(SAMPLE_MD);
        let headings = build_headings(&blocks);
        let mut mermaid_cache = MermaidCache::new();
        let img = render_preview(&blocks, &headings, 800, 600, &fonts, &theme, &layout, &mut mermaid_cache);

        assert_eq!(img.width(), 800);
        assert_eq!(img.height(), 600);
        let bg_pixel = theme.bg;
        let has_content = img.pixels().any(|p| *p != bg_pixel);
        assert!(has_content, "preview image should not be entirely blank");
    }

    #[test]
    fn render_list_smoke_test() {
        let fonts = test_fonts();
        let theme = default_theme();
        let layout = LayoutParams::default();
        let md = "- Item one\n- Item two\n\n1. First\n2. Second\n";
        let (blocks, _) = parse_markdown(md);
        let mut headings = build_headings(&blocks);
        let mut mermaid_cache = MermaidCache::new();
        let (img, _, _) = render_markdown(&blocks, &mut headings, 800, 600, &fonts, &theme, &layout, &mut mermaid_cache, true);
        assert!(img.height() > 100);
    }

    #[test]
    fn render_at_different_widths() {
        let fonts = test_fonts();
        let theme = default_theme();
        let layout = LayoutParams::default();
        for width in [400, 800, 1200, 1920] {
            let (blocks, _) = parse_markdown(SAMPLE_MD);
            let mut headings = build_headings(&blocks);
            let mut mermaid_cache = MermaidCache::new();
            let (img, _, _) = render_markdown(&blocks, &mut headings, width, 600, &fonts, &theme, &layout, &mut mermaid_cache, true);
            assert_eq!(img.width(), width);
            assert!(img.height() > 0);
        }
    }

    #[test]
    fn render_with_mermaid_block_increases_total_height() {
        let fonts = test_fonts();
        let theme = default_theme();
        let layout = LayoutParams::default();

        let without = "# Title\n\nSome text.\n";
        let with = "# Title\n\nSome text.\n\n```mermaid\nflowchart LR\n    A --> B --> C\n```\n";

        let (b1, _) = parse_markdown(without);
        let mut h1 = build_headings(&b1);
        let mut cache1 = MermaidCache::new();
        let (img1, _, _) = render_markdown(&b1, &mut h1, 800, 600, &fonts, &theme, &layout, &mut cache1, true);

        let (b2, _) = parse_markdown(with);
        let mut h2 = build_headings(&b2);
        let mut cache2 = MermaidCache::new();
        let (img2, _, _) = render_markdown(&b2, &mut h2, 800, 600, &fonts, &theme, &layout, &mut cache2, true);

        assert!(
            img2.height() > img1.height(),
            "doc with mermaid ({}) should be taller than without ({})",
            img2.height(), img1.height(),
        );
    }

    #[test]
    fn wrap_code_lines_preserves_internal_whitespace() {
        let fonts = test_fonts();
        let scale = PxScale::from(18.0);
        // Wide enough that the line fits and no wrapping is needed.
        let wrapped = wrap_code_lines("a    b", &fonts, scale, 5000);
        assert_eq!(wrapped.len(), 1, "one source line");
        assert_eq!(wrapped[0].len(), 1, "should not soft-wrap a short line");
        let rebuilt: String = wrapped[0][0].iter().map(|s| s.text.as_str()).collect();
        assert_eq!(rebuilt, "a    b", "multiple internal spaces must be preserved");
    }

    #[test]
    fn wrap_code_lines_preserves_indent_and_trailing_spaces() {
        let fonts = test_fonts();
        let scale = PxScale::from(18.0);
        let wrapped = wrap_code_lines("    indented   ", &fonts, scale, 5000);
        let rebuilt: String = wrapped[0][0].iter().map(|s| s.text.as_str()).collect();
        assert_eq!(rebuilt, "    indented   ");
    }

    #[test]
    fn wrap_code_lines_keeps_blank_lines() {
        let fonts = test_fonts();
        let scale = PxScale::from(18.0);
        let wrapped = wrap_code_lines("a\n\nb", &fonts, scale, 5000);
        assert_eq!(wrapped.len(), 3, "three source lines including the blank");
    }

    #[test]
    fn render_mermaid_is_idempotent_second_call_uses_cache() {
        let fonts = test_fonts();
        let theme = default_theme();
        let layout = LayoutParams::default();
        let md = "# T\n\n```mermaid\nflowchart LR\n    A --> B\n```\n";
        let (blocks, _) = parse_markdown(md);
        let mut headings1 = build_headings(&blocks);
        let mut headings2 = build_headings(&blocks);
        let mut cache = MermaidCache::new();

        let (img1, _, _) = render_markdown(&blocks, &mut headings1, 800, 600, &fonts, &theme, &layout, &mut cache, true);
        let cache_size_after_first = cache.len();
        let (img2, _, _) = render_markdown(&blocks, &mut headings2, 800, 600, &fonts, &theme, &layout, &mut cache, true);

        assert_eq!(img1.height(), img2.height(), "heights must be equal across identical renders");
        assert_eq!(cache.len(), cache_size_after_first, "second render must not add cache entries");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn render_without_fold_arrows_same_size_different_margin() {
        let fonts = test_fonts();
        let theme = default_theme();
        let layout = LayoutParams::default();
        let (blocks, _) = parse_markdown(SAMPLE_MD);
        let mut h1 = build_headings(&blocks);
        let mut h2 = build_headings(&blocks);
        let mut cache = MermaidCache::new();
        let (with, _, margin_left) = render_markdown(&blocks, &mut h1, 800, 600, &fonts, &theme, &layout, &mut cache, true);
        let (without, _, _) = render_markdown(&blocks, &mut h2, 800, 600, &fonts, &theme, &layout, &mut cache, false);

        assert_eq!(with.dimensions(), without.dimensions());
        let margin_differs = (0..with.height()).any(|y| {
            (0..margin_left).any(|x| with.get_pixel(x, y) != without.get_pixel(x, y))
        });
        assert!(margin_differs, "fold arrows should be drawn in the left margin only when enabled");
        let without_margin_blank = (0..without.height())
            .all(|y| (0..margin_left).all(|x| *without.get_pixel(x, y) == theme.bg));
        assert!(without_margin_blank, "left margin should be blank without fold arrows");
    }

    #[test]
    fn total_height_without_viewport_padding_fits_content() {
        let fonts = test_fonts();
        let theme = default_theme();
        let layout = LayoutParams::default();
        let md = "# Title\n\nLast paragraph with descenders: gjpqy.\n";
        let (blocks, _) = parse_markdown(md);
        let mut headings = build_headings(&blocks);
        let mut cache = MermaidCache::new();
        let (img, positions, _) = render_markdown(&blocks, &mut headings, 800, 0, &fonts, &theme, &layout, &mut cache, false);
        let last_y = positions.last().unwrap().1;
        let line_height = (theme.body_size * 1.4) as u32;
        assert!(
            img.height() >= last_y + line_height,
            "image height {} must hold the last line starting at {}",
            img.height(), last_y,
        );
    }

    /// A long document mixing every block kind, for region rendering tests.
    pub(crate) fn long_test_doc() -> String {
        let mut s = String::from("---\ntitle: Long document\nauthor: Test\n---\n\n");
        for i in 0..18 {
            s.push_str(&format!("# Part {i}\n\nIntro paragraph {i} with **bold**, *italic* and `code` words that wrap across the line because it is long enough to wrap at least once in the column.\n\n"));
            s.push_str(&format!("## Section {i}.1\n\n```rust\nfn f{i}() {{\n    let x = {i};\n    println!(\"{{}}\", x);\n}}\n```\n\n"));
            s.push_str("| Key | Value | Notes |\n|-----|-------|-------|\n| a | 1 | first row |\n| b | 2 | second row with gjpqy |\n\n");
            s.push_str("- bullet one\n- bullet two\n  - nested gjpqy\n\n1. first\n2. second\n\n");
            if i == 5 {
                s.push_str("```mermaid\nflowchart LR\n    A --> B --> C\n```\n\n");
            }
            s.push_str(&format!("### Sub {i}.1.1\n\nClosing words with descenders: gjpqy.\n\n"));
        }
        s
    }

    /// Stack `render_region` strips of `strip_h` px and compare with the full
    /// render (vp_height = 0, no fold arrows), byte for byte.
    fn assert_regions_match_full(md: &str, strip_h: u32) {
        let fonts = test_fonts();
        let theme = default_theme();
        let layout = LayoutParams::default();
        let width = 970;
        let (blocks, _) = parse_markdown(md);
        let mut headings = build_headings(&blocks);
        let mut cache = MermaidCache::new();
        let (full, _, _) = render_markdown(&blocks, &mut headings, width, 0, &fonts, &theme, &layout, &mut cache, false);
        let full_headings: Vec<(u32, u32)> = headings.iter().map(|h| (h.y_pos, h.heading_height)).collect();

        let mut headings = build_headings(&blocks);
        let (layouts, total) = layout_blocks(&blocks, &headings, width, &fonts, &theme, &layout, &mut cache);
        assert_eq!(total, full.height());
        let nstrips = total.div_ceil(strip_h);
        let row_bytes = width as usize * 3;
        for i in 0..nstrips {
            let y0 = i * strip_h;
            let strip = render_region(&blocks, &mut headings, width, &fonts, &theme, &layout, &mut cache, false, &layouts, y0, strip_h);
            assert_eq!(strip.dimensions(), (width, strip_h));
            let rows = strip_h.min(total - y0) as usize;
            let want = &full.as_raw()[y0 as usize * row_bytes..(y0 as usize + rows) * row_bytes];
            let got = &strip.as_raw()[..rows * row_bytes];
            if want != got {
                let bad = (0..rows).find(|r| want[r * row_bytes..(r + 1) * row_bytes] != got[r * row_bytes..(r + 1) * row_bytes]).unwrap();
                panic!("strip {i} (h={strip_h}) differs at page row {}", y0 as usize + bad);
            }
            assert!(strip.as_raw()[rows * row_bytes..].chunks(3).all(|p| p == theme.bg.0), "padding is bg");
        }
        let region_headings: Vec<(u32, u32)> = headings.iter().map(|h| (h.y_pos, h.heading_height)).collect();
        assert_eq!(region_headings, full_headings, "heading positions match the full render");
    }

    #[test]
    fn region_strips_match_full_render() {
        let long = long_test_doc();
        let docs: [(&str, &str); 3] = [
            ("SAMPLE_MD", SAMPLE_MD),
            ("docs/sample.md", include_str!("../docs/sample.md")),
            ("long", &long),
        ];
        for (name, md) in docs {
            for strip_h in [21 * 48, 21 * 297, 1000] {
                eprintln!("{name} strip_h={strip_h}");
                assert_regions_match_full(md, strip_h);
            }
        }
    }

    #[test]
    fn long_test_doc_is_long() {
        let fonts = test_fonts();
        let theme = default_theme();
        let layout = LayoutParams::default();
        let (blocks, _) = parse_markdown(&long_test_doc());
        let headings = build_headings(&blocks);
        let mut cache = MermaidCache::new();
        let (_, total) = layout_blocks(&blocks, &headings, 970, &fonts, &theme, &layout, &mut cache);
        assert!(total / 21 >= 600, "long doc has {} rows", total / 21);
        assert!(blocks.iter().any(|b| matches!(b, Block::Mermaid { .. })));
        assert!(blocks.iter().any(|b| matches!(b, Block::Metadata { .. })));
    }

    #[test]
    fn total_height_unchanged() {
        // Heights recorded from the renderer before the layout refactor.
        let fonts = test_fonts();
        let theme = default_theme();
        let layout = LayoutParams::default();
        let cases: [(&str, u32, bool, u32); 8] = [
            (SAMPLE_MD, 800, false, 1340),
            (SAMPLE_MD, 800, true, 1214),
            (SAMPLE_MD, 970, false, 1340),
            (SAMPLE_MD, 970, true, 1214),
            (include_str!("../docs/sample.md"), 800, false, 3014),
            (include_str!("../docs/sample.md"), 800, true, 2838),
            (include_str!("../docs/sample.md"), 970, false, 2914),
            (include_str!("../docs/sample.md"), 970, true, 2763),
        ];
        for (md, width, fold, want) in cases {
            let (blocks, _) = parse_markdown(md);
            let mut headings = build_headings(&blocks);
            headings[1].folded = fold;
            let mut cache = MermaidCache::new();
            let (img, _, _) = render_markdown(&blocks, &mut headings, width, 600, &fonts, &theme, &layout, &mut cache, true);
            assert_eq!(img.height(), want, "width {width} fold {fold}");
        }
    }
}

