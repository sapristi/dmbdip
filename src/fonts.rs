use ab_glyph::FontVec;
use std::process::Command;

pub(crate) struct Fonts {
    pub(crate) regular: FontVec,
    pub(crate) bold: FontVec,
    pub(crate) italic: FontVec,
    pub(crate) mono: FontVec,
}

/// Try to find a font using fc-match (fontconfig).
/// `pattern` is the fontconfig pattern, e.g. "DejaVu Sans" or "sans-serif:style=Bold".
fn fc_match(pattern: &str) -> Option<String> {
    let output = Command::new("fc-match")
        .args(["-f", "%{file}", pattern])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    if path.is_empty() || !std::path::Path::new(&path).exists() {
        return None;
    }
    Some(path)
}

pub(crate) fn load_fonts() -> Fonts {
    let load =
        |paths: &[&str], name: &str, fc_patterns: &[&str]| -> FontVec {
            // Try hardcoded paths first
            for path in paths {
                if let Ok(data) = std::fs::read(path) {
                    if let Ok(font) = FontVec::try_from_vec(data) {
                        return font;
                    }
                }
            }
            // Try fontconfig as fallback
            for pattern in fc_patterns {
                if let Some(path) = fc_match(pattern) {
                    if let Ok(data) = std::fs::read(&path) {
                        if let Ok(font) = FontVec::try_from_vec(data) {
                            eprintln!("Note: using fallback font '{}' for {}", path, name);
                            return font;
                        }
                    }
                }
            }
            eprintln!(
                "Error: Could not find {} font.\n\
                 Install DejaVu fonts or ensure fontconfig (fc-match) is available.\n\
                 On Debian/Ubuntu: sudo apt install fonts-dejavu\n\
                 On Arch: sudo pacman -S ttf-dejavu\n\
                 On macOS: brew install font-dejavu",
                name
            );
            std::process::exit(1);
        };

    Fonts {
        regular: load(
            &[
                "/usr/share/fonts/TTF/DejaVuSans.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/opt/homebrew/share/fonts/dejavu/DejaVuSans.ttf",
                "/usr/local/share/fonts/dejavu/DejaVuSans.ttf",
                "/Library/Fonts/DejaVuSans.ttf",
            ],
            "DejaVu Sans",
            &["DejaVu Sans", "sans-serif"],
        ),
        bold: load(
            &[
                "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
                "/opt/homebrew/share/fonts/dejavu/DejaVuSans-Bold.ttf",
                "/usr/local/share/fonts/dejavu/DejaVuSans-Bold.ttf",
                "/Library/Fonts/DejaVuSans-Bold.ttf",
            ],
            "DejaVu Sans Bold",
            &["DejaVu Sans:style=Bold", "sans-serif:style=Bold", "sans-serif"],
        ),
        italic: load(
            &[
                "/usr/share/fonts/TTF/DejaVuSans-Oblique.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans-Oblique.ttf",
                "/opt/homebrew/share/fonts/dejavu/DejaVuSans-Oblique.ttf",
                "/usr/local/share/fonts/dejavu/DejaVuSans-Oblique.ttf",
                "/Library/Fonts/DejaVuSans-Oblique.ttf",
            ],
            "DejaVu Sans Oblique",
            &[
                "DejaVu Sans:style=Oblique",
                "sans-serif:style=Italic",
                "sans-serif:style=Oblique",
                "sans-serif",
            ],
        ),
        mono: load(
            &[
                "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
                "/opt/homebrew/share/fonts/dejavu/DejaVuSansMono.ttf",
                "/usr/local/share/fonts/dejavu/DejaVuSansMono.ttf",
                "/Library/Fonts/DejaVuSansMono.ttf",
            ],
            "DejaVu Sans Mono",
            &["DejaVu Sans Mono", "monospace"],
        ),
    }
}
