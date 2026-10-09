//! Persistent page memory for NeuroBrowser.
//!
//! [`MemoryService`] is durable page memory.
//! [`MemoryService::blocks_for_url`] reads the blocks committed for one page.
//! This crate is not a Cargo workspace member. The root and `src-tauri` crates
//! depend on it by path.
//!
//! Time is Unix epoch milliseconds (`u64`) via [`now_millis`].

pub mod capture;
pub mod index;
pub mod model;
pub mod policy;
pub mod query;
pub mod store;

pub use capture::{extract_blocks, MAX_BLOCK_CHARS};
pub use model::{
    now_millis, CapturedPage, MemoryError, ScoreComponent, SearchExplain, SearchRequest,
    SearchResult, SemanticBlock,
};
pub use policy::{CaptureDecision, CapturePolicy};
pub use query::{explain, search, QueryError};

use crate::index::{BlockIndex, IndexError};
use crate::store::{PageStore, StoreError};
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;
use url::Url;

/// Durable page memory rooted at a data directory.
///
/// `neuro_memory::MemoryService` keeps captured pages on disk.
/// The app passes `app_data_dir()/memory/`. [`MemoryService::open`] creates that
/// directory and opens `{data_dir}/pages` plus `{data_dir}/index`.
#[derive(Debug)]
pub struct MemoryService {
    data_dir: PathBuf,
    store: PageStore,
    index: BlockIndex,
    // Keep each store/index update and obsolete-file cleanup in one operation.
    mutations: Mutex<()>,
}

impl MemoryService {
    /// Create `data_dir` and open the page store and the Tantivy index under it.
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self, MemoryError> {
        let data_dir = data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&data_dir).map_err(|err| MemoryError::Store {
            message: format!("create {}: {err}", data_dir.display()),
        })?;
        let store = PageStore::open(&data_dir).map_err(store_error)?;
        let index = BlockIndex::open_or_create(data_dir.join("index")).map_err(index_error)?;
        Ok(Self {
            data_dir,
            store,
            index,
            mutations: Mutex::new(()),
        })
    }

    /// Directory passed to [`MemoryService::open`].
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Store `page` and index its blocks when `policy` allows the URL.
    ///
    /// A deny leaves storage unchanged. `content_hash` includes `captured_at`.
    /// Capture and forget operations are serialized.
    pub async fn capture(
        &self,
        page: CapturedPage,
        policy: &CapturePolicy,
    ) -> Result<(), MemoryError> {
        let _mutation = self.mutations.lock().await;
        if let CaptureDecision::Deny { reason } = policy.evaluate(page.url.as_str()) {
            return Err(MemoryError::Denied { reason });
        }

        let prior_hashes = self
            .index
            .page_hashes_for_url(&page.url)
            .map_err(index_error)?;
        let stored = self.store.put(page).await.map_err(store_error)?;
        let blocks = extract_blocks(&stored);
        self.index.remove_by_url(&stored.url).map_err(index_error)?;
        self.index
            .add_page_marker(&stored.url, &stored.content_hash)
            .map_err(index_error)?;
        let obsolete: Vec<_> = prior_hashes
            .into_iter()
            .filter(|hash| *hash != stored.content_hash)
            .collect();
        for hash in &obsolete {
            self.index
                .add_page_marker(&stored.url, hash)
                .map_err(index_error)?;
        }
        for block in &blocks {
            self.index.add_block(block).map_err(index_error)?;
        }
        self.index.commit().map_err(index_error)?;
        // Publish retained deletion candidates with the new blocks before
        // removing files; a failed delete must not lose its recovery metadata.
        for hash in &obsolete {
            self.store.delete(hash).await.map_err(store_error)?;
        }
        if !obsolete.is_empty() {
            for hash in &obsolete {
                self.index.remove_page_marker(hash).map_err(index_error)?;
            }
            self.index.commit().map_err(index_error)?;
        }
        Ok(())
    }

    /// Search indexed blocks. Delegates to [`search`].
    pub async fn search(&self, request: SearchRequest) -> Result<Vec<SearchResult>, MemoryError> {
        search(&self.index, &request).map_err(query_error)
    }

    /// Blocks committed for `page_url` by the last capture of that exact URL.
    ///
    /// Uncommitted adds are omitted. An unknown URL is an empty list, not an error.
    pub async fn blocks_for_url(&self, page_url: &Url) -> Result<Vec<SemanticBlock>, MemoryError> {
        self.index.blocks_for_url(page_url).map_err(index_error)
    }

    /// Score breakdown and matched snippets for one hit. Delegates to [`explain`].
    pub async fn explain(
        &self,
        request: &SearchRequest,
        result: &SearchResult,
    ) -> Result<SearchExplain, MemoryError> {
        explain(&self.index, request, result).map_err(query_error)
    }

    /// Tombstone `page_url` and drop its stored pages and indexed blocks.
    ///
    /// When the URL has a host, that host is appended to
    /// [`CapturePolicy::denied_domains`] unless an equivalent rule is already
    /// present. A later [`MemoryService::capture`] with this policy then denies
    /// the domain. Store files whose page URL equals `page_url` are removed,
    /// and the index drops blocks for that exact URL.
    pub async fn forget(
        &self,
        page_url: &Url,
        policy: &mut CapturePolicy,
    ) -> Result<(), MemoryError> {
        let _mutation = self.mutations.lock().await;
        tombstone_host(policy, page_url);
        self.index.remove_by_url(page_url).map_err(index_error)?;
        self.index.commit().map_err(index_error)?;
        self.store
            .delete_by_url(page_url)
            .await
            .map_err(store_error)?;
        Ok(())
    }
}

fn store_error(err: StoreError) -> MemoryError {
    MemoryError::Store {
        message: err.to_string(),
    }
}

fn index_error(err: IndexError) -> MemoryError {
    MemoryError::Index {
        message: err.to_string(),
    }
}

fn query_error(err: QueryError) -> MemoryError {
    MemoryError::Query {
        message: err.to_string(),
    }
}

/// Push the lowercased host onto `policy.denied_domains` when that exact host is absent.
///
/// The rule is trimmed, a leading dot is stripped, and the comparison is ASCII
/// case-insensitive, matching [`CapturePolicy::evaluate`].
fn tombstone_host(policy: &mut CapturePolicy, page_url: &Url) {
    let Some(host) = page_url.host_str() else {
        return;
    };
    let host = host.to_lowercase();
    let already = policy.denied_domains.iter().any(|rule| {
        rule.trim()
            .trim_start_matches('.')
            .eq_ignore_ascii_case(&host)
    });
    if !already {
        policy.denied_domains.push(host);
    }
}
