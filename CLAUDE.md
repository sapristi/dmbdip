# mdbdp - Markdown Display as Beautiful Pictures

A Rust program that renders markdown files as images and displays them in the terminal (Kitty graphics protocol).

## Development Plan

### Phase 1: Image display with scroll simulation (current)

**Goal:** Display a generated test image in Kitty terminal with keyboard-driven scrolling.

**Requirements:**
- Generate a test image of parametrizable size (width x height, larger than terminal viewport)
- Display a viewport-sized portion of the image using the Kitty graphics protocol
- On keypress (Up/Down, j/k, PgUp/PgDn), shift the viewport and redisplay
- ESC or q to quit

**Implementation steps:**
1. Initialize a Rust project with dependencies: `image` (image generation), `base64` (encoding for Kitty protocol), `crossterm` (raw mode + key events)
2. Generate a test image: colored bands or gradient with text labels showing Y coordinates, so scrolling is visually obvious
3. Implement Kitty graphics protocol output: encode image chunk as PNG, send via `\x1b_Gf=100,a=T,...;base64data\x1b\\`
4. Implement a viewport: track scroll offset, extract the visible sub-image, display it
5. Event loop: read key events with crossterm, update offset, redraw

**Key decisions:**
- Use Kitty's inline image protocol (APC escape sequences)
- Transmit as PNG (f=100) for compression
- Clear and re-transmit on each scroll (simple first, optimize later)
- Terminal size detection via crossterm to auto-size viewport

### Phase 2: Markdown to image rendering (future)

- Parse markdown (pulldown-cmark or similar)
- Render markdown to an image (text layout, code blocks, headings, etc.)
- Display the rendered image with scrolling from Phase 1

### Phase 3: Polish (future)

- Smooth scrolling / partial updates
- Resize handling
- File watching / live reload
- CLI argument parsing (file path, theme, font size)

## Tech Stack

- **Language:** Rust
- **Terminal:** Kitty (graphics protocol)
- **Key crates:** `image`, `base64`, `crossterm`
