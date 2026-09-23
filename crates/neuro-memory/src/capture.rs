//! Heading-bounded block extraction.
//!
//! [`extract_blocks`] reads [`CapturedPage::html`] with `scraper` and emits
//! [`SemanticBlock`]s. [`CapturedPage`] already carries `url`, `html`, `text`,
//! `content_hash`, and `captured_at`, so this milestone does not change `model`.

use crate::model::{CapturedPage, SemanticBlock};
use scraper::{ElementRef, Html, Node, Selector};

/// Maximum characters (Unicode scalar values) in one [`SemanticBlock::text`].
///
/// A heading section is packed with `\n\n` between paragraphs until another
/// paragraph would pass this limit. A single paragraph longer than the limit
/// is split on whitespace, then on a character boundary when one token does
/// not fit.
pub const MAX_BLOCK_CHARS: usize = 1_000;

const PARAGRAPH_GAP: &str = "\n\n";

/// Split `page` into heading-bounded blocks.
///
/// The walk keeps an `h1`–`h6` stack. A new heading pops every heading of
/// equal or deeper rank, then becomes the innermost segment.
/// [`SemanticBlock::heading_path`] is outermost first. Heading labels are not
/// copied into `text`.
///
/// `script`, `style`, `noscript`, `template`, `svg`, `canvas`, `iframe`, and
/// `head` are skipped. When the HTML yields no text, `page.text` is split on
/// blank lines and packed under an empty heading path.
///
/// `block_id` is `{content_hash}:{ordinal}` with a zero-based ordinal.
pub fn extract_blocks(page: &CapturedPage) -> Vec<SemanticBlock> {
    let mut pieces = extract_html_pieces(&page.html);
    if pieces.is_empty() {
        pieces = pack_paragraphs(paragraphs_from_plain(&page.text))
            .into_iter()
            .map(|text| (Vec::new(), text))
            .collect();
    }

    pieces
        .into_iter()
        .enumerate()
        .map(|(ordinal, (heading_path, text))| SemanticBlock {
            block_id: format!("{}:{ordinal}", page.content_hash),
            page_url: page.url.clone(),
            heading_path,
            text,
            captured_at: page.captured_at,
        })
        .collect()
}

fn extract_html_pieces(html: &str) -> Vec<(Vec<String>, String)> {
    if html.trim().is_empty() {
        return Vec::new();
    }

    let document = Html::parse_document(html);
    let selector = Selector::parse("body").expect("valid body selector");
    let Some(body) = document.select(&selector).next() else {
        return Vec::new();
    };

    let mut state = State::default();
    walk_element(body, &mut state);
    state.flush_section();

    let mut pieces = Vec::new();
    for (path, paragraphs) in state.sections {
        for text in pack_paragraphs(paragraphs) {
            pieces.push((path.clone(), text));
        }
    }
    pieces
}

#[derive(Default)]
struct State {
    stack: Vec<(u8, String)>,
    paragraphs: Vec<String>,
    sections: Vec<(Vec<String>, Vec<String>)>,
}

impl State {
    fn push_heading(&mut self, level: u8, text: String) {
        self.flush_section();
        while self
            .stack
            .last()
            .is_some_and(|(current, _)| *current >= level)
        {
            self.stack.pop();
        }
        if !text.is_empty() {
            self.stack.push((level, text));
        }
    }

    fn flush_section(&mut self) {
        if self.paragraphs.is_empty() {
            return;
        }
        let path = self
            .stack
            .iter()
            .map(|(_, text)| text.clone())
            .collect::<Vec<_>>();
        let paragraphs = std::mem::take(&mut self.paragraphs);
        self.sections.push((path, paragraphs));
    }
}

fn walk_element(element: ElementRef, state: &mut State) {
    let name = element.value().name();
    if is_skipped(name) {
        return;
    }
    if let Some(level) = heading_level(name) {
        state.push_heading(level, normalize_ws(&element.text().collect::<String>()));
        return;
    }
    if name == "pre" {
        let text = element.text().collect::<String>();
        let text = text.trim();
        if !text.is_empty() {
            state.paragraphs.push(text.to_string());
        }
        return;
    }
    walk_children(element, state);
}

fn walk_children(parent: ElementRef, state: &mut State) {
    let mut inline = String::new();
    for child in parent.children() {
        match child.value() {
            Node::Text(text) => inline.push_str(text),
            Node::Element(_) => {
                let Some(child_element) = ElementRef::wrap(child) else {
                    continue;
                };
                let name = child_element.value().name();
                if is_skipped(name) {
                    continue;
                }
                if is_inline(name) && !contains_block(child_element) {
                    append_inline(child_element, &mut inline);
                } else {
                    flush_inline(&mut inline, state);
                    walk_element(child_element, state);
                }
            }
            _ => {}
        }
    }
    flush_inline(&mut inline, state);
}

