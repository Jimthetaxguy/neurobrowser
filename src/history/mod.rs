//! Browsing history for a later desktop slice.
//!
//! This is not page memory. `neuro-memory` stores captured pages; this module
//! stores visits. The root crate already depends on `serde_json` and does not
//! depend on SQLite, so a history store is one JSON file, `{dir}/history.json`,
//! plus an in-memory index keyed by URL.
//!
//! [`HistoryStore::open`] takes the directory. Callers choose where it lives.
//!
//! There is one entry per canonical URL. A repeat visit within
//! [`VISIT_COLLAPSE_WINDOW`] of that entry's `last_visited` does not increase
//! `visit_count`. A later visit does. The title on the entry is the title from
//! the newest timestamp.
//!
//! Private windows are a later slice. This store has no private-window flag.
//! [`should_record_visit`] is where a caller skips recording.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use thiserror::Error;

/// File name inside the directory passed to [`HistoryStore::open`].
pub const HISTORY_FILE_NAME: &str = "history.json";

/// Repeats this close to `last_visited` stay one visit.
///
/// Long enough that a reload, or a title that arrives just after navigation,
/// does not count twice. A return after this gap counts as another visit.
pub const VISIT_COLLAPSE_WINDOW: Duration = Duration::from_secs(30);

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Failure from [`HistoryStore`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HistoryError {
    /// A filesystem call failed. `message` includes the path and the OS error.
    #[error("history io: {message}")]
    Io { message: String },

    /// JSON encoding or decoding failed.
    #[error("history json: {message}")]
    Json { message: String },

    /// The file lists the same URL twice.
    #[error("history file lists {url} more than once")]
    DuplicateUrl { url: String },

    /// `record_visit` or `clear_since` was given a time before the Unix epoch.
    #[error("history time is before the unix epoch")]
    TimeBeforeEpoch,

    /// The timestamp cannot be stored as Unix milliseconds.
    #[error("history time is outside the supported range")]
    TimeOutOfRange,
}

impl HistoryError {
    fn io(path: &Path, error: std::io::Error) -> Self {
        Self::Io {
            message: format!("{}: {error}", path.display()),
        }
    }

    fn json(path: &Path, error: impl std::fmt::Display) -> Self {
        Self::Json {
            message: format!("{}: {error}", path.display()),
        }
    }
}

/// One URL in the history file.
///
/// `last_visited` is written as Unix epoch milliseconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryEntry {
    /// Canonical `http` or `https` URL. Username and password are removed.
    pub url: String,
    /// Title from the newest visit recorded for [`HistoryEntry::url`].
    pub title: String,
    /// Visits that were not collapsed into an earlier one.
    pub visit_count: u64,
    /// Newest visit. On disk, Unix epoch milliseconds.
    #[serde(with = "millis")]
    pub last_visited: SystemTime,
}

/// Whether the caller should persist this visit.
///
/// `private_window` is the later slice's flag. When it is `true`, this returns
/// `false`: do not call [`HistoryStore::record_visit`]. The store itself has
/// no private-window mode.
///
/// `about:blank` and any non-`http` / `https` URL also return `false`.
#[must_use]
pub fn should_record_visit(url: &str, private_window: bool) -> bool {
    !private_window && canonical_http_url(url).is_some()
}

/// JSON history file plus an in-memory index of one entry per URL.
#[derive(Debug)]
pub struct HistoryStore {
    path: PathBuf,
    index: Mutex<Index>,
}

#[derive(Debug, Default, Clone)]
struct Index {
    by_url: HashMap<String, HistoryEntry>,
}

