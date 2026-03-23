mod browser;
mod config;
mod constants;
mod file_watcher;
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
use std::time::Instant;

use config::{build_theme, debug_config, load_config};
use constants::{LayoutParams, KEYBINDINGS, BROWSER_KEYBINDINGS};
use fonts::load_fonts;
use kitty::{detect_graphics_support, get_viewport_pixel_size};

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let debug = args.iter().any(|a| a == "--debug");

    if args.iter().any(|a| a == "--debug-config") {
        let config = load_config();
        debug_config(&config);
        std::process::exit(0);
    }

    if args.iter().any(|a| a == "--help" || a == "-h") {
        eprintln!("dmbdip - Display Markdown But Do it Pretty");
        eprintln!();
        eprintln!("Usage: dmbdip [OPTIONS] [markdown-file-or-directory]");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  --debug              Enable debug output");
        eprintln!("  --debug-config       Print resolved configuration and exit");
        eprintln!("  -h, --help           Show this help message");
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

    let start = Instant::now();
    let supported = detect_graphics_support();
    let elapsed = start.elapsed();

    if debug {
        eprintln!("[debug] graphics protocol detection: {} in {:.1}ms",
            if supported { "supported" } else { "not supported" },
            elapsed.as_secs_f64() * 1000.0);
    }

    if !supported {
        eprintln!("Error: your terminal does not support the Kitty graphics protocol.");
        eprintln!("dmbdip requires a compatible terminal (e.g. Kitty, WezTerm, Ghostty).");
        std::process::exit(1);
    }

    let file_path = args.iter()
        .skip(1)
        .find(|a| !a.starts_with("--"))
        .map(|s| s.as_str())
        .unwrap_or(".");
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
