use neuro_memory::index::BlockIndex;
use neuro_memory::{
    explain, extract_blocks, search, CapturedPage, QueryError, ScoreComponent, SearchRequest,
};
use std::path::Path;
use url::Url;

fn page(url: &str, html: &str, hash: &str, captured_at: u64) -> CapturedPage {
    CapturedPage {
        url: Url::parse(url).expect("fixture url"),
        title: "Guide".to_string(),
        html: html.to_string(),
        text: String::new(),
        content_hash: hash.to_string(),
        captured_at,
    }
}

fn request(query: &str, limit: usize) -> SearchRequest {
    SearchRequest {
        query: query.to_string(),
        limit,
    }
}

fn index_pages(dir: &Path, pages: &[CapturedPage]) -> BlockIndex {
    let index = BlockIndex::open_or_create(dir).expect("create index");
    for captured in pages {
        for block in extract_blocks(captured) {
            index.add_block(&block).expect("queue block");
        }
    }
    index.commit().expect("commit");
    index
}

fn component<'a>(explain: &'a [ScoreComponent], field: &str) -> &'a ScoreComponent {
    explain
        .iter()
        .find(|component| component.field == field)
        .unwrap_or_else(|| panic!("missing {field} in {explain:?}"))
}

fn assert_score_close(left: f32, right: f32) {
    let diff = (left - right).abs();
    let scale = left.abs().max(right.abs()).max(1.0);
    assert!(
        diff <= scale * 1.0e-4,
        "scores {left} and {right} differ by {diff}"
    );
}

#[test]
fn search_ranks_committed_text() {
    let dir = tempfile::tempdir().expect("temp dir");
    let guide = page(
        "https://example.com/docs?topic=install",
        "<h1>Guide</h1><p>uniquealpha once.</p><h2>Install</h2><p>uniquealpha uniquealpha uniquealpha uniquealpha uniquealpha.</p>",
        "guide-hash",
        1_700_000_000_000,
    );
    let other = page(
        "https://example.com/notes",
        "<h1>Notes</h1><p>No shared token here.</p>",
        "notes-hash",
        1_700_000_111_111,
    );
    let index = BlockIndex::open_or_create(dir.path()).expect("create index");
    let guide_blocks = extract_blocks(&guide);
    assert!(
        guide_blocks.len() >= 2,
        "heading split should yield two blocks"
    );
    for block in &guide_blocks {
        index.add_block(block).expect("queue guide");
    }
    let hidden = search(&index, &request("uniquealpha", 10)).expect("search uncommitted");
    assert!(hidden.is_empty(), "uncommitted blocks must stay hidden");
    index.commit().expect("commit guide");

    for block in extract_blocks(&other) {
        index.add_block(&block).expect("queue notes");
    }
    index.commit().expect("commit notes");

    let hits = search(&index, &request("uniquealpha", 10)).expect("search");
    assert_eq!(hits.len(), 2, "both guide blocks mention uniquealpha");
    assert!(hits.iter().all(|hit| hit.score > 0.0));
    assert!(hits[0].score >= hits[1].score);
    assert!(
        hits[0].text.matches("uniquealpha").count() > hits[1].text.matches("uniquealpha").count(),
        "the repeated block should rank first: {hits:?}"
    );
    assert_eq!(hits[0].page_url, guide.url);
    assert!(hits[0]
        .heading_path
        .iter()
        .any(|heading| heading == "Install"));
    assert_eq!(hits[0].captured_at, guide.captured_at);
    assert!(hits[0].block_id.starts_with("guide-hash:"));

    let top = search(&index, &request("uniquealpha", 1)).expect("limit");
    assert_eq!(top.len(), 1);
    assert_eq!(top[0].block_id, hits[0].block_id);

    assert!(search(&index, &request("uniquealpha", 0))
        .expect("limit zero")
        .is_empty());
    assert!(search(&index, &request("   ", 5))
        .expect("blank")
        .is_empty());
    assert!(search(&index, &request("missingtoken", 5))
        .expect("miss")
        .is_empty());

    let removed = guide.url.clone();
    index.remove_by_url(&removed).expect("queue delete");
    let still_there = search(&index, &request("uniquealpha", 10)).expect("delete hidden");
    assert_eq!(
        still_there.len(),
        2,
        "an uncommitted delete stays searchable"
    );
    index.commit().expect("commit delete");
    assert!(search(&index, &request("uniquealpha", 10))
        .expect("deleted")
        .is_empty());

    drop(index);
    let reopened = BlockIndex::open_or_create(dir.path()).expect("reopen");
    assert!(search(&reopened, &request("uniquealpha", 10))
        .expect("reopened search")
        .is_empty());
    let notes = search(&reopened, &request("shared", 5)).expect("notes remain");
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].page_url.as_str(), "https://example.com/notes");
    assert!(notes[0].text.contains("No shared token"));
}

