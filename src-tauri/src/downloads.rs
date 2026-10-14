//! WebKit downloads for the desktop shell.
//!
//! The page webview asks before a download starts. Netguard runs on the
//! requested address, then again on the address that remains after redirects.
//! A blocked final address cancels the download before the file is written and
//! again before a finished row can be kept. An allowed file is reserved under
//! the user's Downloads folder with a name that cannot leave that folder. On
//! macOS the saved file gets the quarantine attribute. The main webview hears
//! progress, done, and failed. Opening a file uses the id of a finished
//! download, not a path from the page.

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
    /// Address that remained after redirects, once netguard allowed it.
    final_url: Option<String>,
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
            final_url: None,
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
    ///
    /// The post-redirect URL stored by [`Self::authorize_final_url`] is judged
    /// again before a file can be kept.
    pub fn finish(&self, url: &str, success: bool) -> Option<DownloadUpdate> {
        self.finish_with_final_url(url, None, success)
    }

    /// Like [`Self::finish`], but `final_url` is the address after redirects
    /// when it was not already stored. A blocked address deletes the file.
    pub fn finish_with_final_url(
        &self,
        url: &str,
        final_url: Option<&str>,
        success: bool,
    ) -> Option<DownloadUpdate> {
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
        if entry.phase == DownloadPhase::Failed {
            return Some(entry.update());
        }
        if let Some(final_url) = final_url {
            entry.final_url = Some(final_url.to_string());
        }
        let judged = entry.final_url.clone().unwrap_or_else(|| entry.url.clone());
        if let Some(reason) = neurobrowser::netguard::blocked_reason(&judged) {
            return Some(fail_entry(entry, reason.to_string()));
        }
        if success && entry.path.is_file() {
            if is_regular_file(&entry.path) {
                if let Err(error) = apply_download_quarantine(&entry.path) {
                    return Some(fail_entry(entry, error));
                }
            }
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

    /// Re-run netguard on the post-redirect URL before WebKit may write.
    ///
    /// `Err` is the failed row. Any partial file is removed, and a later
    /// finish will not mark this download done.
    pub fn authorize_final_url(&self, id: &str, final_url: &str) -> Result<(), DownloadUpdate> {
        if let Some(reason) = neurobrowser::netguard::blocked_reason(final_url) {
            return Err(self.cancel(id, reason.to_string()));
        }
        let mut inner = self.lock();
        if let Some(entry) = inner.by_id.get_mut(id) {
            if entry.phase == DownloadPhase::Progress {
                entry.final_url = Some(final_url.to_string());
            }
        }
        Ok(())
    }

    /// Mark an in-progress download failed and delete any partial file.
    pub fn cancel(&self, id: &str, error: String) -> DownloadUpdate {
        let mut inner = self.lock();
        let Some(entry) = inner.by_id.get_mut(id) else {
            return DownloadUpdate {
                id: id.to_string(),
                phase: DownloadPhase::Failed,
                url: String::new(),
                filename: "download".into(),
                received: 0,
                total: None,
                error: Some(error),
            };
        };
        let url = entry.url.clone();
        let update = fail_entry(entry, error);
        dequeue(&mut inner, &url, id);
        update
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
            final_url: None,
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

fn fail_entry(entry: &mut Entry, error: String) -> DownloadUpdate {
    entry.phase = DownloadPhase::Failed;
    entry.error = Some(error);
    remove_saved_file(&entry.path);
    entry.update()
}

fn dequeue(inner: &mut BookInner, url: &str, id: &str) {
    let Some(queue) = inner.inflight.get_mut(url) else {
        return;
    };
    queue.retain(|item| item != id);
    if queue.is_empty() {
        inner.inflight.remove(url);
    }
}

fn remove_saved_file(path: &Path) {
    if std::fs::symlink_metadata(path).is_ok() {
        let _ = std::fs::remove_file(path);
    }
}

fn is_regular_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_file())
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

/// Open a downloaded file without a shell.
///
/// Windows passes the path as one argument to `explorer.exe`. `cmd /C start`
/// would treat `&` in a name such as `report&calc.exe` as a command separator.
pub fn open_file_command(platform: &str, path: &Path) -> (String, Vec<String>) {
    let text = path.display().to_string();
    match platform {
        "macos" => ("open".into(), vec![text]),
        "windows" => ("explorer.exe".into(), vec![text]),
        _ => ("xdg-open".into(), vec![text]),
    }
}

pub fn show_in_folder_command(platform: &str, path: &Path) -> (String, Vec<String>) {
    let text = path.display().to_string();
    match platform {
        "macos" => ("open".into(), vec!["-R".into(), text]),
        "windows" => ("explorer.exe".into(), vec![format!("/select,{text}")]),
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
    let mut command = std::process::Command::new(program);
    command.args(args);
    // Explorer returns before the file opens and often exits non-zero.
    if detaches_after_spawn(program) {
        return command
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Could not launch {program}: {error}"));
    }
    let status = command
        .status()
        .map_err(|error| format!("Could not launch {program}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} exited with {status}"))
    }
}

fn detaches_after_spawn(program: &str) -> bool {
    let name = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);
    name.eq_ignore_ascii_case("explorer.exe") || name.eq_ignore_ascii_case("explorer")
}

#[cfg(target_os = "macos")]
const QUARANTINE_ATTRIBUTE: &str = "com.apple.quarantine";

/// `0083` is the quarantine flag browsers set on a downloaded file.
pub fn quarantine_attribute_value(timestamp_secs: u64, agent: &str, id: &str) -> String {
    format!("0083;{timestamp_secs:x};{agent};{id}")
}

fn apply_download_quarantine(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let id = uuid::Uuid::new_v4().to_string();
        let value = quarantine_attribute_value(now, "NeuroBrowser", &id);
        set_quarantine_attribute(path, &value)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn set_quarantine_attribute(path: &Path, value: &str) -> Result<(), String> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "The download path is not valid".to_string())?;
    let name = CString::new(QUARANTINE_ATTRIBUTE).expect("quarantine attribute name");
    let rc = unsafe {
        setxattr(
            path.as_ptr(),
            name.as_ptr(),
            value.as_ptr() as *const std::ffi::c_void,
            value.len(),
            0,
            0,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(format!(
            "Could not mark the download for quarantine: {}",
            std::io::Error::last_os_error()
        ))
    }
}

#[cfg(target_os = "macos")]
extern "C" {
    fn setxattr(
        path: *const std::ffi::c_char,
        name: *const std::ffi::c_char,
        value: *const std::ffi::c_void,
        size: usize,
        position: u32,
        options: i32,
    ) -> i32;
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

fn final_url_for_download(url: &str) -> Result<String, String> {
    let url = url.to_string();
    let handle = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error.to_string())?;
        runtime.block_on(neurobrowser::netguard::final_url_after_redirects(&url))
    });
    match handle.join() {
        Ok(result) => result,
        Err(_) => Err("Download check stopped".into()),
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
            let Some(path) = start.path.clone() else {
                emit_download(&webview, &start.update);
                return false;
            };
            // WebKit has the response by now, but the event still carries the
            // original address. Follow redirects and judge the address that
            // remains before anything is written.
            let final_url = match final_url_for_download(url.as_str()) {
                Ok(final_url) => final_url,
                Err(error) => {
                    let update = book.cancel(&start.update.id, error);
                    emit_download(&webview, &update);
                    return false;
                }
            };
            if let Err(update) = book.authorize_final_url(&start.update.id, &final_url) {
                emit_download(&webview, &update);
                return false;
            }
            emit_download(&webview, &start.update);
            *destination = path.clone();
            watch_progress(webview.clone(), book, start.update.id, path);
            true
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

    #[cfg(target_os = "macos")]
    fn read_quarantine_attribute(path: &Path) -> Result<String, String> {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let path = CString::new(path.as_os_str().as_bytes())
            .map_err(|_| "The download path is not valid".to_string())?;
        let name = CString::new("com.apple.quarantine").expect("attribute name");
        let mut buf = vec![0u8; 512];
        let read = unsafe {
            getxattr(
                path.as_ptr(),
                name.as_ptr(),
                buf.as_mut_ptr() as *mut std::ffi::c_void,
                buf.len(),
                0,
                0,
            )
        };
        if read < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        buf.truncate(read as usize);
        String::from_utf8(buf).map_err(|error| error.to_string())
    }

    #[cfg(target_os = "macos")]
    extern "C" {
        fn getxattr(
            path: *const std::ffi::c_char,
            name: *const std::ffi::c_char,
            value: *mut std::ffi::c_void,
            size: usize,
            position: u32,
            options: i32,
        ) -> isize;
    }

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

        // `&` stays inside one argument. A shell would treat it as a separator
        // and launch calc.exe beside report.
        let windows = Path::new(r"C:\Users\me\Downloads\report&calc.exe");
        assert_eq!(
            safe_download_filename(r"C:\Users\me\Downloads\report&calc.exe"),
            "report&calc.exe"
        );
        let (open_program, open_args) = open_file_command("windows", windows);
        assert_eq!(
            (open_program.as_str(), open_args.as_slice()),
            (
                "explorer.exe",
                [r"C:\Users\me\Downloads\report&calc.exe".to_string()].as_slice()
            )
        );
        let (show_program, show_args) = show_in_folder_command("windows", windows);
        assert_eq!(
            (show_program.as_str(), show_args.as_slice()),
            (
                "explorer.exe",
                [r"/select,C:\Users\me\Downloads\report&calc.exe".to_string()].as_slice()
            )
        );
        for (program, args) in [
            open_file_command("macos", path),
            show_in_folder_command("macos", path),
            open_file_command("linux", path),
            show_in_folder_command("linux", path),
            (open_program, open_args),
            (show_program, show_args),
        ] {
            let name = program.to_ascii_lowercase();
            assert_ne!(name, "cmd");
            assert_ne!(name, "cmd.exe");
            assert!(
                args.iter().all(
                    |arg| !arg.eq_ignore_ascii_case("/C") && !arg.eq_ignore_ascii_case("start")
                ),
                "{program} {args:?}"
            );
        }
    }

    #[test]
    fn a_blocked_final_url_is_cancelled_before_write_and_before_finish() {
        let root = temp_root();
        let book = DownloadBook::default();
        let start_url = "https://93.184.216.34/start";
        let start = book.begin(&root, start_url, "secret.bin");
        let path = start.path.clone().unwrap();
        fs::write(&path, b"partial").unwrap();
        let failed = book
            .authorize_final_url(&start.update.id, "http://169.254.169.254/latest/meta-data/")
            .expect_err("metadata redirect must cancel");
        assert_eq!(failed.phase, DownloadPhase::Failed);
        let error = failed.error.expect("block reason");
        assert!(
            error.contains("169.254") || error.contains("internal"),
            "{error}"
        );
        assert!(!path.exists());
        assert!(book.finish(start_url, true).is_none());
        assert!(book.path_for_action(&start.update.id).is_err());

        let later = "https://93.184.216.34/later";
        let start = book.begin(&root, later, "later.bin");
        let path = start.path.unwrap();
        fs::write(&path, b"secret").unwrap();
        let failed = book
            .finish_with_final_url(later, Some("http://127.0.0.1/secret"), true)
            .unwrap();
        assert_eq!(failed.phase, DownloadPhase::Failed);
        assert!(failed.error.unwrap().contains("127.0.0.1"));
        assert!(!path.exists());
        assert!(book.path_for_action(&failed.id).is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn an_allowed_final_url_can_finish_and_is_quarantined_where_supported() {
        let root = temp_root();
        let book = DownloadBook::default();
        let url = "https://93.184.216.34/files/report.pdf";
        let start = book.begin(&root, url, "report.pdf");
        assert!(book.authorize_final_url(&start.update.id, url).is_ok());
        let path = start.path.unwrap();
        fs::write(&path, b"%PDF").unwrap();
        let done = book.finish(url, true).unwrap();
        assert_eq!(done.phase, DownloadPhase::Done);
        assert!(path.is_file());
        #[cfg(target_os = "macos")]
        {
            let value = read_quarantine_attribute(&path).expect("quarantine");
            assert!(value.starts_with("0083;"), "{value}");
            assert!(value.contains("NeuroBrowser"), "{value}");
        }
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn quarantine_attribute_value_marks_a_download() {
        assert_eq!(
            quarantine_attribute_value(0x5f3e2a1b, "NeuroBrowser", "ABC"),
            "0083;5f3e2a1b;NeuroBrowser;ABC"
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
