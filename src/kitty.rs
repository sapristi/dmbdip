use base64::Engine;
use image::RgbImage;
use std::io::{self, Write};

use crate::constants::CURSOR_WIDTH;
use crossterm::terminal;

pub(crate) fn kitty_display_raw(
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

pub(crate) fn paint_rect(
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

/// Display a viewport of the rendered image via Kitty protocol.
/// If `col` is Some, position cursor at that column (for browser split view).
pub(crate) fn display_viewport(
    w: &mut impl Write,
    img: &RgbImage,
    scroll_y: u32,
    vp_width: u32,
    vp_height: u32,
    frame: &mut u32,
    col: Option<u16>,
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

    match col {
        Some(c) => kitty_display_at(w, &viewport_data, src_w, src_h, c, new_id, old_id),
        None => kitty_display_raw(w, &viewport_data, src_w, src_h, new_id, old_id),
    }
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

pub(crate) fn get_viewport_pixel_size() -> io::Result<(u32, u32)> {
    let size = terminal::window_size()?;
    if size.width > 0 && size.height > 0 {
        Ok((size.width as u32, size.height as u32))
    } else {
        let (cols, rows) = terminal::size()?;
        Ok((cols as u32 * 8, rows as u32 * 16))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::LayoutParams;
    use crate::overlay::render_search_bar;
    use crate::state::AppState;
    use crate::test_helpers::{test_fonts, SAMPLE_MD};
    use crate::theme::default_theme;

    #[test]
    fn display_viewport_does_not_panic() {
        let fonts = test_fonts();
        let mut state = AppState::new(SAMPLE_MD, &fonts, 800, 600, default_theme(), LayoutParams::default());
        let mut buf: Vec<u8> = Vec::new();
        let ci = state.cursor_info();
        let result = display_viewport(
            &mut buf, &state.img, state.scroll_y,
            state.vp_width, state.vp_height, &mut state.frame,
            None, None, ci, &state.search_highlights, state.search_current,
        );
        assert!(result.is_ok());
        assert!(!buf.is_empty(), "should produce kitty protocol output");
    }

    #[test]
    fn display_viewport_with_search_overlay() {
        let fonts = test_fonts();
        let mut state = AppState::new(SAMPLE_MD, &fonts, 800, 600, default_theme(), LayoutParams::default());
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
            None, Some(&search_bar), ci,
            &state.search_highlights, state.search_current,
        );
        assert!(result.is_ok());
    }
}
