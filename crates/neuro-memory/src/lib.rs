//! Persistent page memory for NeuroBrowser.
//!
//! [`MemoryService`] is durable page memory. It is separate from in-run agent
//! memory (`neurobrowser::agent::memory::AgentMemory`).
//!
//! M1.1 scaffolded the types. M1.2 adds [`CapturePolicy`]. M1.3 adds
//! [`extract_blocks`]. M1.4 adds the page store ([`store::PageStore`]). M1.5
//! adds the Tantivy block index ([`index::BlockIndex`]). M1.6 adds [`search`]
//! and [`explain`]. M1.7 wires those pieces into [`MemoryService`].
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
use url::Url;

/// Durable page memory rooted at a data directory.
///
/// `neuro_memory::MemoryService` keeps captured pages on disk. In-run agent
/// memory lives in `neurobrowser::agent::memory::AgentMemory`.
///
/// The app passes `app_data_dir()/memory/`. [`MemoryService::open`] creates that
/// directory and opens `{data_dir}/pages` plus `{data_dir}/index`.
#[derive(Debug)]
pub struct MemoryService {
    data_dir: PathBuf,
    store: PageStore,
    index: BlockIndex,
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
        })
    }

    /// Directory passed to [`MemoryService::open`].
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Store `page` and index its blocks when `policy` allows the URL.
    ///
    /// [`CapturePolicy::evaluate`] runs first. A deny leaves the store and the
    /// index unchanged and returns [`MemoryError::Denied`]. On allow, the page
    /// is written to the store (which assigns [`CapturedPage::content_hash`]),
    /// blocks are extracted from that stored page, and those blocks replace any
    /// previously indexed blocks for the same URL. The index commit makes the
    /// new blocks searchable.
    pub async fn capture(
        &self,
        page: CapturedPage,
        policy: &CapturePolicy,
    ) -> Result<(), MemoryError> {
        if let CaptureDecision::Deny { reason } = policy.evaluate(page.url.as_str()) {
            return Err(MemoryError::Denied { reason });
        }

        let stored = self.store.put(page).await.map_err(store_error)?;
        let blocks = extract_blocks(&stored);
        self.index.remove_by_url(&stored.url).map_err(index_error)?;
        for block in &blocks {
            self.index.add_block(block).map_err(index_error)?;
        }
        self.index.commit().map_err(index_error)?;
        Ok(())
    }

    /// Search indexed blocks. Delegates to [`search`].
    pub async fn search(&self, request: SearchRequest) -> Result<Vec<SearchResult>, MemoryError> {
        search(&self.index, &request).map_err(query_error)
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
        tombstone_host(policy, page_url);
        self.index.remove_by_url(page_url).map_err(index_error)?;
        self.index.commit().map_err(index_error)?;
        remove_stored_pages(&self.store, page_url).await?;
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

async fn remove_stored_pages(store: &PageStore, page_url: &Url) -> Result<(), MemoryError> {
    let mut hashes = Vec::new();
    let mut dir = tokio::fs::read_dir(store.pages_dir())
        .await
        .map_err(|err| MemoryError::Store {
            message: format!("read {}: {err}", store.pages_dir().display()),
        })?;
    while let Some(entry) = dir.next_entry().await.map_err(|err| MemoryError::Store {
        message: format!("read {}: {err}", store.pages_dir().display()),
    })? {
        let Some(name) = entry.file_name().into_string().ok() else {
            continue;
        };
        let Some(hash) = name.strip_suffix(".json") else {
            continue;
        };
        if is_page_hash(hash) {
            hashes.push(hash.to_string());
        }
    }

    for hash in hashes {
        let Some(page) = store.get(&hash).await.map_err(store_error)? else {
            continue;
        };
        if page.url == *page_url {
            store.delete(&hash).await.map_err(store_error)?;
        }
    }
    Ok(())
}

fn is_page_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}
