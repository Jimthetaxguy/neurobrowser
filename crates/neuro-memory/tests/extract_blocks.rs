use neuro_memory::{extract_blocks, CapturedPage, SemanticBlock, MAX_BLOCK_CHARS};
use url::Url;

const OUTLINE: &str = include_str!("fixtures/outline.html");
const PARAGRAPHS: &str = include_str!("fixtures/paragraphs.html");
const HEADING_WS: &str = include_str!("fixtures/heading_ws.html");
const PRE: &str = include_str!("fixtures/pre.html");
const TABLE: &str = include_str!("fixtures/table.html");
const BROKEN: &str = include_str!("fixtures/broken.html");
const SCRIPT_ONLY: &str = include_str!("fixtures/script_only.html");
const HEADINGS_ONLY: &str = include_str!("fixtures/headings_only.html");
const WHITESPACE: &str = include_str!("fixtures/whitespace.html");
const ARTICLE: &str = include_str!("fixtures/article.txt");
const LONG_SECTION: &str = include_str!("fixtures/long_section.html");
const OVERLONG_HTML: &str = include_str!("fixtures/overlong.html");
const OVERLONG_TEXT: &str = include_str!("fixtures/overlong.txt");
const OVERLONG_TOKEN: &str = include_str!("fixtures/overlong_token.html");

fn page(html: &str, text: &str) -> CapturedPage {
    CapturedPage {
        url: Url::parse("https://example.com/guide").expect("fixture url"),
        title: "Guide".to_string(),
        html: html.to_string(),
        text: text.to_string(),
        content_hash: "pagehash".to_string(),
        captured_at: 1_700_000_000_000,
    }
}

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_string()).collect()
}

fn assert_page_fields(blocks: &[SemanticBlock]) {
    let mut ids = Vec::new();
    for (ordinal, block) in blocks.iter().enumerate() {
        let len = block.text.chars().count();
        assert!(len > 0, "empty block {ordinal}");
        assert!(
            len <= MAX_BLOCK_CHARS,
            "block {ordinal} has {len} chars (max {MAX_BLOCK_CHARS})"
        );
        assert_eq!(block.block_id, format!("pagehash:{ordinal}"));
        assert_eq!(block.page_url.as_str(), "https://example.com/guide");
        assert_eq!(block.captured_at, 1_700_000_000_000);
        ids.push(block.block_id.clone());
    }
    let mut unique = ids.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), ids.len());
}

#[test]
fn outline_tracks_heading_path_and_skips_non_content() {
    let captured = page(OUTLINE, "PLAIN_SHOULD_NOT_APPEAR");
    let blocks = extract_blocks(&captured);
    assert_eq!(blocks, extract_blocks(&captured));
    assert_page_fields(&blocks);

    let expected = [
        (path(&[]), "Preamble text.".to_string()),
        (path(&["H1"]), "Paragraph one.".to_string()),
        (path(&["H1", "H2"]), "Paragraph two.".to_string()),
        (path(&["H1", "H2", "H3"]), "Paragraph three.".to_string()),
        (
            path(&["H1", "H2", "H3", "H4"]),
            "Paragraph four.".to_string(),
        ),
        (
            path(&["H1", "H2", "H3", "H4", "H5"]),
            "Paragraph five.".to_string(),
        ),
        (
            path(&["H1", "H2", "H3", "H4", "H5", "H6"]),
            "Paragraph six.".to_string(),
        ),
        (
            path(&["H1", "H2b"]),
            "Paragraph two b.\n\nItem alpha\n\nItem beta".to_string(),
        ),
        (path(&["Appendix"]), "See notes.".to_string()),
    ];

    assert_eq!(blocks.len(), expected.len());
    for (block, (heading_path, text)) in blocks.iter().zip(expected) {
        assert_eq!(block.heading_path, heading_path);
        assert_eq!(block.text, text);
        assert!(!block.text.contains("SECRET_SCRIPT"));
        assert!(!block.text.contains("SECRET_STYLE"));
        assert!(!block.text.contains("Ignored Title"));
        assert!(!block.text.contains("PLAIN_SHOULD_NOT_APPEAR"));
    }
}

#[test]
fn paragraphs_without_headings_have_an_empty_path() {
    let blocks = extract_blocks(&page(PARAGRAPHS, ""));
    assert_page_fields(&blocks);
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].heading_path.is_empty());
    assert_eq!(
        blocks[0].text,
        "Alpha one.\n\nBeta two continues.\n\nGamma three.\n\nTom & Jerry"
    );
}

#[test]
fn heading_whitespace_is_collapsed_into_the_path() {
    let blocks = extract_blocks(&page(HEADING_WS, ""));
    assert_page_fields(&blocks);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].heading_path, path(&["Hello World"]));
    assert_eq!(blocks[0].text, "Body text.");
}

#[test]
fn pre_preserves_internal_whitespace() {
    let blocks = extract_blocks(&page(PRE, ""));
    assert_page_fields(&blocks);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].heading_path, path(&["Sample"]));
    assert_eq!(
        blocks[0].text,
        "fn main() {\n    println!(\"hi\");\n}\n\nAfter."
    );
}

#[test]
fn table_cells_follow_the_heading() {
    let blocks = extract_blocks(&page(TABLE, ""));
    assert_page_fields(&blocks);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].heading_path, path(&["Prices"]));
    assert_eq!(blocks[0].text, "Item\n\nCost\n\nTea\n\n$2");
}

