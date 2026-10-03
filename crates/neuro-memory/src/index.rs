//! Tantivy index of [`SemanticBlock`]s.
//!
//! The writer sits behind a [`Mutex`]. Callers add or delete, then [`BlockIndex::commit`].
//! Full-text search lives in [`crate::query`]. [`BlockIndex::blocks_for_url`] only reads blocks
//! already committed for one exact page URL.
//!
//! `page_url` is a raw string (`STRING | STORED`), not tokenized text, so
//! [`BlockIndex::remove_by_url`] deletes on the exact URL. `block_id` and `heading_path`
//! are stored as well as indexed so a committed block can be rebuilt. `captured_at` is a
//! fast `u64` of Unix epoch milliseconds, and it is stored for the same reason.

use crate::model::SemanticBlock;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use tantivy::collector::{Count, TopDocs};
use tantivy::directory::MmapDirectory;
use tantivy::query::{BooleanQuery, Occur, Query, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, OwnedValue, Schema, TantivyDocument, Term, FAST, STORED, STRING, TEXT,
};
use tantivy::{Index, IndexWriter, ReloadPolicy};
use url::Url;

const FIELD_BLOCK_ID: &str = "block_id";
const FIELD_PAGE_URL: &str = "page_url";
const FIELD_HEADING_PATH: &str = "heading_path";
const FIELD_TEXT: &str = "text";
const FIELD_CAPTURED_AT: &str = "captured_at";
const PAGE_MARKER: &str = "__captured_page__";

/// Per-thread minimum Tantivy accepts (`MARGIN_IN_BYTES * 15`).
const WRITER_HEAP_BYTES: usize = 15_000_000;
const WRITER_THREADS: usize = 1;

/// Cap for [`BlockIndex::blocks_for_url`]. A captured page stays far under this.
const MAX_BLOCKS_PER_URL: usize = 10_000;
const MAX_CAPTURE_HASHES_PER_URL: usize = 10_000;

