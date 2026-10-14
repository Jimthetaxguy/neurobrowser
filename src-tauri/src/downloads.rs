//! WebKit downloads for the desktop shell.
//!
//! The page webview asks before a download starts. Netguard runs first. An
//! allowed file is reserved under the user's Downloads folder with a name that
//! cannot leave that folder. The main webview hears progress, done, and failed.
//! Opening a file uses the id of a finished download, not a path from the page.

use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::webview::DownloadEvent;
use tauri::{Emitter, EventTarget, Manager, Runtime, Webview};

use crate::runtime::MAIN_WEBVIEW_LABEL;

pub const DOWNLOAD_EVENT: &str = "download-update";

const FILENAME_LIMIT: usize = 180;
const PROGRESS_TICKS: u32 = 1_500;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadPhase {
    Progress,
    Done,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadUpdate {
    pub id: String,
    pub phase: DownloadPhase,
    pub url: String,
    pub filename: String,
    pub received: u64,
    pub total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

struct Entry {
    id: String,
    url: String,
    filename: String,
    path: PathBuf,
    root: PathBuf,
    phase: DownloadPhase,
    received: u64,
    total: Option<u64>,
    error: Option<String>,
}

impl Entry {
    fn update(&self) -> DownloadUpdate {
        DownloadUpdate {
            id: self.id.clone(),
            phase: self.phase,
            url: self.url.clone(),
            filename: self.filename.clone(),
            received: self.received,
            total: self.total,
            error: self.error.clone(),
        }
    }
}

struct BookInner {
    next_id: u64,
    by_id: HashMap<String, Entry>,
    inflight: HashMap<String, VecDeque<String>>,
}

pub struct DownloadBook {
    inner: Mutex<BookInner>,
}

impl Default for DownloadBook {
    fn default() -> Self {
        Self {
            inner: Mutex::new(BookInner {
                next_id: 1,
                by_id: HashMap::new(),
                inflight: HashMap::new(),
            }),
        }
    }
}

pub struct DownloadStart {
    pub update: DownloadUpdate,
    /// Set when WebKit may write this absolute path. Absent when the download
    /// was refused before it started.
    pub path: Option<PathBuf>,
}

impl DownloadBook {
    /// Decide a download before any bytes are written.
    ///
    /// Netguard runs before the book lock, so a blocked address never reserves
    /// a filename. `suggested` is a display name from WebKit, not a path.
    pub fn begin(&self, root: &Path, url: &str, suggested: &str) -> DownloadStart {
        if let Some(reason) = neurobrowser::netguard::blocked_reason(url) {
            return self.record_failure(root, url, suggested, reason.to_string());
        }
        if !root.is_absolute() {
            return self.record_failure(
                root,
                url,
                suggested,
                "Downloads folder is unavailable".into(),
            );
        }
        if let Err(error) = std::fs::create_dir_all(root) {
            return self.record_failure(
                root,
                url,
                suggested,
                format!("Downloads folder is unavailable: {error}"),
            );
        }
        let mut inner = self.lock();
        let reserved = reserved_paths(&inner);
        let path = allocate_path(root, suggested, &reserved);
        let filename = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "download".into());
        let id = next_id(&mut inner);
        inner
            .inflight
            .entry(url.to_string())
            .or_default()
            .push_back(id.clone());
        let entry = Entry {
            id,
            url: url.to_string(),
            filename,
            path: path.clone(),
            root: root.to_path_buf(),
            phase: DownloadPhase::Progress,
            received: 0,
            total: None,
            error: None,
        };
        let update = entry.update();
        inner.by_id.insert(entry.id.clone(), entry);
        DownloadStart {
            update,
            path: Some(path),
        }
    }

    /// WebKit finished one download of `url`. The oldest in-flight row for that
    /// URL is the one that started first. macOS does not report the saved path,
    /// so the path reserved at the start is the one that counts.
    pub fn finish(&self, url: &str, success: bool) -> Option<DownloadUpdate> {
        let mut inner = self.lock();
        let id = {
            let queue = inner.inflight.get_mut(url)?;
            let id = queue.pop_front()?;
            if queue.is_empty() {
                inner.inflight.remove(url);
            }
            id
        };
        let entry = inner.by_id.get_mut(&id)?;
        if success && entry.path.is_file() {
            let len = std::fs::metadata(&entry.path)
                .map(|meta| meta.len())
                .unwrap_or(entry.received);
            entry.phase = DownloadPhase::Done;
            entry.received = len;
            entry.total = Some(len);
            entry.error = None;
        } else if success {
            entry.phase = DownloadPhase::Failed;
            entry.error = Some("The download finished without a file".into());
        } else {
            entry.phase = DownloadPhase::Failed;
            entry.error = Some("The download did not finish".into());
        }
        Some(entry.update())
    }

    /// Byte count while the row is still in progress. A late tick after the
    /// download finishes returns nothing.
    pub fn note_received(&self, id: &str, received: u64) -> Option<DownloadUpdate> {
        let mut inner = self.lock();
        let entry = inner.by_id.get_mut(id)?;
        if entry.phase != DownloadPhase::Progress || entry.received == received {
            return None;
        }
        entry.received = received;
        Some(entry.update())
    }

    pub fn is_in_progress(&self, id: &str) -> bool {
        self.lock()
            .by_id
            .get(id)
            .is_some_and(|entry| entry.phase == DownloadPhase::Progress)
    }

    /// Absolute path of a finished download that still lives inside its
    /// Downloads folder. A symlink that leaves that folder is refused.
    pub fn path_for_action(&self, id: &str) -> Result<PathBuf, String> {
        let inner = self.lock();
        let entry = inner.by_id.get(id).ok_or("Unknown download")?;
        if entry.phase != DownloadPhase::Done {
            return Err("That download is not ready to open".into());
        }
        let root = entry
            .root
            .canonicalize()
            .map_err(|error| format!("Downloads folder is unavailable: {error}"))?;
        let path = entry
            .path
            .canonicalize()
            .map_err(|_| "The download is no longer in the Downloads folder".to_string())?;
        if !path.starts_with(&root) {
            return Err("The download is no longer in the Downloads folder".into());
        }
        Ok(path)
    }

    /// Record a refusal that did not reserve a file. Used when the Downloads
    /// folder itself cannot be resolved.
    pub fn refuse(&self, url: &str, suggested: &str, error: String) -> DownloadUpdate {
        self.record_failure(Path::new(""), url, suggested, error)
            .update
    }

    fn record_failure(
        &self,
        root: &Path,
        url: &str,
        suggested: &str,
        error: String,
    ) -> DownloadStart {
        let mut inner = self.lock();
        let id = next_id(&mut inner);
        let entry = Entry {
            id,
            url: url.to_string(),
            filename: safe_download_filename(suggested),
            path: root.join("download"),
            root: root.to_path_buf(),
            phase: DownloadPhase::Failed,
            received: 0,
            total: None,
            error: Some(error),
        };
        let update = entry.update();
        inner.by_id.insert(entry.id.clone(), entry);
        DownloadStart { update, path: None }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BookInner> {
        self.inner.lock().unwrap_or_else(|error| error.into_inner())
    }
}

fn next_id(inner: &mut BookInner) -> String {
    let id = format!("dl-{}", inner.next_id);
    inner.next_id += 1;
    id
}

fn reserved_paths(inner: &BookInner) -> HashSet<PathBuf> {
    inner
        .by_id
        .values()
        .filter(|entry| entry.phase == DownloadPhase::Progress)
        .map(|entry| entry.path.clone())
        .collect()
}

/// Last path segment, with separators and control characters removed.
///
/// Both `/` and `\` count as separators on every platform, so a Windows-style
/// name cannot leave the Downloads folder when the shell is running on Linux.
pub fn safe_download_filename(suggested: &str) -> String {
    let raw = suggested
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or("download")
        .to_string();
    let mut cleaned = String::new();
    for ch in raw.chars() {
        if ch.is_control() || ch == '/' || ch == '\\' {
            continue;
        }
        cleaned.push(ch);
    }
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() || cleaned.chars().all(|ch| ch == '.') {
        return "download".into();
    }
    truncate_filename(&cleaned, FILENAME_LIMIT)
}

fn truncate_filename(filename: &str, limit: usize) -> String {
    if filename.len() <= limit {
        return filename.to_string();
    }
    let (stem, ext) = split_extension(filename);
    let ext_part = if ext.is_empty() {
        String::new()
    } else {
        format!(".{ext}")
    };
    let room = limit.saturating_sub(ext_part.len()).max(1);
    let mut stem_bytes = stem.len().min(room);
    while !stem.is_char_boundary(stem_bytes) {
        stem_bytes -= 1;
    }
    format!("{}{ext_part}", &stem[..stem_bytes])
}

fn split_extension(filename: &str) -> (&str, &str) {
    match filename.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, ext),
        _ => (filename, ""),
    }
}

