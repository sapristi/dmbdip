use ab_glyph::FontVec;
use font_kit::family_name::FamilyName;
use font_kit::properties::{Properties, Style, Weight};
use font_kit::source::SystemSource;

use crate::config::FontsConfig;

pub(crate) struct Fonts {
    pub(crate) regular: FontVec,
    pub(crate) bold: FontVec,
    pub(crate) italic: FontVec,
    pub(crate) mono: FontVec,
}

fn load_font_from_path(path: &str, name: &str) -> FontVec {
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error: Could not read font file '{}' for {}: {}", path, name, e);
            std::process::exit(1);
        }
    };
    match FontVec::try_from_vec(data) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Error: Could not parse font file '{}' for {}: {}", path, name, e);
            std::process::exit(1);
        }
    }
}

fn load_font(
    source: &SystemSource,
    families: &[FamilyName],
    properties: &Properties,
    name: &str,
) -> FontVec {
    let handle = match source.select_best_match(families, properties) {
        Ok(h) => h,
        Err(_) => {
            eprintln!(
                "Error: Could not find {} font.\n\
                 Install DejaVu fonts or ensure fontconfig is available.\n\
                 On Debian/Ubuntu: sudo apt install fonts-dejavu\n\
                 On Arch: sudo pacman -S ttf-dejavu\n\
                 On macOS: brew install font-dejavu",
                name
            );
            std::process::exit(1);
        }
    };

    let font = match handle.load() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Error: Could not load {} font: {}", name, e);
            std::process::exit(1);
        }
    };

    let font_data = match font.copy_font_data() {
        Some(data) => data,
        None => {
            eprintln!("Error: Could not read font data for {}", name);
            std::process::exit(1);
        }
    };

    let index = match &handle {
        font_kit::handle::Handle::Path { font_index, .. }
        | font_kit::handle::Handle::Memory { font_index, .. } => *font_index,
    };
    match FontVec::try_from_vec_and_index((*font_data).clone(), index) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Error: Could not parse {} font: {}", name, e);
            std::process::exit(1);
        }
    }
}

pub(crate) fn load_fonts(font_config: Option<&FontsConfig>) -> Fonts {
    let source = SystemSource::new();

    let sans_families = &[
        FamilyName::Title("DejaVu Sans".to_string()),
        FamilyName::SansSerif,
    ];
    let mono_families = &[
        FamilyName::Title("DejaVu Sans Mono".to_string()),
        FamilyName::Monospace,
    ];

    let regular = match font_config.and_then(|c| c.regular.as_deref()) {
        Some(path) => load_font_from_path(path, "regular"),
        None => load_font(&source, sans_families, &Properties::new(), "DejaVu Sans"),
    };
    let bold = match font_config.and_then(|c| c.bold.as_deref()) {
        Some(path) => load_font_from_path(path, "bold"),
        None => load_font(
            &source,
            sans_families,
            Properties::new().weight(Weight::BOLD),
            "DejaVu Sans Bold",
        ),
    };
    let italic = match font_config.and_then(|c| c.italic.as_deref()) {
        Some(path) => load_font_from_path(path, "italic"),
        None => load_font(
            &source,
            sans_families,
            Properties::new().style(Style::Italic),
            "DejaVu Sans Italic",
        ),
    };
    let mono = match font_config.and_then(|c| c.mono.as_deref()) {
        Some(path) => load_font_from_path(path, "mono"),
        None => load_font(
            &source,
            mono_families,
            &Properties::new(),
            "DejaVu Sans Mono",
        ),
    };

    Fonts {
        regular,
        bold,
        italic,
        mono,
    }
}
