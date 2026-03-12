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

    let sans_name = font_config
        .and_then(|c| c.sans.as_deref())
        .unwrap_or("DejaVu Sans");
    let mono_name = font_config
        .and_then(|c| c.mono.as_deref())
        .unwrap_or("DejaVu Sans Mono");

    let sans_families = &[
        FamilyName::Title(sans_name.to_string()),
        FamilyName::SansSerif,
    ];
    let mono_families = &[
        FamilyName::Title(mono_name.to_string()),
        FamilyName::Monospace,
    ];

    Fonts {
        regular: load_font(&source, sans_families, &Properties::new(), sans_name),
        bold: load_font(
            &source,
            sans_families,
            Properties::new().weight(Weight::BOLD),
            &format!("{} Bold", sans_name),
        ),
        italic: load_font(
            &source,
            sans_families,
            Properties::new().style(Style::Italic),
            &format!("{} Italic", sans_name),
        ),
        mono: load_font(
            &source,
            mono_families,
            &Properties::new(),
            mono_name,
        ),
    }
}
