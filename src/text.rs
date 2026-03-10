use ab_glyph::PxScale;
use image::RgbImage;
use imageproc::drawing::{draw_filled_rect_mut, draw_text_mut, text_size};
use imageproc::rect::Rect;

use crate::fonts::Fonts;
use crate::theme::Theme;
use crate::types::{Span, SpanStyle};

pub(crate) fn spans_to_plain(spans: &[Span]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

fn split_preserving_indent(text: &str) -> (usize, Vec<&str>) {
    let leading = text.len() - text.trim_start().len();
    let words: Vec<&str> = text.split_whitespace().collect();
    (leading, words)
}

pub(crate) fn wrap_spans(spans: &[Span], fonts: &Fonts, scale: PxScale, max_width: u32) -> Vec<Vec<Span>> {
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

pub(crate) fn draw_spans(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::test_fonts;

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
}
