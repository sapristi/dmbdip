use pulldown_cmark::{Event as MdEvent, Options, Parser, Tag, TagEnd};

use crate::types::{Block, ListItem, ListMarker, Span, SpanStyle};

pub(crate) fn parse_metadata(source: &str) -> (Vec<(String, String)>, &str) {
    let trimmed = source.trim_start();
    if !trimmed.starts_with("---") {
        return (Vec::new(), source);
    }

    let after_first = &trimmed[3..];
    if let Some(end) = after_first.find("\n---") {
        let meta_block = &after_first[..end];
        let rest = &after_first[end + 4..];
        let rest = rest.strip_prefix('\n').unwrap_or(rest);

        let entries: Vec<(String, String)> = meta_block
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() {
                    return None;
                }
                let (key, val) = line.split_once(':')?;
                Some((key.trim().to_string(), val.trim().to_string()))
            })
            .collect();

        (entries, rest)
    } else {
        (Vec::new(), source)
    }
}

pub(crate) fn parse_markdown(source: &str) -> Vec<Block> {
    let (metadata, source) = parse_metadata(source);

    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    let parser = Parser::new_ext(source, options);
    let mut blocks = Vec::new();

    if !metadata.is_empty() {
        blocks.push(Block::Metadata { entries: metadata });
    }

    let mut spans: Vec<Span> = Vec::new();
    let mut style_stack: Vec<SpanStyle> = vec![SpanStyle::Normal];
    let mut in_heading: Option<pulldown_cmark::HeadingLevel> = None;
    let mut in_paragraph = false;

    let mut in_table = false;
    let mut table_headers: Vec<Vec<Span>> = Vec::new();
    let mut table_rows: Vec<Vec<Vec<Span>>> = Vec::new();
    let mut current_row: Vec<Vec<Span>> = Vec::new();
    let mut in_table_head = false;
    let mut cell_spans: Vec<Span> = Vec::new();

    let mut in_code_block = false;
    let mut code_text = String::new();

    let mut list_stack: Vec<ListMarker> = Vec::new();
    let mut list_items: Vec<ListItem> = Vec::new();
    let mut list_item_spans_stack: Vec<Vec<Span>> = Vec::new();
    let mut list_item_insert_idx: Vec<usize> = Vec::new();

    let current_style = |stack: &[SpanStyle]| stack.last().cloned().unwrap_or(SpanStyle::Normal);

    for event in parser {
        match event {
            MdEvent::Start(Tag::Heading { level, .. }) => {
                in_heading = Some(level);
                spans.clear();
            }
            MdEvent::End(TagEnd::Heading(_)) => {
                if let Some(level) = in_heading.take() {
                    blocks.push(Block::Heading {
                        level,
                        spans: std::mem::take(&mut spans),
                    });
                }
            }
            MdEvent::Start(Tag::Paragraph) => {
                if !in_table && list_stack.is_empty() {
                    in_paragraph = true;
                    spans.clear();
                }
            }
            MdEvent::End(TagEnd::Paragraph) => {
                if in_paragraph {
                    in_paragraph = false;
                    let s = std::mem::take(&mut spans);
                    if !s.is_empty() {
                        blocks.push(Block::Paragraph { spans: s });
                    }
                }
            }
            MdEvent::Start(Tag::Strong) => style_stack.push(SpanStyle::Bold),
            MdEvent::End(TagEnd::Strong) => {
                style_stack.pop();
            }
            MdEvent::Start(Tag::Emphasis) => style_stack.push(SpanStyle::Italic),
            MdEvent::End(TagEnd::Emphasis) => {
                style_stack.pop();
            }
            MdEvent::Start(Tag::CodeBlock(_)) => {
                in_code_block = true;
                code_text.clear();
            }
            MdEvent::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                blocks.push(Block::CodeBlock {
                    text: std::mem::take(&mut code_text),
                });
            }
            MdEvent::Code(code) => {
                let target = if in_table {
                    &mut cell_spans
                } else if !list_item_spans_stack.is_empty() {
                    list_item_spans_stack.last_mut().unwrap()
                } else {
                    &mut spans
                };
                target.push(Span {
                    text: code.to_string(),
                    style: SpanStyle::Code,
                });
            }
            MdEvent::Start(Tag::Table(_)) => {
                in_table = true;
                table_headers.clear();
                table_rows.clear();
            }
            MdEvent::End(TagEnd::Table) => {
                in_table = false;
                blocks.push(Block::Table {
                    headers: std::mem::take(&mut table_headers),
                    rows: std::mem::take(&mut table_rows),
                });
            }
            MdEvent::Start(Tag::TableHead) => {
                in_table_head = true;
                current_row.clear();
            }
            MdEvent::End(TagEnd::TableHead) => {
                in_table_head = false;
                table_headers = std::mem::take(&mut current_row);
            }
            MdEvent::Start(Tag::TableRow) => {
                current_row.clear();
            }
            MdEvent::End(TagEnd::TableRow) => {
                if !in_table_head {
                    table_rows.push(std::mem::take(&mut current_row));
                }
            }
            MdEvent::Start(Tag::TableCell) => {
                cell_spans.clear();
            }
            MdEvent::End(TagEnd::TableCell) => {
                current_row.push(std::mem::take(&mut cell_spans));
            }
            MdEvent::Text(t) => {
                if in_code_block {
                    code_text.push_str(&t);
                } else if in_table {
                    cell_spans.push(Span {
                        text: t.to_string(),
                        style: current_style(&style_stack),
                    });
                } else if !list_item_spans_stack.is_empty() {
                    list_item_spans_stack.last_mut().unwrap().push(Span {
                        text: t.to_string(),
                        style: current_style(&style_stack),
                    });
                } else {
                    spans.push(Span {
                        text: t.to_string(),
                        style: current_style(&style_stack),
                    });
                }
            }
            MdEvent::Start(Tag::List(first_number)) => {
                match first_number {
                    Some(start) => list_stack.push(ListMarker::Ordered(start)),
                    None => list_stack.push(ListMarker::Bullet),
                }
            }
            MdEvent::Start(Tag::Item) => {
                list_item_spans_stack.push(Vec::new());
                list_item_insert_idx.push(list_items.len());
            }
            MdEvent::End(TagEnd::Item) => {
                let item_spans = list_item_spans_stack.pop().unwrap_or_default();
                let insert_at = list_item_insert_idx.pop().unwrap_or(list_items.len());
                let depth = (list_stack.len() as u32).saturating_sub(1);
                let marker = match list_stack.last_mut() {
                    Some(ListMarker::Ordered(n)) => {
                        let m = ListMarker::Ordered(*n);
                        *n += 1;
                        m
                    }
                    Some(ListMarker::Bullet) => ListMarker::Bullet,
                    None => ListMarker::Bullet,
                };
                list_items.insert(insert_at, ListItem {
                    marker,
                    depth,
                    spans: item_spans,
                });
            }
            MdEvent::End(TagEnd::List(_)) => {
                list_stack.pop();
                if list_stack.is_empty() {
                    let items = std::mem::take(&mut list_items);
                    if !items.is_empty() {
                        blocks.push(Block::List { items });
                    }
                }
            }
            MdEvent::SoftBreak | MdEvent::HardBreak => {
                let target = if in_table {
                    &mut cell_spans
                } else if !list_item_spans_stack.is_empty() {
                    list_item_spans_stack.last_mut().unwrap()
                } else {
                    &mut spans
                };
                target.push(Span {
                    text: " ".to_string(),
                    style: SpanStyle::Normal,
                });
            }
            _ => {}
        }
    }

    // Flush any remaining uncommitted spans
    if !spans.is_empty() {
        blocks.push(Block::Paragraph { spans });
    }

    blocks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{SAMPLE_MD, SAMPLE_WITH_META};
    use pulldown_cmark::HeadingLevel;

    #[test]
    fn parse_basic_structure() {
        let blocks = parse_markdown(SAMPLE_MD);
        assert!(blocks.len() >= 8, "expected at least 8 blocks, got {}", blocks.len());
        assert!(matches!(blocks[0], Block::Heading { level: HeadingLevel::H1, .. }));
    }

    #[test]
    fn parse_inline_styles() {
        let blocks = parse_markdown("Hello **bold** and *italic* and `code`.");
        assert_eq!(blocks.len(), 1);
        if let Block::Paragraph { spans } = &blocks[0] {
            let styles: Vec<_> = spans.iter().map(|s| &s.style).collect();
            assert!(styles.contains(&&SpanStyle::Bold));
            assert!(styles.contains(&&SpanStyle::Italic));
            assert!(styles.contains(&&SpanStyle::Code));
        } else {
            panic!("expected Paragraph");
        }
    }

    #[test]
    fn parse_metadata_block() {
        let blocks = parse_markdown(SAMPLE_WITH_META);
        assert!(matches!(&blocks[0], Block::Metadata { entries } if entries.len() == 2));
        if let Block::Metadata { entries } = &blocks[0] {
            assert_eq!(entries[0], ("title".to_string(), "Test Doc".to_string()));
            assert_eq!(entries[1], ("author".to_string(), "Tester".to_string()));
        }
    }

    #[test]
    fn parse_code_block() {
        let blocks = parse_markdown("```\nline1\nline2\n```\n");
        assert_eq!(blocks.len(), 1);
        if let Block::CodeBlock { text } = &blocks[0] {
            assert!(text.contains("line1"));
            assert!(text.contains("line2"));
        } else {
            panic!("expected CodeBlock");
        }
    }

    #[test]
    fn parse_table() {
        let blocks = parse_markdown("| A | B |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |\n");
        assert_eq!(blocks.len(), 1);
        if let Block::Table { headers, rows } = &blocks[0] {
            assert_eq!(headers.len(), 2);
            assert_eq!(rows.len(), 2);
        } else {
            panic!("expected Table");
        }
    }

    #[test]
    fn parse_unordered_list() {
        let blocks = parse_markdown("- Alpha\n- Beta\n- Gamma\n");
        let list = blocks.iter().find(|b| matches!(b, Block::List { .. }));
        assert!(list.is_some(), "should produce a List block");
        if let Block::List { items } = list.unwrap() {
            assert_eq!(items.len(), 3);
            for item in items {
                assert!(matches!(item.marker, crate::types::ListMarker::Bullet));
                assert_eq!(item.depth, 0);
            }
            assert_eq!(items[0].spans[0].text, "Alpha");
        }
    }

    #[test]
    fn parse_ordered_list() {
        let blocks = parse_markdown("1. One\n2. Two\n3. Three\n");
        let list = blocks.iter().find(|b| matches!(b, Block::List { .. }));
        assert!(list.is_some());
        if let Block::List { items } = list.unwrap() {
            assert_eq!(items.len(), 3);
            assert!(matches!(items[0].marker, crate::types::ListMarker::Ordered(1)));
            assert!(matches!(items[1].marker, crate::types::ListMarker::Ordered(2)));
            assert!(matches!(items[2].marker, crate::types::ListMarker::Ordered(3)));
        }
    }

    #[test]
    fn parse_nested_list() {
        let blocks = parse_markdown("- Top\n    - Nested\n        - Deep\n- Back\n");
        let list = blocks.iter().find(|b| matches!(b, Block::List { .. }));
        assert!(list.is_some());
        if let Block::List { items } = list.unwrap() {
            assert_eq!(items.len(), 4);
            assert_eq!(items[0].depth, 0);
            assert_eq!(items[1].depth, 1);
            assert_eq!(items[2].depth, 2);
            assert_eq!(items[3].depth, 0);
        }
    }

    #[test]
    fn metadata_missing() {
        let (entries, rest) = parse_metadata("# Just a heading\n");
        assert!(entries.is_empty());
        assert_eq!(rest, "# Just a heading\n");
    }

    #[test]
    fn metadata_unclosed() {
        let (entries, rest) = parse_metadata("---\nkey: val\nno closing fence\n");
        assert!(entries.is_empty());
        assert!(rest.contains("key: val"));
    }
}
