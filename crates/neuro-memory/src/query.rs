//! Search and explain over a [`BlockIndex`](crate::index::BlockIndex).
//!
//! [`search`] runs a Tantivy [`QueryParser`](tantivy::query::QueryParser) on the
//! `text` and `heading_path` fields and collects [`TopDocs`](tantivy::collector::TopDocs).
//! [`explain`] returns the per-field score breakdown and the matched snippets
//! from [`SnippetGenerator`](tantivy::snippet::SnippetGenerator).
//!
//! [`crate::MemoryService::search`] and [`crate::MemoryService::explain`] call
//! [`search`] and [`explain`].

use crate::index::{BlockIndex, IndexError};
use crate::model::{ScoreComponent, SearchExplain, SearchRequest, SearchResult};
use serde::Deserialize;
use std::collections::BTreeSet;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Occur, Query, QueryParser, QueryParserError, TermQuery};
use tantivy::schema::{Field, IndexRecordOption, TantivyDocument, Term};
use tantivy::snippet::SnippetGenerator;
use tantivy::{DocAddress, ReloadPolicy, Searcher, TantivyError};

const FIELD_TEXT: &str = "text";
const FIELD_HEADING_PATH: &str = "heading_path";

/// Highlighted fragment length passed to [`SnippetGenerator::set_max_num_chars`].
const SNIPPET_MAX_CHARS: usize = 200;

/// Failure from [`search`] or [`explain`].
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error(transparent)]
    Index(#[from] IndexError),
    #[error("failed to parse query {query:?}: {source}")]
    Parse {
        query: String,
        #[source]
        source: QueryParserError,
    },
    #[error(transparent)]
    Tantivy(#[from] TantivyError),
    #[error("could not read the score explanation: {source}")]
    ExplainFormat {
        #[source]
        source: serde_json::Error,
    },
    #[error("block {block_id} does not match query {query:?}")]
    NotMatched { block_id: String, query: String },
}

/// Hits for `request`, highest score first.
///
/// Only committed blocks are visible. Call [`explain`] for the breakdown and
/// snippets. `limit == 0` or a blank query returns no hits. A query that
/// Tantivy rejects is [`QueryError::Parse`].
pub fn search(
    index: &BlockIndex,
    request: &SearchRequest,
) -> Result<Vec<SearchResult>, QueryError> {
    if request.limit == 0 || request.query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let parsed = parse_user_query(index, &request.query)?;
    let searcher = open_searcher(index)?;
    let hits = searcher.search(parsed.as_ref(), &TopDocs::with_limit(request.limit))?;
    let mut results = Vec::with_capacity(hits.len());
    for (score, address) in hits {
        let doc: TantivyDocument = searcher.doc(address)?;
        let block = index.block_from_stored(&doc)?;
        results.push(SearchResult {
            block_id: block.block_id,
            page_url: block.page_url,
            heading_path: block.heading_path,
            text: block.text,
            score,
            captured_at: block.captured_at,
        });
    }
    Ok(results)
}

/// Score breakdown and matched snippets for `result` under `request`.
///
/// Snippets are HTML. Tantivy wraps each match in `<b>` tags. The breakdown
/// has one [`ScoreComponent`] per matching field (`text`, then `heading_path`).
pub fn explain(
    index: &BlockIndex,
    request: &SearchRequest,
    result: &SearchResult,
) -> Result<SearchExplain, QueryError> {
    if request.query.trim().is_empty() {
        return Err(not_matched(request, result));
    }
    let parsed = parse_user_query(index, &request.query)?;
    let searcher = open_searcher(index)?;
    let address = find_match(index, &searcher, &request.query, &result.block_id)?;
    let doc: TantivyDocument = searcher.doc(address)?;
    let stored = index.block_from_stored(&doc)?;
    if stored.block_id != result.block_id {
        return Err(not_matched(request, result));
    }
    let explanation = parsed.explain(&searcher, address)?;
    let breakdown = score_breakdown(index, &request.query, &searcher, address, &explanation)?;
    let snippets = matched_snippets(
        &searcher,
        parsed.as_ref(),
        &doc,
        &[index.text_field(), index.heading_path_field()],
    )?;
    Ok(SearchExplain {
        breakdown,
        snippets,
    })
}

fn not_matched(request: &SearchRequest, result: &SearchResult) -> QueryError {
    QueryError::NotMatched {
        block_id: result.block_id.clone(),
        query: request.query.clone(),
    }
}

fn parse_user_query(index: &BlockIndex, query: &str) -> Result<Box<dyn Query>, QueryError> {
    let parser = QueryParser::for_index(
        index.tantivy_index(),
        vec![index.text_field(), index.heading_path_field()],
    );
    parser
        .parse_query(query)
        .map_err(|source| QueryError::Parse {
            query: query.to_string(),
            source,
        })
}

fn open_searcher(index: &BlockIndex) -> Result<Searcher, QueryError> {
    let reader = index
        .tantivy_index()
        .reader_builder()
        .reload_policy(ReloadPolicy::Manual)
        .try_into()?;
    Ok(reader.searcher())
}

fn find_match(
    index: &BlockIndex,
    searcher: &Searcher,
    query: &str,
    block_id: &str,
) -> Result<DocAddress, QueryError> {
    let user_query = parse_user_query(index, query)?;
    let block_query = TermQuery::new(
        Term::from_field_text(index.block_id_field(), block_id),
        IndexRecordOption::Basic,
    );
    let filtered = BooleanQuery::new(vec![
        (Occur::Must, user_query),
        (Occur::Must, Box::new(block_query)),
    ]);
    let hits = searcher.search(&filtered, &TopDocs::with_limit(1))?;
    hits.into_iter()
        .next()
        .map(|(_score, address)| address)
        .ok_or_else(|| QueryError::NotMatched {
            block_id: block_id.to_string(),
            query: query.to_string(),
        })
}

fn matched_snippets(
    searcher: &Searcher,
    query: &dyn Query,
    doc: &TantivyDocument,
    fields: &[Field],
) -> Result<Vec<String>, QueryError> {
    let mut snippets = Vec::new();
    for field in fields {
        let mut generator = SnippetGenerator::create(searcher, query, *field)?;
        generator.set_max_num_chars(SNIPPET_MAX_CHARS);
        let snippet = generator.snippet_from_doc(doc);
        if snippet.is_empty() {
            continue;
        }
        snippets.push(snippet.to_html());
    }
    Ok(snippets)
}

fn score_breakdown(
    index: &BlockIndex,
    query: &str,
    searcher: &Searcher,
    address: DocAddress,
    explanation: &tantivy::query::Explanation,
) -> Result<Vec<ScoreComponent>, QueryError> {
    let parsed: ExplanationNode = serde_json::from_str(&explanation.to_pretty_json())
        .map_err(|source| QueryError::ExplainFormat { source })?;
    let from_tree = aggregate(contributions(
        &parsed,
        index.text_field().field_id(),
        index.heading_path_field().field_id(),
    ));
    if !from_tree.is_empty() {
        return Ok(from_tree);
    }
    // Phrase scorers do not record a term field on the explanation node.
    // Re-parse against one field at a time and explain that query instead.
    breakdown_by_field(index, query, searcher, address)
}

fn breakdown_by_field(
    index: &BlockIndex,
    query: &str,
    searcher: &Searcher,
    address: DocAddress,
) -> Result<Vec<ScoreComponent>, QueryError> {
    let mut components = Vec::new();
    for (name, field) in [
        (FIELD_TEXT, index.text_field()),
        (FIELD_HEADING_PATH, index.heading_path_field()),
    ] {
        let parser = QueryParser::for_index(index.tantivy_index(), vec![field]);
        let field_query = parser
            .parse_query(query)
            .map_err(|source| QueryError::Parse {
                query: query.to_string(),
                source,
            })?;
        if !terms_target_only(field_query.as_ref(), field) {
            continue;
        }
        match field_query.explain(searcher, address) {
            Ok(explanation) => components.push(ScoreComponent {
                field: name.to_string(),
                score: explanation.value(),
            }),
            Err(TantivyError::InvalidArgument(message)) if message.contains("does not match") => {}
            Err(error) => return Err(QueryError::Tantivy(error)),
        }
    }
    Ok(components)
}

fn terms_target_only(query: &dyn Query, field: Field) -> bool {
    let mut saw_term = false;
    let mut only = true;
    query.query_terms(&mut |term, _in_phrase| {
        saw_term = true;
        if term.field() != field {
            only = false;
        }
    });
    saw_term && only
}

#[derive(Debug, Deserialize)]
struct ExplanationNode {
    value: f32,
    #[serde(default)]
    details: Vec<ExplanationNode>,
    #[serde(default)]
    context: Vec<String>,
}

fn contributions(
    node: &ExplanationNode,
    text_id: u32,
    heading_id: u32,
) -> Vec<(&'static str, f32)> {
    let fields = known_fields(node, text_id, heading_id);
    match fields.as_slice() {
        [field] => vec![(*field, node.value)],
        [] => Vec::new(),
        _ => node
            .details
            .iter()
            .flat_map(|child| contributions(child, text_id, heading_id))
            .collect(),
    }
}

fn known_fields(node: &ExplanationNode, text_id: u32, heading_id: u32) -> Vec<&'static str> {
    let mut ids = BTreeSet::new();
    collect_field_ids(node, &mut ids);
    let mut names = Vec::new();
    if ids.contains(&text_id) {
        names.push(FIELD_TEXT);
    }
    if ids.contains(&heading_id) {
        names.push(FIELD_HEADING_PATH);
    }
    names
}