#[test]
fn search_matches_heading_path_when_the_body_does_not() {
    let dir = tempfile::tempdir().expect("temp dir");
    let captured = page(
        "https://example.com/zephyr",
        "<h1>Zephyrheading</h1><p>Body without that token.</p>",
        "zephyr-hash",
        1_700_000_222_222,
    );
    let index = index_pages(dir.path(), &[captured]);
    let hits = search(&index, &request("zephyrheading", 5)).expect("search heading");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].heading_path, vec!["Zephyrheading".to_string()]);
    assert!(!hits[0].text.to_lowercase().contains("zephyrheading"));
    assert!(hits[0].text.contains("Body without that token."));
}

#[test]
fn explain_breaks_down_text_and_heading_scores_with_snippets() {
    let dir = tempfile::tempdir().expect("temp dir");
    let captured = page(
        "https://example.com/both",
        "<h1>uniqueboth</h1><p>uniqueboth appears in the body too.</p><h2>Elsewhere</h2><p>Nothing to see.</p>",
        "both-hash",
        1_700_000_333_333,
    );
    let index = index_pages(dir.path(), &[captured]);
    let hits = search(&index, &request("uniqueboth", 5)).expect("search");
    assert_eq!(
        hits.len(),
        2,
        "the h1 stays on the nested section's heading path"
    );
    let hit = hits
        .iter()
        .find(|hit| hit.text.contains("uniqueboth"))
        .expect("body hit");
    let nested = hits
        .iter()
        .find(|hit| hit.heading_path == ["uniqueboth".to_string(), "Elsewhere".to_string()])
        .expect("nested hit");
    assert!(nested.text.contains("Nothing to see."));
    let nested_explain = explain(&index, &request("uniqueboth", 5), nested).expect("nested");
    assert_eq!(nested_explain.breakdown.len(), 1, "{nested_explain:?}");
    assert_eq!(nested_explain.breakdown[0].field, "heading_path");
    assert_score_close(nested_explain.breakdown[0].score, nested.score);

    let explained = explain(&index, &request("uniqueboth", 5), hit).expect("explain");
    assert_eq!(explained.breakdown.len(), 2, "{explained:?}");
    let text = component(&explained.breakdown, "text");
    let heading = component(&explained.breakdown, "heading_path");
    assert!(text.score > 0.0);
    assert!(heading.score > 0.0);
    assert_score_close(text.score + heading.score, hit.score);
    assert!(
        explained
            .snippets
            .iter()
            .any(|snippet| snippet.contains("<b>uniqueboth</b>")),
        "snippets should highlight the match: {explained:?}"
    );
    assert!(explained.snippets.len() >= 2, "{explained:?}");
}

