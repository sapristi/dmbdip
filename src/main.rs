mod browser;
mod config;
mod constants;
mod fonts;
mod headings;
mod kitty;
mod overlay;
mod parsing;
mod render;
mod source_render;
mod smooth_scroll;
mod source_state;
mod state;
mod text;
mod theme;
mod types;

#[cfg(test)]
mod test_helpers;

use std::io;
use std::path::Path;

use config::{build_theme, debug_config, load_config};
use constants::{LayoutParams, KEYBINDINGS, BROWSER_KEYBINDINGS};
use fonts::load_fonts;
use kitty::get_viewport_pixel_size;

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.get(1).map(|s| s.as_str()) == Some("--debug-config") {
        let config = load_config();
        debug_config(&config);
        std::process::exit(0);
    }

    if args.get(1).map(|s| s.as_str()) == Some("--help")
        || args.get(1).map(|s| s.as_str()) == Some("-h")
    {
        eprintln!("dmbdip - Display Markdown But Do it Pretty");
        eprintln!();
        eprintln!("Usage: dmbdip [markdown-file-or-directory]");
        eprintln!("       dmbdip --debug-config");
        eprintln!();
        eprintln!("Renders markdown files as images in the terminal using the Kitty");
        eprintln!("graphics protocol. Always opens in browser mode with a file list");
        eprintln!("on the left. When given a file, opens it directly with the file");
        eprintln!("list hidden.");
        eprintln!();
        eprintln!("Keybindings (document view):");
        for &(key, desc) in KEYBINDINGS {
            eprintln!("  {:<20} {}", key, desc);
        }
        eprintln!();
        eprintln!("Keybindings (file browser):");
        for &(key, desc) in BROWSER_KEYBINDINGS {
            eprintln!("  {:<20} {}", key, desc);
        }
        std::process::exit(0);
    }

    let file_path = args.get(1).map(|s| s.as_str()).unwrap_or(".");
    let path = Path::new(file_path);

    let config = load_config();
    let theme = build_theme(&config.theme);
    let layout = LayoutParams::from_config(&config.layout);
    let fonts = load_fonts(Some(&config.fonts));
    let (vp_width, vp_height) = get_viewport_pixel_size()?;
    let extra_extensions = config.browser.extra_extensions.unwrap_or_default();

    if path.is_dir() {
        browser::run_browser(path, None, &fonts, vp_width, vp_height, &theme, &layout, &extra_extensions)
    } else {
        let dir = match path.parent() {
            Some(p) if p.as_os_str().is_empty() => Path::new("."),
            Some(p) => p,
            None => Path::new("."),
        };
        browser::run_browser(dir, Some(path), &fonts, vp_width, vp_height, &theme, &layout, &extra_extensions)
    }
}
