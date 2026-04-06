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
use std::path::{Path, PathBuf};
use std::time::Instant;

use config::{build_theme, debug_config, load_config};
use constants::LayoutParams;
use fonts::load_fonts;
use kitty::{detect_graphics_support, get_viewport_pixel_size};

struct Args {
    debug: bool,
    debug_config: bool,
    path: PathBuf,
}

fn print_help() {
    eprintln!("dmbdip - Display Markdown But Do it Pretty");
    eprintln!();
    eprintln!("Usage: dmbdip [OPTIONS] [markdown-file-or-directory]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --debug              Enable debug output");
    eprintln!("  --debug-config       Print resolved configuration and exit");
    eprintln!("  -v, --version        Show version");
    eprintln!("  -h, --help           Show this help message");
    eprintln!();
    eprintln!("Renders markdown files as images in the terminal using the Kitty");
    eprintln!("graphics protocol. Always opens in browser mode with a file list");
    eprintln!("on the left. When given a file, opens it directly with the file");
    eprintln!("list hidden.");
    eprintln!();
    eprintln!("Press 'h' in the app to view keybindings.");
}

fn parse_args() -> Result<Args, lexopt::Error> {
    use lexopt::prelude::*;

    let mut debug = false;
    let mut debug_config = false;
    let mut path = PathBuf::from(".");
    let mut parser = lexopt::Parser::from_env();

    while let Some(arg) = parser.next()? {
        match arg {
            Short('h') | Long("help") => {
                print_help();
                std::process::exit(0);
            }
            Short('v') | Long("version") => {
                eprintln!("dmbdip {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            Long("debug") => debug = true,
            Long("debug-config") => debug_config = true,
            Value(val) => path = PathBuf::from(val),
            _ => return Err(arg.unexpected()),
        }
    }

    Ok(Args { debug, debug_config, path })
}

fn main() -> io::Result<()> {
    let args = parse_args().unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        std::process::exit(2);
    });

    if args.debug_config {
        let config = load_config();
        debug_config(&config);
        std::process::exit(0);
    }

    let start = Instant::now();
    let supported = detect_graphics_support();
    let elapsed = start.elapsed();

    if args.debug {
        eprintln!("[debug] graphics protocol detection: {} in {:.1}ms",
            if supported { "supported" } else { "not supported" },
            elapsed.as_secs_f64() * 1000.0);
    }

    if !supported {
        eprintln!("Error: your terminal does not support the Kitty graphics protocol.");
        eprintln!("dmbdip requires a compatible terminal (e.g. Kitty, WezTerm, Ghostty).");
        std::process::exit(1);
    }

    let path = &args.path;

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