fn collect_field_ids(node: &ExplanationNode, ids: &mut BTreeSet<u32>) {
    for line in &node.context {
        push_field_ids(line, ids);
    }
    for child in &node.details {
        collect_field_ids(child, ids);
    }
}

fn push_field_ids(text: &str, ids: &mut BTreeSet<u32>) {
    let marker = "Term(field=";
    let mut rest = text;
    while let Some(index) = rest.find(marker) {
        rest = &rest[index + marker.len()..];
        let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
        if let Ok(id) = digits.parse::<u32>() {
            ids.insert(id);
        }
        if digits.is_empty() {
            break;
        }
    }
}

fn aggregate(pairs: Vec<(&'static str, f32)>) -> Vec<ScoreComponent> {
    let mut text = 0.0;
    let mut heading = 0.0;
    let mut saw_text = false;
    let mut saw_heading = false;
    for (field, score) in pairs {
        if field == FIELD_TEXT {
            text += score;
            saw_text = true;
        } else if field == FIELD_HEADING_PATH {
            heading += score;
            saw_heading = true;
        }
    }
    let mut components = Vec::new();
    if saw_text {
        components.push(ScoreComponent {
            field: FIELD_TEXT.to_string(),
            score: text,
        });
    }
    if saw_heading {
        components.push(ScoreComponent {
            field: FIELD_HEADING_PATH.to_string(),
            score: heading,
        });
    }
    components
}
