use base64::Engine;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{self, ClearType},
};
use image::{Rgb, RgbImage, ImageBuffer};
use std::io::{self, Write};

const SCROLL_STEP: u32 = 40;

fn generate_test_image(width: u32, height: u32) -> RgbImage {
    let mut img = ImageBuffer::new(width, height);

    let band_height = 60;
    let colors: Vec<Rgb<u8>> = vec![
        Rgb([220, 50, 50]),
        Rgb([50, 180, 50]),
        Rgb([50, 100, 220]),
        Rgb([220, 180, 50]),
        Rgb([180, 50, 220]),
        Rgb([50, 200, 200]),
        Rgb([220, 130, 50]),
        Rgb([120, 120, 120]),
    ];

    for (x, y, pixel) in img.enumerate_pixels_mut() {
        let band_index = (y / band_height) as usize;
        let base_color = colors[band_index % colors.len()];
        let factor = 0.7 + 0.3 * (x as f32 / width as f32);
        *pixel = Rgb([
            (base_color[0] as f32 * factor) as u8,
            (base_color[1] as f32 * factor) as u8,
            (base_color[2] as f32 * factor) as u8,
        ]);
    }

    draw_labels(&mut img, band_height);
    img
}

fn draw_labels(img: &mut RgbImage, band_height: u32) {
    let height = img.height();
    for band in 0..(height / band_height) {
        let y_pos = band * band_height + band_height / 2;
        let label = format!("y={}", band * band_height);
        draw_text(img, 10, y_pos.saturating_sub(3), &label);
    }
}

fn draw_text(img: &mut RgbImage, x: u32, y: u32, text: &str) {
    let white = Rgb([255, 255, 255]);
    let black = Rgb([0, 0, 0]);

    for (ci, ch) in text.chars().enumerate() {
        let glyph = get_glyph(ch);
        let cx = x + ci as u32 * 7;
        for row in 0..7u32 {
            for col in 0..5u32 {
                let px = cx + col;
                let py = y + row;
                if px < img.width() && py < img.height() {
                    img.put_pixel(px, py, black);
                    if glyph[row as usize] & (1 << (4 - col)) != 0 {
                        img.put_pixel(px, py, white);
                    }
                }
            }
        }
    }
}

fn get_glyph(ch: char) -> [u8; 7] {
    match ch {
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111],
        '3' => [0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110],
        'y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01000, 0b10000, 0b10000],
        '=' => [0b00000, 0b00000, 0b11111, 0b00000, 0b11111, 0b00000, 0b00000],
        _ => [0b00000; 7],
    }
}

fn get_viewport_pixel_size() -> io::Result<(u32, u32)> {
    let size = terminal::window_size()?;
    if size.width > 0 && size.height > 0 {
        Ok((size.width as u32, size.height as u32))
    } else {
        let (cols, rows) = terminal::size()?;
        Ok((cols as u32 * 8, rows as u32 * 16))
    }
}

fn encode_png(img: &RgbImage) -> Vec<u8> {
    let mut buf = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut buf);
    image::ImageEncoder::write_image(
        encoder,
        img.as_raw(),
        img.width(),
        img.height(),
        image::ExtendedColorType::Rgb8,
    )
    .expect("PNG encoding failed");
    buf
}

const IMAGE_ID: u32 = 1;

/// Transmit the full image to kitty (store it, don't display yet).
fn kitty_transmit(stdout: &mut io::Stdout, img: &RgbImage) -> io::Result<()> {
    let png_data = encode_png(img);
    let b64 = base64::engine::general_purpose::STANDARD.encode(&png_data);

    let chunk_size = 4096;
    let bytes = b64.as_bytes();
    let total_chunks = (bytes.len() + chunk_size - 1) / chunk_size;

    for (i, chunk) in bytes.chunks(chunk_size).enumerate() {
        let chunk_str = std::str::from_utf8(chunk).unwrap();
        let is_last = i == total_chunks - 1;
        let m = if is_last { 0 } else { 1 };

        if i == 0 {
            // a=t: transmit only (don't display), i=ID, f=100: PNG
            write!(
                stdout,
                "\x1b_Ga=t,i={IMAGE_ID},f=100,q=2,m={m};{chunk_str}\x1b\\"
            )?;
        } else {
            write!(stdout, "\x1b_Gm={m};{chunk_str}\x1b\\")?;
        }
    }

    stdout.flush()
}

/// Display a viewport of the already-transmitted image using source rect cropping.
fn kitty_place(
    stdout: &mut io::Stdout,
    scroll_y: u32,
    vp_width: u32,
    vp_height: u32,
    img_height: u32,
) -> io::Result<()> {
    let h = vp_height.min(img_height - scroll_y);
    // Delete previous placements for this image, then place new one
    // a=p: display, i=ID, x/y: source origin, w/h: source rect size, q=2: suppress errors
    write!(
        stdout,
        "\x1b_Ga=d,d=i,i={IMAGE_ID},q=2\x1b\\\x1b_Ga=p,i={IMAGE_ID},x=0,y={scroll_y},w={vp_width},h={h},q=2,C=1\x1b\\"
    )?;
    stdout.flush()
}

fn redraw(
    stdout: &mut io::Stdout,
    scroll_y: u32,
    vp_width: u32,
    vp_height: u32,
    img_height: u32,
) -> io::Result<()> {
    execute!(stdout, cursor::MoveTo(0, 0))?;
    kitty_place(stdout, scroll_y, vp_width, vp_height, img_height)
}

fn main() -> io::Result<()> {
    let (vp_width, vp_height) = get_viewport_pixel_size()?;

    let img_width = vp_width;
    let img_height = vp_height * 3;

    eprintln!(
        "Viewport: {}x{} px, Image: {}x{} px",
        vp_width, vp_height, img_width, img_height
    );

    let img = generate_test_image(img_width, img_height);
    let max_scroll = img_height.saturating_sub(vp_height);
    let mut scroll_y: u32 = 0;

    let mut stdout = io::stdout();

    terminal::enable_raw_mode()?;
    execute!(stdout, cursor::Hide, terminal::Clear(ClearType::All))?;

    // Transmit full image once, then only change placements on scroll
    kitty_transmit(&mut stdout, &img)?;
    redraw(&mut stdout, scroll_y, vp_width, vp_height, img_height)?;

    loop {
        if let Event::Key(KeyEvent {
            code, modifiers, ..
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
                    redraw(&mut stdout, scroll_y, vp_width, vp_height, img_height)?;
                }
            }
        }
    }

    // Cleanup: delete image data
    write!(stdout, "\x1b_Ga=d,d=I,i={IMAGE_ID},q=2\x1b\\")?;
    execute!(
        stdout,
        cursor::Show,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0)
    )?;
    terminal::disable_raw_mode()?;

    Ok(())
}
