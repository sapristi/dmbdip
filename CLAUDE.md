# mdbdp - Display Markdown But Do it Pretty

A Rust program that renders markdown files as images and displays them in the terminal using the Kitty graphics protocol.

> This file serves as both the project README and the CLAUDE.md (AI assistant instructions).

## Usage

```
mdbdp <markdown-file>
mdbdp --help
```

**Requirements:** Kitty terminal (or any terminal supporting the Kitty graphics protocol), DejaVu fonts installed.

## Building

Requires Rust (1.85+ for edition 2024). Install via [rustup](https://rustup.rs/).

```
cargo build --release
```

The binary will be at `target/release/mdbdp`. Copy it to a directory in your `$PATH`:

```
cp target/release/mdbdp ~/.local/bin/
```

## Keybindings

| Key | Action |
|-----|--------|
| Up/Down | Navigate between headings |
| Left/Right/Tab | Toggle fold open/close |
| Space | Scroll down |
| Shift+Space | Scroll up |
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

## Workflow Rules

- Commit changes each time a task is done
- Clean git history if appropriate (squash fixup commits)
- Evaluate quality every few commits and after fixing bugs

# Advancement

## Todo

(no pending tasks)

## Completed Tasks

- [x] Task 0: Fix inline code spacing and code block indentation
- [x] Task 1: Hierarchical heading numbering (1., 1.1., 1.1.1.)
- [x] Task 2: Heading navigation with cursor (orange bar in left margin)
- [x] Task 3: Section folding with ▶/▼ indicators
- [x] Task 4: New keybindings (arrows for nav, space for scroll)
- [x] Task 5: CLAUDE.md as both README and CLAUDE.md with user info at top
- [x] Task 6: --help flag with usage and keybindings
- [x] Task 7: Help overlay when pressing `h`
- [x] Task 8: Vim-style search with `/`, `n`/`N` navigation