fn allocate_path(root: &Path, suggested: &str, reserved: &HashSet<PathBuf>) -> PathBuf {
    let safe = safe_download_filename(suggested);
    let first = root.join(&safe);
    if path_is_free(&first, reserved) {
        return first;
    }
    let (stem, ext) = split_extension(&safe);
    for n in 1..10_000u32 {
        let name = if ext.is_empty() {
            format!("{stem} ({n})")
        } else {
            format!("{stem} ({n}).{ext}")
        };
        let candidate = root.join(name);
        if path_is_free(&candidate, reserved) {
            return candidate;
        }
    }
    root.join(format!("{stem}-{}", std::process::id()))
}

fn path_is_free(path: &Path, reserved: &HashSet<PathBuf>) -> bool {
    !reserved.contains(path) && !path.exists()
}

pub fn suggested_download_name(url: &str, destination: &Path) -> String {
    if let Some(name) = destination.file_name().and_then(|item| item.to_str()) {
        let folder = name.eq_ignore_ascii_case("downloads");
        if !name.is_empty() && !folder {
            return name.to_string();
        }
    }
    name_from_url(url).unwrap_or_else(|| "download".into())
}

fn name_from_url(url: &str) -> Option<String> {
    let parsed = url.parse::<tauri::Url>().ok()?;
    let segment = parsed
        .path_segments()?
        .filter(|segment| !segment.is_empty())
        .next_back()?;
    let decoded = percent_decode_segment(segment);
    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}

