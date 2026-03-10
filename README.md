# dmbdip - Display Markdown But Do it Pretty

![Preview](assets/preview.png)

A Rust program that renders markdown files as images and displays them in the terminal using the Kitty graphics protocol.

## Usage

```
dmbdip <markdown-file>
dmbdip <directory>
dmbdip --help
```

When given a directory, opens a two-pane file browser: text file listing on the left, markdown preview on the right.

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

### File Browser Mode

| Key | Action |
|-----|--------|
| Up/Down, j/k | Move cursor |
| Enter | Preview selected markdown file |
| Right / l | Enter subfolder |
| Left / h | Go to parent directory |
| q/Esc | Quit |

## Development

See [DEVELOPMENT.md](DEVELOPMENT.md) for architecture, tech stack, task tracking, and workflow notes.
