# mdbdp - Markdown Display as Beautiful Pictures

A Rust program that renders markdown files as images and displays them in the terminal (Kitty graphics protocol).

## Workflow Rules

- Commit changes each time a task is done
- Clean git history if appropriate (squash fixup commits)
- Evaluate quality every few commits and after fixing bugs

## Tech Stack

- **Language:** Rust
- **Terminal:** Kitty (graphics protocol, raw RGB f=24, double-buffered)
- **Key crates:** `image`, `base64`, `crossterm`, `pulldown-cmark`, `ab_glyph`, `imageproc`
- **Fonts:** DejaVu Sans (regular, bold, oblique) + DejaVu Sans Mono

## Architecture

- Parse markdown into `Vec<Block>` (Heading, Paragraph, CodeBlock, Table, Metadata)
- Inline text uses `Vec<Span>` with styles: Normal, Bold, Italic, Code
- `HeadingInfo` tracks each heading's position, number, fold state
- `AppState` manages blocks, headings, cursor, scroll, fold state
- Two-pass rendering: compute height, then draw to RgbImage (re-renders on fold/nav changes)
- Kitty display: double-buffered with alternating image IDs, raw RGB, BufWriter
- Alternate screen mode for proper key passthrough

## Keybindings

- Up/Down: navigate between headings
- Left/Right: toggle fold open/close
- Space: scroll down half page
- Shift+Space: scroll up half page
- j/k: small scroll steps
- PgUp/PgDn: half-page scroll
- Home/End: top/bottom
- q/Esc/Ctrl-C: quit

## Completed Tasks

- [x] Task 0: Fix inline code spacing and code block indentation
- [x] Task 1: Hierarchical heading numbering (1., 1.1., 1.1.1.)
- [x] Task 2: Heading navigation with cursor (orange bar in left margin)
- [x] Task 3: Section folding with ▶/▼ indicators
- [x] Task 4: New keybindings (arrows for nav, space for scroll)