fn percent_decode_segment(segment: &str) -> String {
    let bytes = segment.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let (Some(high), Some(low)) =
                (hex_nibble(bytes[index + 1]), hex_nibble(bytes[index + 2]))
            {
                decoded.push((high << 4) | low);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub fn open_file_command(platform: &str, path: &Path) -> (String, Vec<String>) {
    let text = path.display().to_string();
    match platform {
        "macos" => ("open".into(), vec![text]),
        "windows" => (
            "cmd".into(),
            vec!["/C".into(), "start".into(), String::new(), text],
        ),
        _ => ("xdg-open".into(), vec![text]),
    }
}

pub fn show_in_folder_command(platform: &str, path: &Path) -> (String, Vec<String>) {
    let text = path.display().to_string();
    match platform {
        "macos" => ("open".into(), vec!["-R".into(), text]),
        "windows" => ("explorer".into(), vec![format!("/select,{text}")]),
        _ => {
            let folder = path.parent().unwrap_or(path).display().to_string();
            ("xdg-open".into(), vec![folder])
        }
    }
}

fn host_platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

pub fn launch_program(program: &str, args: &[String]) -> Result<(), String> {
    let status = std::process::Command::new(program)
        .args(args)
        .status()
        .map_err(|error| format!("Could not launch {program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} exited with {status}"))
    }
}

pub fn open_download_file(book: &DownloadBook, id: &str) -> Result<(), String> {
    let path = book.path_for_action(id)?;
    let (program, args) = open_file_command(host_platform(), &path);
    launch_program(&program, &args)
}

pub fn show_download_in_folder(book: &DownloadBook, id: &str) -> Result<(), String> {
    let path = book.path_for_action(id)?;
    let (program, args) = show_in_folder_command(host_platform(), &path);
    launch_program(&program, &args)
}

fn downloads_directory<R: Runtime>(webview: &Webview<R>) -> Result<PathBuf, String> {
    webview
        .app_handle()
        .path()
        .download_dir()
        .map_err(|error| error.to_string())
}

fn emit_download<R: Runtime>(webview: &Webview<R>, update: &DownloadUpdate) {
    if let Err(error) = webview.emit_to(
        EventTarget::labeled(MAIN_WEBVIEW_LABEL),
        DOWNLOAD_EVENT,
        update,
    ) {
        tracing::warn!("failed to emit {DOWNLOAD_EVENT}: {error}");
    }
}

fn watch_progress<R: Runtime>(
    webview: Webview<R>,
    book: std::sync::Arc<DownloadBook>,
    id: String,
    path: PathBuf,
) {
    std::thread::spawn(move || {
        let mut last = 0u64;
        for _ in 0..PROGRESS_TICKS {
            std::thread::sleep(PROGRESS_INTERVAL);
            if !book.is_in_progress(&id) {
                return;
            }
            let received = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            if received == last {
                continue;
            }
            last = received;
            if let Some(update) = book.note_received(&id, received) {
                emit_download(&webview, &update);
            }
        }
    });
}

/// Handle one WebKit download event from a page webview.
///
/// Returning false on a request stops the download before it starts.
pub fn handle_download_event<R: Runtime>(webview: Webview<R>, event: DownloadEvent<'_>) -> bool {
    let Some(state) = webview.try_state::<crate::AppState>() else {
        return false;
    };
    let book = state.downloads.clone();
    match event {
        DownloadEvent::Requested { url, destination } => {
            let root = match downloads_directory(&webview) {
                Ok(root) => root,
                Err(error) => {
                    let update = book.refuse(url.as_str(), "download", error);
                    emit_download(&webview, &update);
                    return false;
                }
            };
            let suggested = suggested_download_name(url.as_str(), destination);
            let start = book.begin(&root, url.as_str(), &suggested);
            emit_download(&webview, &start.update);
            if let Some(path) = start.path {
                *destination = path.clone();
                watch_progress(webview.clone(), book, start.update.id, path);
                true
            } else {
                false
            }
        }
        DownloadEvent::Finished { url, success, .. } => {
            if let Some(update) = book.finish(url.as_str(), success) {
                emit_download(&webview, &update);
            }
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

    fn temp_root() -> PathBuf {
        let n = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("neurobrowser-downloads-{nanos}-{n}"));
        fs::create_dir_all(&path).expect("temp downloads");
        path
    }

    #[test]
    fn filenames_stay_inside_the_downloads_folder() {
        assert_eq!(safe_download_filename("../../etc/passwd"), "passwd");
        assert_eq!(safe_download_filename("/tmp/report.pdf"), "report.pdf");
        assert_eq!(safe_download_filename(".."), "download");
        assert_eq!(safe_download_filename("."), "download");
        assert_eq!(safe_download_filename(""), "download");
        assert_eq!(safe_download_filename("a/b\\c"), "c");
        assert_eq!(safe_download_filename("notes.txt"), "notes.txt");
        let weird = safe_download_filename("bad\u{0000}name\n.pdf");
        assert!(!weird.contains('\u{0}'));
        assert!(!weird.contains('\n'));
        assert!(weird.ends_with(".pdf"));
    }

    #[test]
    fn colliding_names_gain_a_numeric_suffix() {
        let root = temp_root();
        fs::write(root.join("report.pdf"), b"old").unwrap();
        let book = DownloadBook::default();
        let first = book.begin(&root, "https://93.184.216.34/report.pdf", "report.pdf");
        let second = book.begin(&root, "https://93.184.216.34/report.pdf", "report.pdf");
        assert_eq!(
            first.path.as_deref(),
            Some(root.join("report (1).pdf").as_path())
        );
        assert_eq!(
            second.path.as_deref(),
            Some(root.join("report (2).pdf").as_path())
        );
        assert!(first.path.unwrap().starts_with(&root));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn netguard_blocks_a_download_before_a_file_is_reserved() {
        let root = temp_root();
        let book = DownloadBook::default();
        let blocked = book.begin(&root, "http://127.0.0.1/secret.bin", "../outside.bin");
        assert!(blocked.path.is_none());
        assert_eq!(blocked.update.phase, DownloadPhase::Failed);
        assert_eq!(blocked.update.filename, "outside.bin");
        assert!(blocked.update.error.unwrap().contains("internal/loopback"));
        assert!(!root.join("outside.bin").exists());
        assert!(!root.join("secret.bin").exists());

        let script = book.begin(&root, "javascript:alert(1)", "x");
        assert!(script.path.is_none());
        assert_eq!(script.update.phase, DownloadPhase::Failed);

        let file = book.begin(&root, "file:///etc/passwd", "passwd");
        assert!(file.path.is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_public_download_reserves_a_unique_file_and_can_finish() {
        let root = temp_root();
        let book = DownloadBook::default();
        let url = "https://93.184.216.34/files/report.pdf";
        let start = book.begin(&root, url, "report.pdf");
        let path = start.path.expect("allowed");
        assert_eq!(start.update.phase, DownloadPhase::Progress);
        assert_eq!(start.update.id, "dl-1");
        assert_eq!(path, root.join("report.pdf"));
        assert!(book.note_received("dl-1", 40).unwrap().received == 40);
        assert!(book.path_for_action("dl-1").is_err());

        fs::write(&path, b"pdf-bytes").unwrap();
        let done = book.finish(url, true).unwrap();
        assert_eq!(done.phase, DownloadPhase::Done);
        assert_eq!(done.received, b"pdf-bytes".len() as u64);
        assert_eq!(done.total, Some(b"pdf-bytes".len() as u64));
        assert!(book.note_received("dl-1", 99).is_none());
        assert_eq!(
            book.path_for_action("dl-1").unwrap(),
            path.canonicalize().unwrap()
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_missing_file_and_a_symlink_outside_downloads_cannot_be_opened() {
        let root = temp_root();
        let book = DownloadBook::default();
        let url = "https://93.184.216.34/empty.bin";
        let start = book.begin(&root, url, "empty.bin");
        assert!(start.path.is_some());
        let finished = book.finish(url, true).unwrap();
        assert_eq!(finished.phase, DownloadPhase::Failed);
        assert!(book.path_for_action(&finished.id).is_err());

        let url = "https://93.184.216.34/link.bin";
        let start = book.begin(&root, url, "link.bin");
        let path = start.path.unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/passwd", &path).unwrap();
            let done = book.finish(url, true).unwrap();
            assert_eq!(done.phase, DownloadPhase::Done);
            assert!(book.path_for_action(&done.id).is_err());
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_same_url_finishes_in_start_order() {
        let root = temp_root();
        let book = DownloadBook::default();
        let url = "https://93.184.216.34/twice.txt";
        let first = book.begin(&root, url, "twice.txt").path.unwrap();
        let second = book.begin(&root, url, "twice.txt").path.unwrap();
        fs::write(&second, b"two").unwrap();
        let failed = book.finish(url, false).unwrap();
        assert_eq!(failed.filename, "twice.txt");
        assert_eq!(failed.phase, DownloadPhase::Failed);
        fs::write(&first, b"one").unwrap();
        let done = book.finish(url, true).unwrap();
        assert_eq!(done.filename, "twice (1).txt");
        assert_eq!(done.phase, DownloadPhase::Done);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn open_and_show_commands_do_not_use_a_shell() {
        let path = Path::new("/Users/me/Downloads/report.pdf");
        assert_eq!(
            open_file_command("macos", path),
            ("open".into(), vec!["/Users/me/Downloads/report.pdf".into()])
        );
        assert_eq!(
            show_in_folder_command("macos", path),
            (
                "open".into(),
                vec!["-R".into(), "/Users/me/Downloads/report.pdf".into()]
            )
        );
        assert_eq!(
            open_file_command("linux", path),
            (
                "xdg-open".into(),
                vec!["/Users/me/Downloads/report.pdf".into()]
            )
        );
        assert_eq!(
            show_in_folder_command("linux", path),
            ("xdg-open".into(), vec!["/Users/me/Downloads".into()])
        );
    }

    #[test]
    fn a_suggested_folder_falls_back_to_the_url_name() {
        let name = suggested_download_name(
            "https://cdn.example/files/Picture%20One.png?x=1",
            Path::new("/home/me/Downloads"),
        );
        assert_eq!(name, "Picture One.png");
    }
}
