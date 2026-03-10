# dmbdip - Display Markdown But Do it Pretty

![Preview](assets/preview.png)

A Rust program that renders markdown files as images and displays them in the terminal using the Kitty graphics protocol.

> This file serves as both the project README and the CLAUDE.md (AI assistant instructions).

## Usage

```
dmbdip <markdown-file>
dmbdip --help
```

**Requirements:** Kitty terminal (or any terminal supporting the Kitty graphics protocol), DejaVu fonts installed.

## Building

Requires Rust (1.85+ for edition 2024). Install via [rustup](https://rustup.rs/).

```
cargo build --release
```

The binary will be at `target/release/dmbdip`. Copy it to a directory in your `$PATH`:

```
cp target/release/dmbdip ~/.local/bin/
```

## Keybindings

| Key | Action |
|-----|--------|
| Up/Down | Navigate between headings |
| Left/Right/Tab | Toggle fold open/close |
| Space | Scroll down |
| Ctrl+Space | Scroll up |
| j/k | Small scroll steps |
| PgUp/PgDn | Half-page scroll |
| Home/End | Jump to top/bottom |
| / | Search text (vim-style) |
| n/N | Next/previous search match |
| h | Show keybindings help overlay |
| q/Esc/Ctrl-C | Quit |

## Tech Stack

- **Language:** Rust
- **Terminal:** Kitty (graphics protocol, raw RGB f=24, double-buffered)
- **Key crates:** `image`, `base64`, `crossterm`, `pulldown-cmark`, `ab_glyph`, `imageproc`
- **Fonts:** DejaVu Sans (regular, bold, oblique) + DejaVu Sans Mono

## Architecture

- Parse markdown into `Vec<Block>` (Heading, Paragraph, CodeBlock, Table, Metadata)
- Inline text uses `Vec<Span>` with styles: Normal, Bold, Italic, Code
- `HeadingInfo` tracks each heading's position, number, fold state
- `AppState` manages blocks, headings, cursor, scroll, fold, search state
- Two-pass rendering: compute height, then draw to RgbImage (re-renders on fold/nav changes)
- Kitty display: double-buffered with alternating image IDs, raw RGB, BufWriter
- Alternate screen mode for proper key passthrough

## Development

See [DEVELOPMENT.md](DEVELOPMENT.md) for task tracking and workflow notes.
