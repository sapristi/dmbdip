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

#[cfg(test)]
pub(crate) fn default_theme() -> Theme {
    crate::config::build_theme(&crate::config::ThemeConfig::default())
}
