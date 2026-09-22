//! Content-addressed JSON store for [`CapturedPage`] values.
//!
//! [`PageStore::open`] takes the memory data directory and writes one document
//! per page at `{data_dir}/pages/{content_hash}.json`. [`content_hash`] is the
//! lowercase SHA-256 hex of a compact JSON object with keys `url`, `title`,
//! `html`, `text`, and `captured_at`, in that order. The `content_hash` field
//! is not part of that body, so [`PageStore::put`] computes it and stores the
//! result on the page.
//!
//! A put writes a temporary file in `pages/` and renames it into place, so a
//! reader never observes a half-written document. A get checks that the file
//! still hashes to its name. Delete is idempotent. [`crate::MemoryService`]
//! does not call this module yet (M1.7).

use crate::model::CapturedPage;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use thiserror::Error;

const PAGES_DIR: &str = "pages";

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Failure from [`PageStore`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum StoreError {
    /// A filesystem call failed. `message` includes the path and the OS error.
    #[error("page store io: {message}")]
    Io { message: String },

    /// JSON encoding or decoding failed.
    #[error("page store json: {message}")]
    Json { message: String },

    /// The key is not 64 lowercase hexadecimal characters.
    #[error("invalid content hash")]
    InvalidContentHash,

    /// The file named by this hash is not the page that hashes to it.
    #[error("page at {content_hash} does not match its content hash")]
    HashMismatch { content_hash: String },
}

impl StoreError {
    fn io(path: &Path, err: std::io::Error) -> Self {
        Self::Io {
            message: format!("{}: {err}", path.display()),
        }
    }

    fn json(path: &Path, err: impl std::fmt::Display) -> Self {
        Self::Json {
            message: format!("{}: {err}", path.display()),
        }
    }
}

/// JSON page files under `{data_dir}/pages/`.
#[derive(Debug, Clone)]
pub struct PageStore {
    pages_dir: PathBuf,
}

impl PageStore {
    /// Create `{data_dir}/pages/` if it is missing.
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self, StoreError> {
        let pages_dir = data_dir.as_ref().join(PAGES_DIR);
        std::fs::create_dir_all(&pages_dir).map_err(|err| StoreError::io(&pages_dir, err))?;
        Ok(Self { pages_dir })
    }

    /// Directory that holds `{content_hash}.json` files.
    pub fn pages_dir(&self) -> &Path {
        &self.pages_dir
    }

    /// Store `page`, replacing [`CapturedPage::content_hash`] with the body hash.
    ///
    /// The same body writes the same file again. The caller's `content_hash`
    /// is not used as the key.
    pub async fn put(&self, mut page: CapturedPage) -> Result<CapturedPage, StoreError> {
        page.content_hash = content_hash(&page);
        let path = self.page_path(&page.content_hash)?;
        let bytes = encode_page(&page)?;
        write_atomic(&path, &bytes).await?;
        Ok(page)
    }

    /// Load the page named by `hash`.
    ///
    /// `hash` is a [`content_hash`] value. Returns `Ok(None)` when no page is
    /// stored there. A stored page whose body or recorded hash does not match
    /// `hash` is [`StoreError::HashMismatch`].
    pub async fn get(&self, hash: &str) -> Result<Option<CapturedPage>, StoreError> {
        let path = self.page_path(hash)?;
        let bytes = match tokio::fs::read(&path).await {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(StoreError::io(&path, err)),
        };
        let page: CapturedPage =
            serde_json::from_slice(&bytes).map_err(|err| StoreError::json(&path, err))?;
        if page.content_hash != hash || content_hash(&page) != hash {
            return Err(StoreError::HashMismatch {
                content_hash: hash.to_string(),
            });
        }
        Ok(Some(page))
    }

    /// Remove the page stored at `hash`.
    ///
    /// Removing a hash that is not stored succeeds.
    pub async fn delete(&self, hash: &str) -> Result<(), StoreError> {
        let path = self.page_path(hash)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(()),
            Err(err) => Err(StoreError::io(&path, err)),
        }
    }

    fn page_path(&self, content_hash: &str) -> Result<PathBuf, StoreError> {
        if !is_content_hash(content_hash) {
            return Err(StoreError::InvalidContentHash);
        }
        Ok(self.pages_dir.join(format!("{content_hash}.json")))
    }
}

/// Lowercase SHA-256 hex of the canonical page body.
///
/// The body is compact JSON (`url`, `title`, `html`, `text`, `captured_at`).
/// [`CapturedPage::content_hash`] is not an input.
pub fn content_hash(page: &CapturedPage) -> String {
    let body = canonical_body(page);
    hex_encode(Sha256::digest(&body).as_slice())
}

