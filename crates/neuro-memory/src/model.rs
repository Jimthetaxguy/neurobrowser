//! Core records for persistent page memory.
//!
//! Timestamps are Unix epoch milliseconds (`u64`) taken from [`std::time::SystemTime`],
//! the same clock and width as `neurobrowser::agent::memory::AgentEvent::now`.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

/// Milliseconds since the Unix epoch.
///
/// A clock set before the epoch yields `0`.
pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

/// Failure from [`crate::MemoryService`].
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MemoryError {
    /// [`crate::CapturePolicy`] refused this page. The store and index are unchanged.
    #[error("capture denied: {reason}")]
    Denied { reason: String },

    /// The page store could not read or write a page.
    #[error("page store: {message}")]
    Store { message: String },

    /// The block index could not open, update, or commit.
    #[error("block index: {message}")]
    Index { message: String },

    /// Search or explain failed.
    #[error("query: {message}")]
    Query { message: String },
}

/// One captured page, before block extraction.
///
/// M1.4 stores one JSON document per page, keyed by [`Self::content_hash`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedPage {
    pub url: Url,
    pub title: String,
    /// Raw HTML. M1.3 reads this with `scraper`.
    pub html: String,
    /// Plain text, used when HTML yields no blocks.
    pub text: String,
    /// SHA-256 hex of the stored body. M1.4 computes this with `sha2`.
    pub content_hash: String,
    /// Unix epoch milliseconds. See [`now_millis`].
    pub captured_at: u64,
}

/// A heading-bounded chunk of a [`CapturedPage`].
///
/// M1.3 builds these. M1.5 indexes `block_id`, `page_url`, `heading_path`,
/// `text`, and `captured_at`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticBlock {
    pub block_id: String,
    pub page_url: Url,
    /// Outermost heading first. Empty for the paragraph fallback.
    pub heading_path: Vec<String>,
    pub text: String,
    pub captured_at: u64,
}

/// A query against the page index.
///
/// M1.6 parses `query` over the `text` and `heading_path` fields and collects
/// at most `limit` hits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub limit: usize,
}

/// One hit from [`crate::MemoryService::search`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub block_id: String,
    pub page_url: Url,
    pub heading_path: Vec<String>,
    pub text: String,
    pub score: f32,
    pub captured_at: u64,
}

/// Why a hit scored the way it did.
///
/// M1.6 fills this from Tantivy's score explanation and `SnippetGenerator`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchExplain {
    pub breakdown: Vec<ScoreComponent>,
    pub snippets: Vec<String>,
}

/// One term in a [`SearchExplain`] score breakdown.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScoreComponent {
    pub field: String,
    pub score: f32,
}
