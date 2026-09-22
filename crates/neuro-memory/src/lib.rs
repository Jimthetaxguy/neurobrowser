//! Persistent page memory for NeuroBrowser.
//!
//! [`MemoryService`] is durable page memory. It is separate from in-run agent
//! memory (`neurobrowser::agent::memory::AgentMemory`).
//!
//! M1.1 scaffolded the types and service stubs. M1.2 adds [`CapturePolicy`].
//! M1.3 adds [`extract_blocks`]. Capture, search, explain, and forget stay
//! stubs until their milestones. This crate is not a Cargo workspace member.
//! The root and `src-tauri` crates take a path dependency in M1.7.
//!
//! Time is Unix epoch milliseconds (`u64`) via [`now_millis`].

pub mod capture;
pub mod model;
pub mod policy;

// Later milestones declare these modules next to `model`:
// M1.4 pub mod store;
// M1.5 pub mod index;
// M1.6 pub mod query;

pub use capture::{extract_blocks, MAX_BLOCK_CHARS};
pub use model::{
    now_millis, CapturedPage, MemoryError, ScoreComponent, SearchExplain, SearchRequest,
    SearchResult, SemanticBlock,
};
pub use policy::{CaptureDecision, CapturePolicy};

use std::path::{Path, PathBuf};
use url::Url;

/// Durable memory rooted at a data directory.
///
/// The app passes `app_data_dir()/memory/`. [`MemoryService::open`] only records
/// that path. Capture, search, explain, and forget stay stubs until their milestones.
#[derive(Debug)]
pub struct MemoryService {
    data_dir: PathBuf,
}

impl MemoryService {
    /// Remember `data_dir` for the page store and the Tantivy index.
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self, MemoryError> {
        // M1.7 fill-in: create the directory and open the store and index.
        Ok(Self {
            data_dir: data_dir.as_ref().to_path_buf(),
        })
    }

    /// Directory passed to [`MemoryService::open`].
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Store `page` and index its blocks.
    pub async fn capture(&self, page: CapturedPage) -> Result<(), MemoryError> {
        // M1.7 fill-in: policy (M1.2), extract blocks (M1.3), page store (M1.4), index (M1.5).
        let _ = (page, self.data_dir());
        Err(MemoryError::NotImplemented { task: "M1.7" })
    }

    /// Search indexed blocks.
    pub async fn search(&self, request: SearchRequest) -> Result<Vec<SearchResult>, MemoryError> {
        // M1.6 fill-in: query parser and TopDocs. M1.7 wires the call.
        let _ = (request, self.data_dir());
        Err(MemoryError::NotImplemented { task: "M1.6" })
    }

    /// Score breakdown and matched snippets for one hit.
    pub async fn explain(
        &self,
        request: &SearchRequest,
        result: &SearchResult,
    ) -> Result<SearchExplain, MemoryError> {
        // M1.6 fill-in: Tantivy explain and SnippetGenerator.
        let _ = (request, result, self.data_dir());
        Err(MemoryError::NotImplemented { task: "M1.6" })
    }

    /// Drop indexed blocks for `page_url`.
    pub async fn forget(&self, page_url: &Url) -> Result<(), MemoryError> {
        // M1.7 fill-in: remove_by_url (M1.5) and tombstone the domain (M1.2).
        let _ = (page_url, self.data_dir());
        Err(MemoryError::NotImplemented { task: "M1.7" })
    }
}
