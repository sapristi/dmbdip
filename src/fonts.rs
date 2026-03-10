use ab_glyph::FontVec;

pub(crate) struct Fonts {
    pub(crate) regular: FontVec,
    pub(crate) bold: FontVec,
    pub(crate) italic: FontVec,
    pub(crate) mono: FontVec,
}

pub(crate) fn load_fonts() -> Fonts {
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