impl HistoryStore {
    /// Create `dir` if it is missing and load `{dir}/history.json`.
    ///
    /// A missing file is an empty history. A present file is loaded as written.
    /// This does not create the file until a later write.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, HistoryError> {
        let dir = dir.as_ref();
        fs::create_dir_all(dir).map_err(|error| HistoryError::io(dir, error))?;
        let path = dir.join(HISTORY_FILE_NAME);
        let index = load_history(&path)?;
        Ok(Self {
            path,
            index: Mutex::new(index),
        })
    }

    /// Record a visit at `at`.
    ///
    /// Returns `Ok(None)` for `about:blank` and any URL whose scheme is not
    /// `http` or `https`, without touching the file. `Err` leaves the previous
    /// file and index in place.
    ///
    /// The same URL within [`VISIT_COLLAPSE_WINDOW`] of the stored
    /// `last_visited` stays one entry: `visit_count` is unchanged and
    /// `last_visited` moves forward when `at` is newer. A visit outside that
    /// window increments `visit_count`. The stored title is the title from the
    /// newest timestamp, including an empty title.
    ///
    /// Times are stored as Unix milliseconds. A time before the Unix epoch is
    /// [`HistoryError::TimeBeforeEpoch`].
    ///
    /// Private windows do not belong here. Call [`should_record_visit`] and
    /// skip this method when it returns `false`.
    pub fn record_visit(
        &self,
        url: &str,
        title: &str,
        at: SystemTime,
    ) -> Result<Option<HistoryEntry>, HistoryError> {
        let Some(url) = canonical_http_url(url) else {
            return Ok(None);
        };
        let at = truncate_to_millis(at)?;
        let mut index = self.lock();
        let previous = index.by_url.get(&url).cloned();
        let entry = index.record(&url, title, at);
        if let Err(error) = write_history(&self.path, &index) {
            match previous {
                Some(previous) => {
                    index.by_url.insert(url, previous);
                }
                None => {
                    index.by_url.remove(&url);
                }
            }
            return Err(error);
        }
        Ok(Some(entry))
    }

    /// Newest entries first.
    ///
    /// Order is `last_visited` descending, then `visit_count` descending, then
    /// URL ascending. A `limit` of zero returns an empty list.
    pub fn recent(&self, limit: usize) -> Vec<HistoryEntry> {
        self.ranked(limit, None)
    }

    /// Case-insensitive substring match over URL and title.
    ///
    /// Ranking matches [`HistoryStore::recent`]. An empty query matches every
    /// entry. A `limit` of zero returns an empty list.
    pub fn search(&self, query: &str, limit: usize) -> Vec<HistoryEntry> {
        self.ranked(limit, Some(query))
    }

    /// Remove every entry and write an empty history file.
    pub fn clear(&self) -> Result<(), HistoryError> {
        let mut index = self.lock();
        let previous = std::mem::take(&mut index.by_url);
        if let Err(error) = write_history(&self.path, &index) {
            index.by_url = previous;
            return Err(error);
        }
        Ok(())
    }

    /// Remove entries whose `last_visited` is at or after `time`.
    ///
    /// A collapsed entry has one timestamp, the latest visit, so this drops the
    /// whole URL when that latest visit falls in range. A time that matches
    /// nothing does not rewrite the file.
    pub fn clear_since(&self, time: SystemTime) -> Result<(), HistoryError> {
        let time = truncate_to_millis(time)?;
        let mut index = self.lock();
        if index.by_url.values().all(|entry| entry.last_visited < time) {
            return Ok(());
        }
        let previous = index.by_url.clone();
        index.by_url.retain(|_, entry| entry.last_visited < time);
        if let Err(error) = write_history(&self.path, &index) {
            index.by_url = previous;
            return Err(error);
        }
        Ok(())
    }

    fn ranked(&self, limit: usize, query: Option<&str>) -> Vec<HistoryEntry> {
        if limit == 0 {
            return Vec::new();
        }
        let needle = query.map(str::to_lowercase);
        let index = self.lock();
        let mut matches: Vec<HistoryEntry> = index
            .by_url
            .values()
            .filter(|entry| match &needle {
                None => true,
                Some(needle) => {
                    contains_ignore_case(&entry.url, needle)
                        || contains_ignore_case(&entry.title, needle)
                }
            })
            .cloned()
            .collect();
        drop(index);
        matches.sort_by(by_recency);
        matches.truncate(limit);
        matches
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Index> {
        self.index.lock().expect("history store lock poisoned")
    }
}

impl Index {
    fn record(&mut self, url: &str, title: &str, at: SystemTime) -> HistoryEntry {
        if let Some(entry) = self.by_url.get_mut(url) {
            if within_collapse_window(entry.last_visited, at) {
                if at >= entry.last_visited {
                    entry.last_visited = at;
                    entry.title = title.to_string();
                }
            } else {
                entry.visit_count = entry.visit_count.saturating_add(1);
                if at > entry.last_visited {
                    entry.last_visited = at;
                    entry.title = title.to_string();
                }
            }
            return entry.clone();
        }

        let entry = HistoryEntry {
            url: url.to_string(),
            title: title.to_string(),
            visit_count: 1,
            last_visited: at,
        };
        self.by_url.insert(entry.url.clone(), entry.clone());
        entry
    }
}

fn by_recency(left: &HistoryEntry, right: &HistoryEntry) -> std::cmp::Ordering {
    right
        .last_visited
        .cmp(&left.last_visited)
        .then_with(|| right.visit_count.cmp(&left.visit_count))
        .then_with(|| left.url.cmp(&right.url))
}

