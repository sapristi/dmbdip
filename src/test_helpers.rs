#[cfg(test)]
pub(crate) const SAMPLE_MD: &str = "\
# Hello World

Some body text with **bold** and *italic* words.

## Section One

Paragraph under section one.

### Subsection 1.1

Details here.

## Section Two

Another paragraph.

```rust
fn main() {
    println!(\"hello\");
}
```

| Key | Value |
|-----|-------|
| a   | 1     |
| b   | 2     |
";

#[cfg(test)]
pub(crate) const SAMPLE_WITH_META: &str = "\
---
title: Test Doc
author: Tester
---

# Title

Body text.
";

#[cfg(test)]
pub(crate) fn test_fonts() -> crate::fonts::Fonts {
    crate::fonts::load_fonts()
}