#[test]
fn long_section_splits_on_paragraph_boundaries() {
    let para_a = "A".repeat(600);
    let para_b = "B".repeat(600);
    assert!(
        LONG_SECTION.contains(&para_a) && LONG_SECTION.contains(&para_b),
        "long_section fixture drifted"
    );
    assert!(
        para_a.chars().count() + 2 + para_b.chars().count() > MAX_BLOCK_CHARS,
        "the two long paragraphs must not fit in one block"
    );
    assert!(
        para_b.chars().count() + 2 + "Tail.".chars().count() <= MAX_BLOCK_CHARS,
        "Tail. must pack onto the second paragraph"
    );

    let blocks = extract_blocks(&page(LONG_SECTION, ""));
    assert_page_fields(&blocks);
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0].heading_path, path(&["Section"]));
    assert_eq!(blocks[1].heading_path, path(&["Section"]));
    assert_eq!(blocks[0].text, para_a);
    assert_eq!(blocks[1].text, format!("{para_b}\n\nTail."));
}

#[test]
fn overlong_paragraph_splits_on_whitespace_without_cutting_tokens() {
    let source = OVERLONG_TEXT.trim();
    assert!(OVERLONG_HTML.contains(source));
    assert!(source.chars().count() > MAX_BLOCK_CHARS);

    let blocks = extract_blocks(&page(OVERLONG_HTML, ""));
    assert_page_fields(&blocks);
    assert!(blocks.len() >= 2);
    assert!(blocks
        .iter()
        .all(|block| block.heading_path == path(&["Long"])));

    let extracted: Vec<&str> = blocks
        .iter()
        .flat_map(|block| block.text.split_whitespace())
        .filter(|word| *word != "Tail.")
        .collect();
    let expected: Vec<&str> = source.split_whitespace().collect();
    assert_eq!(extracted, expected);
    assert!(blocks.iter().any(|block| block.text.ends_with("Tail.")));
    assert!(extracted.iter().all(|word| word.len() == 5));
}

#[test]
fn overlong_token_splits_on_character_boundaries() {
    let token = "é".repeat(2500);
    assert!(OVERLONG_TOKEN.contains(&token));
    assert!(token.chars().count() > MAX_BLOCK_CHARS);

    let blocks = extract_blocks(&page(OVERLONG_TOKEN, ""));
    assert_page_fields(&blocks);
    assert!(blocks
        .iter()
        .all(|block| block.heading_path == path(&["Token"])));
    assert_eq!(
        blocks.len(),
        token.chars().count().div_ceil(MAX_BLOCK_CHARS)
    );
    assert_eq!(
        blocks
            .iter()
            .map(|block| block.text.as_str())
            .collect::<String>(),
        token
    );
    let last = blocks.len() - 1;
    for (index, block) in blocks.iter().enumerate() {
        let len = block.text.chars().count();
        if index == last && !token.chars().count().is_multiple_of(MAX_BLOCK_CHARS) {
            assert_eq!(len, token.chars().count() % MAX_BLOCK_CHARS);
        } else {
            assert_eq!(len, MAX_BLOCK_CHARS);
        }
    }
}

#[test]
fn plain_text_fallback_joins_blank_line_paragraphs() {
    let blocks = extract_blocks(&page(SCRIPT_ONLY, ARTICLE));
    assert_page_fields(&blocks);
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].heading_path.is_empty());
    assert_eq!(
        blocks[0].text,
        "First paragraph lives here.\n\nSecond paragraph lives here."
    );
    assert!(!blocks[0].text.contains("SECRET_SCRIPT"));
    assert!(!blocks[0].text.contains("SECRET_STYLE"));
}

#[test]
fn plain_text_fallback_also_splits_at_max_length() {
    let source = OVERLONG_TEXT.trim();
    let blocks = extract_blocks(&page(SCRIPT_ONLY, source));
    assert_page_fields(&blocks);
    assert!(blocks.len() >= 2);
    assert!(blocks.iter().all(|block| block.heading_path.is_empty()));
    let extracted: Vec<&str> = blocks
        .iter()
        .flat_map(|block| block.text.split_whitespace())
        .collect();
    assert_eq!(extracted, source.split_whitespace().collect::<Vec<_>>());
}

#[test]
fn headings_without_prose_fall_back_to_plain_text() {
    let blocks = extract_blocks(&page(HEADINGS_ONLY, ARTICLE));
    assert_page_fields(&blocks);
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].heading_path.is_empty());
    assert_eq!(
        blocks[0].text,
        "First paragraph lives here.\n\nSecond paragraph lives here."
    );
}

#[test]
fn whitespace_html_is_not_a_block() {
    assert!(extract_blocks(&page(WHITESPACE, "  \n")).is_empty());
    let blocks = extract_blocks(&page(WHITESPACE, "Fallback line."));
    assert_page_fields(&blocks);
    assert_eq!(blocks.len(), 1);
    assert!(blocks[0].heading_path.is_empty());
    assert_eq!(blocks[0].text, "Fallback line.");
}

#[test]
fn empty_page_yields_no_blocks() {
    assert!(extract_blocks(&page("", "")).is_empty());
}

#[test]
fn broken_html_still_extracts_visible_text() {
    let blocks = extract_blocks(&page(BROKEN, ""));
    assert_page_fields(&blocks);
    let text = blocks
        .iter()
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Hello there"));
    assert!(text.contains("Loose text"));
}