fn within_collapse_window(left: SystemTime, right: SystemTime) -> bool {
    left.duration_since(right)
        .or_else(|_| right.duration_since(left))
        .is_ok_and(|gap| gap <= VISIT_COLLAPSE_WINDOW)
}

fn contains_ignore_case(haystack: &str, needle_lowercase: &str) -> bool {
    haystack.to_lowercase().contains(needle_lowercase)
}

/// Parsed `http` or `https` URL, without userinfo.
///
/// `about:blank` and every other scheme return `None`. `https://example.com`
/// and `https://example.com/` are the same URL. Username and password are
/// stripped so a login URL does not write the secret into the file.
fn canonical_http_url(raw: &str) -> Option<String> {
    let mut parsed = url::Url::parse(raw.trim()).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    parsed.set_username("").ok()?;
    parsed.set_password(None).ok()?;
    Some(parsed.into())
}

fn truncate_to_millis(time: SystemTime) -> Result<SystemTime, HistoryError> {
    from_millis(to_millis(time)?)
}

fn to_millis(time: SystemTime) -> Result<u64, HistoryError> {
    let duration = time
        .duration_since(UNIX_EPOCH)
        .map_err(|_| HistoryError::TimeBeforeEpoch)?;
    u64::try_from(duration.as_millis()).map_err(|_| HistoryError::TimeOutOfRange)
}

fn from_millis(millis: u64) -> Result<SystemTime, HistoryError> {
    UNIX_EPOCH
        .checked_add(Duration::from_millis(millis))
        .ok_or(HistoryError::TimeOutOfRange)
}

fn load_history(path: &Path) -> Result<Index, HistoryError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Index::default()),
        Err(error) => return Err(HistoryError::io(path, error)),
    };
    if bytes.is_empty() {
        return Err(HistoryError::json(path, "file is empty"));
    }
    let entries: Vec<HistoryEntry> =
        serde_json::from_slice(&bytes).map_err(|error| HistoryError::json(path, error))?;
    let mut index = Index::default();
    for entry in entries {
        if index.by_url.contains_key(&entry.url) {
            return Err(HistoryError::DuplicateUrl { url: entry.url });
        }
        index.by_url.insert(entry.url.clone(), entry);
    }
    Ok(index)
}

fn write_history(path: &Path, index: &Index) -> Result<(), HistoryError> {
    let mut entries: Vec<&HistoryEntry> = index.by_url.values().collect();
    entries.sort_by(|left, right| by_recency(left, right));
    let mut bytes =
        serde_json::to_vec_pretty(&entries).map_err(|error| HistoryError::json(path, error))?;
    bytes.push(b'\n');
    write_atomic(path, &bytes)
}

/// Write `bytes` by creating a temp file in the same directory and renaming it
/// over `path`. Readers see the previous file or the complete new one.
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), HistoryError> {
    let parent = path
        .parent()
        .expect("history.json is created inside the directory passed to open");
    let file_name = path.file_name().expect("history.json has a file name");
    let temporary = parent.join(format!(
        ".{}.{}.{}.tmp",
        file_name.to_string_lossy(),
        std::process::id(),
        TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));

    if let Err(error) = write_temp(&temporary, bytes) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }

    match fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            // Windows refuses to rename over a file that already exists.
            if let Err(remove_error) = fs::remove_file(path) {
                let _ = fs::remove_file(&temporary);
                return Err(HistoryError::io(path, remove_error));
            }
            if let Err(rename_error) = fs::rename(&temporary, path) {
                let _ = fs::remove_file(&temporary);
                return Err(HistoryError::io(path, rename_error));
            }
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(HistoryError::Io {
                message: format!(
                    "rename {} to {}: {error}",
                    temporary.display(),
                    path.display()
                ),
            })
        }
    }
}

fn write_temp(path: &Path, bytes: &[u8]) -> Result<(), HistoryError> {
    let mut file = File::create(path).map_err(|error| HistoryError::io(path, error))?;
    file.write_all(bytes)
        .map_err(|error| HistoryError::io(path, error))?;
    file.sync_all()
        .map_err(|error| HistoryError::io(path, error))?;
    Ok(())
}

mod millis {
    use super::{from_millis, to_millis};
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::SystemTime;

    pub fn serialize<S>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let millis = to_millis(*time).map_err(serde::ser::Error::custom)?;
        serializer.serialize_u64(millis)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SystemTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let millis = u64::deserialize(deserializer)?;
        from_millis(millis).map_err(serde::de::Error::custom)
    }
}
