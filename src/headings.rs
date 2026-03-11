use pulldown_cmark::HeadingLevel;

use crate::types::{Block, HeadingInfo};

/// Build heading info list and assign hierarchical numbers.
pub(crate) fn build_headings(blocks: &[Block]) -> Vec<HeadingInfo> {
    let mut headings = Vec::new();
    let mut counters = [0u32; 6];

    for (bi, block) in blocks.iter().enumerate() {
        if let Block::Heading { level, .. } = block {
            let idx = heading_level_index(level);
            counters[idx] += 1;
            for c in &mut counters[idx + 1..] {
                *c = 0;
            }
            let parts: Vec<String> = counters[..=idx].iter().map(|c| c.to_string()).collect();
            headings.push(HeadingInfo {
                block_index: bi,
                level: *level,
                number: format!("{}.", parts.join(".")),
                folded: false,
                y_pos: 0,
                heading_height: 0,
            });
        }
    }
    headings
}

/// Check if block at `block_index` is hidden due to a folded heading.
pub(crate) fn is_block_folded(block_index: usize, headings: &[HeadingInfo]) -> bool {
    for (hi, heading) in headings.iter().enumerate() {
        if !heading.folded {
            continue;
        }
        let fold_level = heading_level_index(&heading.level);
        let start = heading.block_index + 1;

        let end = headings
            .iter()
            .skip(hi + 1)
            .find(|h| heading_level_index(&h.level) <= fold_level)
            .map(|h| h.block_index)
            .unwrap_or(usize::MAX);

        if block_index >= start && block_index < end {
            return true;
        }
    }
    false
}

pub(crate) fn heading_level_index(level: &HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 0,
        HeadingLevel::H2 => 1,
        HeadingLevel::H3 => 2,
        HeadingLevel::H4 => 3,
        HeadingLevel::H5 => 4,
        HeadingLevel::H6 => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsing::parse_markdown;
    use crate::test_helpers::SAMPLE_MD;

    #[test]
    fn heading_numbering() {
        let blocks = parse_markdown(SAMPLE_MD);
        let headings = build_headings(&blocks);
        assert_eq!(headings.len(), 5);
        assert_eq!(headings[0].number, "1.");
        assert_eq!(headings[1].number, "1.1.");
        assert_eq!(headings[2].number, "1.1.1.");
        assert_eq!(headings[3].number, "1.2.");
        assert_eq!(headings[4].number, "1.3.");
    }

    #[test]
    fn heading_level_indices() {
        assert_eq!(heading_level_index(&HeadingLevel::H1), 0);
        assert_eq!(heading_level_index(&HeadingLevel::H3), 2);
        assert_eq!(heading_level_index(&HeadingLevel::H6), 5);
    }

    #[test]
    fn fold_hides_children() {
        let blocks = parse_markdown(SAMPLE_MD);
        let mut headings = build_headings(&blocks);
        headings[1].folded = true;
        let h2_bi = headings[1].block_index;
        let h3_bi = headings[2].block_index;

        assert!(is_block_folded(h2_bi + 1, &headings));
        assert!(is_block_folded(h3_bi, &headings));
        assert!(!is_block_folded(headings[3].block_index, &headings));
    }

    #[test]
    fn fold_h1_hides_everything() {
        let blocks = parse_markdown(SAMPLE_MD);
        let mut headings = build_headings(&blocks);
        headings[0].folded = true;
        for bi in (headings[0].block_index + 1)..blocks.len() {
            assert!(is_block_folded(bi, &headings), "block {} should be folded", bi);
        }
    }
}
