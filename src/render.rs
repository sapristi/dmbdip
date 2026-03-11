use ab_glyph::PxScale;
use image::{Rgb, RgbImage};
use imageproc::drawing::{draw_filled_rect_mut, draw_line_segment_mut, draw_text_mut, text_size};
use imageproc::rect::Rect;
use pulldown_cmark::HeadingLevel;

use crate::constants::*;
use crate::fonts::Fonts;
use crate::headings::is_block_folded;
use crate::text::{draw_spans, spans_to_plain, wrap_spans};
use crate::theme::Theme;
use crate::types::{Block, HeadingInfo, Span, SpanStyle};

pub(crate) fn render_preview(
    blocks: &[Block],
    headings: &[HeadingInfo],
    width: u32,
    max_height: u32,
    fonts: &Fonts,
) -> RgbImage {
    let theme = crate::theme::default_theme();
    let content_width = (width - MARGIN_LEFT - MARGIN_RIGHT).min(MAX_CONTENT_WIDTH);
    let margin_left = (width - content_width) / 2;

    let mut img = RgbImage::from_pixel(width, max_height, theme.bg);
    let mut y: u32 = PARAGRAPH_GAP;
    let mut heading_idx: usize = 0;

    for (_bi, block) in blocks.iter().enumerate() {
        if y >= max_height {
            break;
        }

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
                if hi >= headings.len() {
                    continue;
                }

                let (lines, size, line_height) = wrap_heading_text(
                    &headings[hi], spans, fonts, &theme, content_width,
                );
                let (_, color) = heading_style(level, &theme);
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
                y += PARAGRAPH_GAP;
            }
            Block::Paragraph { spans } => {
                let scale = PxScale::from(theme.body_size);
                let indented_width = content_width - BLOCK_INDENT;
                let lines = wrap_spans(spans, fonts, scale, indented_width);
                let line_height = (theme.body_size * 1.4) as u32;

                for line in &lines {
                    if y >= max_height {
                        break;
                    }
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

    img
}

pub(crate) fn render_markdown(
    blocks: &[Block],
    headings: &mut [HeadingInfo],
    width: u32,
    vp_height: u32,
    fonts: &Fonts,
) -> (RgbImage, Vec<(usize, u32)>, u32) {
    let theme = crate::theme::default_theme();
    let content_width = (width - MARGIN_LEFT - MARGIN_RIGHT).min(MAX_CONTENT_WIDTH);
    let margin_left = (width - content_width) / 2;

    let total_height = compute_total_height(blocks, headings, fonts, &theme, content_width, vp_height);

    let mut img = RgbImage::from_pixel(width, total_height.max(1), theme.bg);
    let mut y: u32 = PARAGRAPH_GAP;
    let mut heading_idx: usize = 0;
    let mut block_positions: Vec<(usize, u32)> = Vec::new();

    for (bi, block) in blocks.iter().enumerate() {
        if is_block_folded(bi, headings) {
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

                headings[hi].y_pos = y;
                headings[hi].heading_height = heading_total_h;

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

pub(crate) fn wrap_code_lines(text: &str, fonts: &Fonts, scale: PxScale, inner_width: u32) -> Vec<Vec<Vec<Span>>> {
    text.lines()
        .map(|line| {
            wrap_spans(
                &[Span { text: line.to_string(), style: SpanStyle::Code }],
                fonts, scale, inner_width,
            )
        })
        .collect()
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

fn compute_total_height(
    blocks: &[Block],
    headings: &[HeadingInfo],
    fonts: &Fonts,
    theme: &Theme,
    content_width: u32,
    vp_height: u32,
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

    h + PARAGRAPH_GAP + vp_height / 2
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::headings::build_headings;
    use crate::parsing::parse_markdown;
    use crate::test_helpers::{test_fonts, SAMPLE_MD};

    #[test]
    fn render_produces_valid_image() {
        let fonts = test_fonts();
        let blocks = parse_markdown(SAMPLE_MD);
        let mut headings = build_headings(&blocks);
        let (img, positions, margin_left) = render_markdown(&blocks, &mut headings, 800, 600, &fonts);

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
        render_markdown(&blocks, &mut headings, 800, 600, &fonts);

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
        let blocks = parse_markdown(SAMPLE_MD);

        let mut headings_open = build_headings(&blocks);
        let (img_open, _, _) = render_markdown(&blocks, &mut headings_open, 800, 600, &fonts);

        let mut headings_folded = build_headings(&blocks);
        headings_folded[0].folded = true;
        let (img_folded, _, _) = render_markdown(&blocks, &mut headings_folded, 800, 600, &fonts);

        assert!(
            img_folded.height() < img_open.height(),
            "folded ({}) should be shorter than open ({})",
            img_folded.height(), img_open.height(),
        );
    }

    #[test]
    fn render_preview_produces_valid_image() {
        let fonts = test_fonts();
        let blocks = parse_markdown(SAMPLE_MD);
        let headings = build_headings(&blocks);
        let img = render_preview(&blocks, &headings, 800, 600, &fonts);

        assert_eq!(img.width(), 800);
        assert_eq!(img.height(), 600);
        // Check it's not entirely blank (background color)
        let theme = crate::theme::default_theme();
        let bg_pixel = theme.bg;
        let has_content = img.pixels().any(|p| *p != bg_pixel);
        assert!(has_content, "preview image should not be entirely blank");
    }

    #[test]
    fn render_at_different_widths() {
        let fonts = test_fonts();
        for width in [400, 800, 1200, 1920] {
            let blocks = parse_markdown(SAMPLE_MD);
            let mut headings = build_headings(&blocks);
            let (img, _, _) = render_markdown(&blocks, &mut headings, width, 600, &fonts);
            assert_eq!(img.width(), width);
            assert!(img.height() > 0);
        }
    }
}