#[test]
fn explain_phrase_and_fielded_queries() {
    let dir = tempfile::tempdir().expect("temp dir");
    let phrase_page = page(
        "https://example.com/phrase",
        "<h1>Bravo Charlie</h1><p>alpha bravo charlie delta.</p>",
        "phrase-hash",
        1_700_000_444_444,
    );
    let alpha = page(
        "https://example.com/alpha",
        "<h1>Other</h1><p>alphatoken sits in the body.</p>",
        "alpha-hash",
        1_700_000_555_555,
    );
    let beta = page(
        "https://example.com/beta",
        "<h1>Betatoken</h1><p>plain body without the marker.</p>",
        "beta-hash",
        1_700_000_666_666,
    );
    let index = index_pages(dir.path(), &[phrase_page, alpha, beta]);

    let phrase_hits = search(&index, &request("\"bravo charlie\"", 5)).expect("phrase search");
    assert_eq!(phrase_hits.len(), 1);
    let phrase_explain =
        explain(&index, &request("\"bravo charlie\"", 5), &phrase_hits[0]).expect("phrase explain");
    assert!(
        phrase_explain
            .breakdown
            .iter()
            .any(|component| component.field == "text" && component.score > 0.0),
        "{phrase_explain:?}"
    );
    assert!(
        phrase_explain
            .breakdown
            .iter()
            .any(|component| component.field == "heading_path" && component.score > 0.0),
        "{phrase_explain:?}"
    );
    let phrase_sum: f32 = phrase_explain
        .breakdown
        .iter()
        .map(|component| component.score)
        .sum();
    assert_score_close(phrase_sum, phrase_hits[0].score);
    assert!(
        phrase_explain.snippets.iter().any(|snippet| {
            snippet.contains("<b>bravo</b>") && snippet.contains("<b>charlie</b>")
        }),
        "{phrase_explain:?}"
    );
    assert!(
        phrase_explain.snippets.iter().any(|snippet| {
            snippet.contains("<b>Bravo</b>") && snippet.contains("<b>Charlie</b>")
        }),
        "{phrase_explain:?}"
    );

    let query = "text:alphatoken OR heading_path:betatoken";
    let fielded = search(&index, &request(query, 5)).expect("fielded search");
    assert_eq!(fielded.len(), 2, "{fielded:?}");
    let alpha_hit = fielded
        .iter()
        .find(|hit| hit.page_url.as_str() == "https://example.com/alpha")
        .expect("alpha hit");
    let beta_hit = fielded
        .iter()
        .find(|hit| hit.page_url.as_str() == "https://example.com/beta")
        .expect("beta hit");
    let alpha_explain = explain(&index, &request(query, 5), alpha_hit).expect("alpha explain");
    assert_eq!(alpha_explain.breakdown.len(), 1, "{alpha_explain:?}");
    assert_eq!(alpha_explain.breakdown[0].field, "text");
    assert_score_close(alpha_explain.breakdown[0].score, alpha_hit.score);
    assert!(alpha_explain
        .snippets
        .iter()
        .any(|snippet| snippet.contains("<b>alphatoken</b>")));
    let beta_explain = explain(&index, &request(query, 5), beta_hit).expect("beta explain");
    assert_eq!(beta_explain.breakdown.len(), 1, "{beta_explain:?}");
    assert_eq!(beta_explain.breakdown[0].field, "heading_path");
    assert_score_close(beta_explain.breakdown[0].score, beta_hit.score);
    assert!(beta_explain
        .snippets
        .iter()
        .any(|snippet| snippet.contains("<b>Betatoken</b>")));
}

#[test]
fn explain_rejects_a_block_the_query_does_not_match() {
    let dir = tempfile::tempdir().expect("temp dir");
    let captured = page(
        "https://example.com/docs",
        "<h1>Guide</h1><p>uniquealpha once.</p>",
        "guide-hash",
        1_700_000_777_777,
    );
    let index = index_pages(dir.path(), &[captured]);
    let hits = search(&index, &request("uniquealpha", 5)).expect("search");
    assert_eq!(hits.len(), 1);

    let mut missing = hits[0].clone();
    missing.block_id = "guide-hash:99".to_string();
    let missing_err = explain(&index, &request("uniquealpha", 5), &missing).unwrap_err();
    assert!(
        matches!(missing_err, QueryError::NotMatched { .. }),
        "{missing_err}"
    );

    let wrong_query = request("missingtoken", 5);
    let wrong_err = explain(&index, &wrong_query, &hits[0]).unwrap_err();
    assert!(
        matches!(wrong_err, QueryError::NotMatched { .. }),
        "{wrong_err}"
    );

    let blank = explain(&index, &request("  ", 5), &hits[0]).unwrap_err();
    assert!(matches!(blank, QueryError::NotMatched { .. }), "{blank}");

    let syntax = search(&index, &request("\"", 5)).unwrap_err();
    assert!(matches!(syntax, QueryError::Parse { .. }), "{syntax}");
}
