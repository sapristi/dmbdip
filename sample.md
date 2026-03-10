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

### Nested Heading

- This is a list item (rendered as plain text for now)
- Another item with **bold** and *italic*
- Third item with `code`

## Final Section

The end of the document. This demonstrates scrolling with real markdown content including **bold**, *italic*, and `code` formatting.
