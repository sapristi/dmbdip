---
title: Sample Document
author: John Doe
date: 2026-03-10
version: 1.0
---

# Main Heading

This is a paragraph with **bold text**, *italic text*, and `inline code`. You can mix them: **bold with *nested italic*** and regular text continues here.

## Second Level Heading

Here is another paragraph. The renderer handles **multiple paragraphs** separated by blank lines, with proper word wrapping when text is long enough to exceed the available width.

### Third Level Heading

A shorter paragraph with some `code_snippets` and *emphasis* for variety.

## Code Blocks

```rust
fn main() {
    let greeting = "Hello, world!";
    println!("{}", greeting);
    for i in 0..10 {
        println!("Count: {}", i);
    }
}
```

## Tables

| Name    | Age | City       | Description                                      |
|---------|-----|------------|--------------------------------------------------|
| Alice   | 30  | New York   | Software engineer working on distributed systems |
| Bob     | 25  | London     | Frontend developer with a passion for `React`    |
| Charlie | 35  | Tokyo      | A very long description that should wrap nicely within the table cell to demonstrate text wrapping |
| Diana   | 28  | Paris      | Data scientist specializing in **machine learning** |

## Another Section

Some more text after the table to verify that layout continues correctly after table rendering.

### Bullet Lists

- First item
- Second item with **bold** and *italic*
- Third item with `code`
  - Nested item one
  - Nested item two with **emphasis**
    - Deeply nested item
    - Another deep item with `inline code`
  - Nested item three
- Fourth item back at top level

### Numbered Lists

1. First step
2. Second step with **important** details
3. Third step with `code_example`
   1. Sub-step one
   2. Sub-step two with *italic text*
      1. Deep sub-step
      2. Another deep sub-step
   3. Sub-step three
4. Fourth step back at top level

### Mixed Lists

- Bullet item one
  1. Numbered sub-item
  2. Another numbered sub-item
- Bullet item two
  - Nested bullet
    1. Deep numbered item
    2. Another deep numbered item
  - Another nested bullet
- Bullet item three

## Final Section

The end of the document. This demonstrates scrolling with real markdown content including **bold**, *italic*, and `code` formatting.