fn append_inline(element: ElementRef, buf: &mut String) {
    if element.value().name() == "br" {
        buf.push(' ');
        return;
    }
    for child in element.children() {
        match child.value() {
            Node::Text(text) => buf.push_str(text),
            Node::Element(_) => {
                let Some(child_element) = ElementRef::wrap(child) else {
                    continue;
                };
                if is_skipped(child_element.value().name()) {
                    continue;
                }
                append_inline(child_element, buf);
            }
            _ => {}
        }
    }
}

fn flush_inline(buf: &mut String, state: &mut State) {
    let text = normalize_ws(buf);
    buf.clear();
    if !text.is_empty() {
        state.paragraphs.push(text);
    }
}

fn contains_block(element: ElementRef) -> bool {
    element.children().any(|child| {
        let Some(child_element) = ElementRef::wrap(child) else {
            return false;
        };
        let name = child_element.value().name();
        if is_skipped(name) {
            return false;
        }
        if heading_level(name).is_some() || !is_inline(name) {
            return true;
        }
        contains_block(child_element)
    })
}

fn heading_level(name: &str) -> Option<u8> {
    match name {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

fn is_skipped(name: &str) -> bool {
    matches!(
        name,
        "script" | "style" | "noscript" | "template" | "svg" | "canvas" | "iframe" | "head"
    )
}

fn is_inline(name: &str) -> bool {
    matches!(
        name,
        "a" | "abbr"
            | "b"
            | "bdi"
            | "bdo"
            | "br"
            | "cite"
            | "code"
            | "data"
            | "dfn"
            | "em"
            | "i"
            | "kbd"
            | "mark"
            | "q"
            | "rp"
            | "rt"
            | "ruby"
            | "s"
            | "samp"
            | "small"
            | "span"
            | "strong"
            | "sub"
            | "sup"
            | "time"
            | "u"
            | "var"
            | "wbr"
    )
}

fn normalize_ws(raw: &str) -> String {
    let mut out = String::new();
    for word in raw.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

fn paragraphs_from_plain(text: &str) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        let line = normalize_ws(line);
        if line.is_empty() {
            if !current.is_empty() {
                paragraphs.push(std::mem::take(&mut current));
            }
            continue;
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(&line);
    }
    if !current.is_empty() {
        paragraphs.push(current);
    }
    paragraphs
}

fn pack_paragraphs(paragraphs: Vec<String>) -> Vec<String> {
    let gap = PARAGRAPH_GAP.chars().count();
    let mut packed = Vec::new();
    let mut current = String::new();
    for paragraph in paragraphs {
        for piece in split_to_limit(&paragraph) {
            if current.is_empty() {
                current = piece;
                continue;
            }
            if current.chars().count() + gap + piece.chars().count() > MAX_BLOCK_CHARS {
                packed.push(std::mem::take(&mut current));
                current = piece;
            } else {
                current.push_str(PARAGRAPH_GAP);
                current.push_str(&piece);
            }
        }
    }
    if !current.is_empty() {
        packed.push(current);
    }
    packed
}

fn split_to_limit(text: &str) -> Vec<String> {
    const _: () = assert!(MAX_BLOCK_CHARS > 0);

    let mut chunks = Vec::new();
    let mut rest = text.trim();
    if rest.is_empty() {
        return chunks;
    }

    while rest.chars().count() > MAX_BLOCK_CHARS {
        let mut end = 0;
        let mut last_break = None;
        for (count, (index, ch)) in rest.char_indices().enumerate() {
            if count == MAX_BLOCK_CHARS {
                break;
            }
            end = index + ch.len_utf8();
            if ch.is_whitespace() {
                last_break = Some(end);
            }
        }
        let cut = match last_break {
            Some(at) if at > 0 && at < rest.len() => at,
            _ => end,
        };
        if cut == 0 {
            break;
        }
        let (head, tail) = rest.split_at(cut);
        let head = head.trim();
        if !head.is_empty() {
            chunks.push(head.to_string());
        }
        let next = tail.trim_start();
        if next.len() >= rest.len() {
            break;
        }
        rest = next;
    }
    if !rest.is_empty() {
        chunks.push(rest.to_string());
    }
    chunks
}