fn canonical_body(page: &CapturedPage) -> Vec<u8> {
    #[derive(Serialize)]
    struct Body<'a> {
        url: &'a str,
        title: &'a str,
        html: &'a str,
        text: &'a str,
        captured_at: u64,
    }

    serde_json::to_vec(&Body {
        url: page.url.as_str(),
        title: &page.title,
        html: &page.html,
        text: &page.text,
        captured_at: page.captured_at,
    })
    .expect("url, title, html, text, and captured_at always serialize to JSON")
}

fn encode_page(page: &CapturedPage) -> Result<Vec<u8>, StoreError> {
    let mut bytes = serde_json::to_vec_pretty(page).map_err(|err| StoreError::Json {
        message: err.to_string(),
    })?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn is_content_hash(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

async fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    let parent = path
        .parent()
        .expect("page path is {content_hash}.json inside pages/");
    let file_name = path
        .file_name()
        .expect("page path is {content_hash}.json inside pages/");
    let tmp = parent.join(format!(
        ".{}.{}.{}.tmp",
        file_name.to_string_lossy(),
        std::process::id(),
        TMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));

    if let Err(err) = tokio::fs::write(&tmp, bytes).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(StoreError::io(&tmp, err));
    }

    match tokio::fs::rename(&tmp, path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == ErrorKind::AlreadyExists => {
            // Windows rejects rename over an existing file. Drop the published
            // page and retry once so a repeat put of the same body replaces it.
            if let Err(remove_err) = tokio::fs::remove_file(path).await {
                let _ = tokio::fs::remove_file(&tmp).await;
                return Err(StoreError::io(path, remove_err));
            }
            if let Err(rename_err) = tokio::fs::rename(&tmp, path).await {
                let _ = tokio::fs::remove_file(&tmp).await;
                return Err(StoreError::io(path, rename_err));
            }
            Ok(())
        }
        Err(err) => {
            let _ = tokio::fs::remove_file(&tmp).await;
            Err(StoreError::io(path, err))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    const DOCS_BODY: &str = r#"{"url":"https://example.com/docs","title":"Docs","html":"<h1>Docs</h1><p>Hello</p>","text":"Docs\nHello","captured_at":1700000000000}"#;
    const DOCS_HASH: &str = "b4f6dc1d0429c98c3f6cf49debda1bfb24e607503b35aec858b034d2e4afb7b2";

    fn docs_page() -> CapturedPage {
        CapturedPage {
            url: Url::parse("https://example.com/docs").expect("url"),
            title: "Docs".to_string(),
            html: "<h1>Docs</h1><p>Hello</p>".to_string(),
            text: "Docs\nHello".to_string(),
            content_hash: "pending".to_string(),
            captured_at: 1_700_000_000_000,
        }
    }

    #[test]
    fn hex_encode_matches_sha256_of_empty() {
        assert_eq!(
            hex_encode(Sha256::digest(b"").as_slice()),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn content_hash_is_sha256_of_compact_body_json() {
        let page = docs_page();
        assert_eq!(
            std::str::from_utf8(&canonical_body(&page)).expect("utf-8"),
            DOCS_BODY
        );
        assert_eq!(
            hex_encode(Sha256::digest(DOCS_BODY.as_bytes()).as_slice()),
            DOCS_HASH
        );
        assert_eq!(content_hash(&page), DOCS_HASH);
    }

    #[test]
    fn content_hash_tracks_body_fields_only() {
        let page = docs_page();
        let base = content_hash(&page);

        let mut ignored = page.clone();
        ignored.content_hash = "a".repeat(64);
        assert_eq!(content_hash(&ignored), base);

        let mut changed = page.clone();
        changed.title.push('!');
        assert_ne!(content_hash(&changed), base);

        changed = page.clone();
        changed.html.push(' ');
        assert_ne!(content_hash(&changed), base);

        changed = page.clone();
        changed.text.push(' ');
        assert_ne!(content_hash(&changed), base);

        changed = page.clone();
        changed.url = Url::parse("https://example.com/other").expect("url");
        assert_ne!(content_hash(&changed), base);

        changed = page.clone();
        changed.captured_at += 1;
        assert_ne!(content_hash(&changed), base);
    }

    #[test]
    fn page_store_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PageStore>();
    }

    #[tokio::test]
    async fn put_writes_pretty_json_with_trailing_newline() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = PageStore::open(dir.path()).expect("open");
        let stored = store.put(docs_page()).await.expect("put");
        let path = dir
            .path()
            .join("pages")
            .join(format!("{}.json", stored.content_hash));

        let mut expected = serde_json::to_vec_pretty(&stored).expect("pretty json");
        expected.push(b'\n');
        assert_eq!(std::fs::read(path).expect("read page file"), expected);
    }
}
