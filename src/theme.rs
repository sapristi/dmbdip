use image::Rgb;

#[derive(Clone, Copy)]
pub(crate) struct Theme {
    pub(crate) bg: Rgb<u8>,
    pub(crate) body_color: Rgb<u8>,
    pub(crate) body_size: f32,
    pub(crate) code_color: Rgb<u8>,
    pub(crate) code_bg: Rgb<u8>,
    pub(crate) cursor_color: Rgb<u8>,
    pub(crate) h1_color: Rgb<u8>,
    pub(crate) h1_size: f32,
    pub(crate) h2_color: Rgb<u8>,
    pub(crate) h2_size: f32,
    pub(crate) h3_color: Rgb<u8>,
    pub(crate) h3_size: f32,
    pub(crate) meta_key_color: Rgb<u8>,
    pub(crate) meta_val_color: Rgb<u8>,
    pub(crate) table_border: Rgb<u8>,
    pub(crate) table_header_bg: Rgb<u8>,
}

pub(crate) fn default_theme() -> Theme {
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