/// Failure from [`BlockIndex`].
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("failed to create index directory {}: {source}", path.display())]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    OpenDirectory(#[from] tantivy::directory::error::OpenDirectoryError),
    #[error(transparent)]
    Tantivy(#[from] tantivy::TantivyError),
    #[error("index writer lock poisoned")]
    Poisoned,
    #[error("stored field `{field}` is missing or has an unexpected type")]
    BadField { field: &'static str },
    #[error("stored page_url `{url}` is not a URL")]
    BadUrl {
        url: String,
        #[source]
        source: url::ParseError,
    },
    #[error("page {url} has {count} blocks, above the {limit} lookup limit")]
    TooManyBlocks {
        url: String,
        count: usize,
        limit: usize,
    },
    #[error("page {url} has {count} retained captures, above the {limit} cleanup limit")]
    TooManyCaptures {
        url: String,
        count: usize,
        limit: usize,
    },
}

struct Fields {
    block_id: Field,
    page_url: Field,
    heading_path: Field,
    text: Field,
    captured_at: Field,
}

impl Fields {
    fn from_schema(schema: &Schema) -> Result<Self, IndexError> {
        Ok(Self {
            block_id: schema.get_field(FIELD_BLOCK_ID)?,
            page_url: schema.get_field(FIELD_PAGE_URL)?,
            heading_path: schema.get_field(FIELD_HEADING_PATH)?,
            text: schema.get_field(FIELD_TEXT)?,
            captured_at: schema.get_field(FIELD_CAPTURED_AT)?,
        })
    }
}

fn schema() -> Schema {
    let mut builder = Schema::builder();
    builder.add_text_field(FIELD_BLOCK_ID, STRING | STORED);
    builder.add_text_field(FIELD_PAGE_URL, STRING | STORED);
    builder.add_text_field(FIELD_HEADING_PATH, TEXT | STORED);
    builder.add_text_field(FIELD_TEXT, TEXT | STORED);
    builder.add_u64_field(FIELD_CAPTURED_AT, FAST | STORED);
    builder.build()
}

/// On-disk Tantivy index of semantic blocks.
///
/// `dir` is the index directory itself.
pub struct BlockIndex {
    index: Index,
    writer: Mutex<IndexWriter>,
    fields: Fields,
}

impl std::fmt::Debug for BlockIndex {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("BlockIndex").finish_non_exhaustive()
    }
}

impl BlockIndex {
    /// Open the index at `dir`, or create it with the block schema.
    ///
    /// Creates `dir` when it is missing. An existing index with a different schema
    /// is left in place and returned as an error. A second writer on the same
    /// directory fails with Tantivy's lock error.
    pub fn open_or_create(dir: impl AsRef<Path>) -> Result<Self, IndexError> {
        let dir = dir.as_ref();
        std::fs::create_dir_all(dir).map_err(|source| IndexError::CreateDir {
            path: dir.to_path_buf(),
            source,
        })?;
        let index = Index::open_or_create(MmapDirectory::open(dir)?, schema())?;
        let fields = Fields::from_schema(&index.schema())?;
        let writer = index.writer_with_num_threads(WRITER_THREADS, WRITER_HEAP_BYTES)?;
        Ok(Self {
            index,
            writer: Mutex::new(writer),
            fields,
        })
    }

    /// Schema written by [`BlockIndex::open_or_create`].
    pub fn schema(&self) -> Schema {
        self.index.schema()
    }

    pub(crate) fn tantivy_index(&self) -> &Index {
        &self.index
    }

    pub(crate) fn block_id_field(&self) -> Field {
        self.fields.block_id
    }

    pub(crate) fn heading_path_field(&self) -> Field {
        self.fields.heading_path
    }

    pub(crate) fn text_field(&self) -> Field {
        self.fields.text
    }

    pub(crate) fn block_from_stored(
        &self,
        doc: &TantivyDocument,
    ) -> Result<SemanticBlock, IndexError> {
        self.block_from_doc(doc)
    }

    /// Queue `block`. It is not searchable until [`BlockIndex::commit`].
    pub fn add_block(&self, block: &SemanticBlock) -> Result<(), IndexError> {
        let mut doc = TantivyDocument::default();
        doc.add_text(self.fields.block_id, &block.block_id);
        doc.add_text(self.fields.page_url, block.page_url.as_str());
        for heading in &block.heading_path {
            doc.add_text(self.fields.heading_path, heading);
        }
        doc.add_text(self.fields.text, &block.text);
        doc.add_u64(self.fields.captured_at, block.captured_at);
        self.writer()?.add_document(doc)?;
        Ok(())
    }

    // A second block_id term marks metadata without changing the disk schema.
    // The first stored ID retains the existing {hash}:{suffix} convention.
    pub(crate) fn add_page_marker(&self, page_url: &Url, hash: &str) -> Result<(), IndexError> {
        let mut doc = TantivyDocument::default();
        doc.add_text(self.fields.block_id, format!("{hash}:page"));
        doc.add_text(self.fields.block_id, PAGE_MARKER);
        doc.add_text(self.fields.page_url, page_url.as_str());
        self.writer()?.add_document(doc)?;
        Ok(())
    }

    pub(crate) fn remove_page_marker(&self, hash: &str) -> Result<(), IndexError> {
        self.writer()?.delete_term(Term::from_field_text(
            self.fields.block_id,
            &format!("{hash}:page"),
        ));
        Ok(())
    }

    // Legacy semantic blocks already contain their page hash in block_id.
    pub(crate) fn page_hashes_for_url(&self, page_url: &Url) -> Result<Vec<String>, IndexError> {
        let mut hashes = BTreeSet::new();
        let mut docs = self.documents_for_url(page_url, true)?;
        if docs.is_empty() {
            // Old indexes have only semantic blocks; new indexes need read
            // only the small marker set, never the current page's full text.
            docs = self.documents_for_url(page_url, false)?;
        }
        for doc in docs {
            let id = required_str(&doc, self.fields.block_id, FIELD_BLOCK_ID)?;
            let (hash, _) = id.split_once(':').ok_or(IndexError::BadField {
                field: FIELD_BLOCK_ID,
            })?;
            if !crate::store::is_content_hash(hash) {
                return Err(IndexError::BadField {
                    field: FIELD_BLOCK_ID,
                });
            }
            hashes.insert(hash.to_string());
        }
        Ok(hashes.into_iter().collect())
    }

    pub(crate) fn exclude_page_markers(&self, query: Box<dyn Query>) -> Box<dyn Query> {
        let marker = TermQuery::new(
            Term::from_field_text(self.fields.block_id, PAGE_MARKER),
            IndexRecordOption::Basic,
        );
        Box::new(BooleanQuery::new(vec![
            (Occur::Must, query),
            (Occur::MustNot, Box::new(marker)),
        ]))
    }

    /// Queue deletion of every block whose page URL equals `page_url`.
    ///
    /// The deletion is visible after [`BlockIndex::commit`]. Blocks added after
    /// this call in the same commit are kept, so a recapture can delete and
    /// then add before one commit.
    pub fn remove_by_url(&self, page_url: &Url) -> Result<(), IndexError> {
        let term = Term::from_field_text(self.fields.page_url, page_url.as_str());
        self.writer()?.delete_term(term);
        Ok(())
    }

    /// Publish queued adds and deletes.
    pub fn commit(&self) -> Result<(), IndexError> {
        self.writer()?.commit()?;
        Ok(())
    }

    /// Blocks committed for `page_url`, in index order.
    ///
    /// Uncommitted adds and deletes are omitted. This is an exact URL lookup.
    pub fn blocks_for_url(&self, page_url: &Url) -> Result<Vec<SemanticBlock>, IndexError> {
        self.documents_for_url(page_url, false)?
            .iter()
            .map(|doc| self.block_from_doc(doc))
            .collect()
    }

    fn documents_for_url(
        &self,
        page_url: &Url,
        markers_only: bool,
    ) -> Result<Vec<TantivyDocument>, IndexError> {
        let reader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;
        let searcher = reader.searcher();
        let term = Term::from_field_text(self.fields.page_url, page_url.as_str());
        let mut query: Box<dyn Query> = Box::new(TermQuery::new(term, IndexRecordOption::Basic));
        if markers_only {
            let marker = TermQuery::new(
                Term::from_field_text(self.fields.block_id, PAGE_MARKER),
                IndexRecordOption::Basic,
            );
            query = Box::new(BooleanQuery::new(vec![
                (Occur::Must, query),
                (Occur::Must, Box::new(marker)),
            ]));
        } else {
            query = self.exclude_page_markers(query);
        }
        let count = searcher.search(query.as_ref(), &Count)?;
        if count == 0 {
            return Ok(Vec::new());
        }
        let limit = if markers_only {
            MAX_CAPTURE_HASHES_PER_URL
        } else {
            MAX_BLOCKS_PER_URL
        };
        if count > limit {
            return Err(if markers_only {
                IndexError::TooManyCaptures {
                    url: page_url.to_string(),
                    count,
                    limit,
                }
            } else {
                IndexError::TooManyBlocks {
                    url: page_url.to_string(),
                    count,
                    limit,
                }
            });
        }
        let hits = searcher.search(query.as_ref(), &TopDocs::with_limit(count))?;
        hits.into_iter()
            .map(|(_, address)| searcher.doc(address).map_err(IndexError::from))
            .collect()
    }

    fn writer(&self) -> Result<MutexGuard<'_, IndexWriter>, IndexError> {
        self.writer.lock().map_err(|_| IndexError::Poisoned)
    }

    fn block_from_doc(&self, doc: &TantivyDocument) -> Result<SemanticBlock, IndexError> {
        let block_id = required_str(doc, self.fields.block_id, FIELD_BLOCK_ID)?;
        let page_url_raw = required_str(doc, self.fields.page_url, FIELD_PAGE_URL)?;
        let page_url = Url::parse(&page_url_raw).map_err(|source| IndexError::BadUrl {
            url: page_url_raw,
            source,
        })?;
        let heading_path = doc
            .get_all(self.fields.heading_path)
            .map(|value| match value {
                OwnedValue::Str(text) => Ok(text.clone()),
                _ => Err(IndexError::BadField {
                    field: FIELD_HEADING_PATH,
                }),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let text = required_str(doc, self.fields.text, FIELD_TEXT)?;
        let captured_at = match doc.get_first(self.fields.captured_at) {
            Some(OwnedValue::U64(millis)) => *millis,
            _ => {
                return Err(IndexError::BadField {
                    field: FIELD_CAPTURED_AT,
                })
            }
        };
        Ok(SemanticBlock {
            block_id,
            page_url,
            heading_path,
            text,
            captured_at,
        })
    }
}

fn required_str(
    doc: &TantivyDocument,
    field: Field,
    name: &'static str,
) -> Result<String, IndexError> {
    match doc.get_first(field) {
        Some(OwnedValue::Str(value)) => Ok(value.clone()),
        _ => Err(IndexError::BadField { field: name }),
    }
}
