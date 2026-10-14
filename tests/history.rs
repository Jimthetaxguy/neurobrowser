//! Browsing-history store. No IPC and no UI: callers open a directory.

use neurobrowser::history::{
    should_record_visit, HistoryEntry, HistoryError, HISTORY_FILE_NAME, VISIT_COLLAPSE_WINDOW,
};
use neurobrowser::HistoryStore;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn at_secs(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

fn open_store() -> (tempfile::TempDir, HistoryStore) {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = HistoryStore::open(dir.path()).expect("open");
    (dir, store)
}

fn history_path(dir: &Path) -> PathBuf {
    dir.join(HISTORY_FILE_NAME)
}

fn file_names(dir: &Path) -> Vec<String> {
    let mut names = fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[test]
fn store_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<HistoryStore>();
}

#[test]
fn open_creates_the_directory_and_does_not_create_a_file() {
    let parent = tempfile::tempdir().expect("temp dir");
    let dir = parent.path().join("profile").join("history");
    let store = HistoryStore::open(&dir).expect("open");
    assert!(dir.is_dir());
    assert!(store.recent(1).is_empty());
    assert!(!history_path(&dir).exists());
}

#[test]
fn records_http_and_https_and_reloads_from_disk() {
    let (dir, store) = open_store();
    let at = UNIX_EPOCH + Duration::from_millis(1_700_000_000_123) + Duration::from_micros(400);
    let recorded = store
        .record_visit("https://example.com/docs", "Docs", at)
        .expect("record")
        .expect("stored");
    let stored_at = UNIX_EPOCH + Duration::from_millis(1_700_000_000_123);
    assert_eq!(recorded.url, "https://example.com/docs");
    assert_eq!(recorded.title, "Docs");
    assert_eq!(recorded.visit_count, 1);
    assert_eq!(recorded.last_visited, stored_at);

    let http = store
        .record_visit("http://example.com/docs", "Plain", at)
        .expect("record http")
        .expect("stored http");
    assert_eq!(http.url, "http://example.com/docs");
    assert_eq!(store.recent(10).len(), 2);

    let bytes = fs::read(history_path(dir.path())).expect("read history");
    let on_disk: Vec<HistoryEntry> = serde_json::from_slice(&bytes).expect("parse history");
    assert_eq!(on_disk, store.recent(10));
    assert!(bytes.ends_with(b"\n"));

    drop(store);
    let reloaded = HistoryStore::open(dir.path()).expect("reopen");
    assert_eq!(reloaded.recent(10), on_disk);

    let second = HistoryStore::open(dir.path()).expect("second open");
    assert_eq!(second.recent(10), reloaded.recent(10));
}

#[test]
fn canonicalizes_url_identity_and_strips_userinfo() {
    let (dir, store) = open_store();
    let at = at_secs(1_700_000_000);

    let root = store
        .record_visit("  https://example.com  ", "Root", at)
        .expect("record")
        .expect("stored");
    assert_eq!(root.url, "https://example.com/");

    let again = store
        .record_visit(
            "https://example.com/",
            "Root again",
            at + Duration::from_secs(1),
        )
        .expect("record slash")
        .expect("stored slash");
    assert_eq!(again.visit_count, 1);
    assert_eq!(again.title, "Root again");
    assert_eq!(again.url, "https://example.com/");

    let docs = store
        .record_visit("HTTPS://Example.COM/Docs", "Docs", at)
        .expect("record docs")
        .expect("stored docs");
    assert_eq!(docs.url, "https://example.com/Docs");

    store
        .record_visit("https://example.com/a", "A", at)
        .expect("record a");
    store
        .record_visit("https://example.com/a/", "A slash", at)
        .expect("record a slash");
    let urls: Vec<_> = store
        .recent(10)
        .into_iter()
        .map(|entry| entry.url)
        .collect();
    assert!(urls.contains(&"https://example.com/a".to_string()));
    assert!(urls.contains(&"https://example.com/a/".to_string()));

    store
        .record_visit("https://example.com/#one", "Fragment", at)
        .expect("record fragment");
    assert_eq!(
        store
            .recent(10)
            .iter()
            .filter(|entry| entry.url == "https://example.com/#one")
            .count(),
        1
    );

    let login = store
        .record_visit("https://user:secret@example.com/private-path", "Login", at)
        .expect("record login")
        .expect("stored login");
    assert_eq!(login.url, "https://example.com/private-path");
    let bytes = fs::read(history_path(dir.path())).expect("read history");
    let text = String::from_utf8(bytes).expect("utf-8");
    assert!(!text.contains("secret"));
    assert!(!text.contains("user:"));
}

#[test]
fn collapses_repeat_visits_inside_the_window() {
    let (_dir, store) = open_store();
    let start = at_secs(1_700_000_000);
    let url = "https://example.com/page";

    let first = store
        .record_visit(url, "Kept", start)
        .expect("first")
        .expect("stored");
    assert_eq!(first.visit_count, 1);

    let emptied = store
        .record_visit(url, "", start + Duration::from_secs(1))
        .expect("empty title")
        .expect("stored");
    assert_eq!(emptied.visit_count, 1);
    assert_eq!(emptied.title, "");
    assert_eq!(emptied.last_visited, start + Duration::from_secs(1));

    let on_edge = start + Duration::from_secs(1) + VISIT_COLLAPSE_WINDOW;
    let edge = store
        .record_visit(url, "Edge", on_edge)
        .expect("edge")
        .expect("stored");
    assert_eq!(edge.visit_count, 1);
    assert_eq!(edge.title, "Edge");
    assert_eq!(edge.last_visited, on_edge);

    // The window slides with last_visited, so another exact gap still collapses.
    let slid = on_edge + VISIT_COLLAPSE_WINDOW;
    let still_one = store
        .record_visit(url, "Slid", slid)
        .expect("slid")
        .expect("stored");
    assert_eq!(still_one.visit_count, 1);
    assert_eq!(still_one.last_visited, slid);
    assert_eq!(store.recent(10).len(), 1);

    let past = slid + VISIT_COLLAPSE_WINDOW + Duration::from_millis(1);
    let counted = store
        .record_visit(url, "Counted", past)
        .expect("past")
        .expect("stored");
    assert_eq!(counted.visit_count, 2);
    assert_eq!(counted.title, "Counted");
    assert_eq!(counted.last_visited, past);
    assert_eq!(store.recent(10).len(), 1);
}

#[test]
fn older_visit_does_not_move_last_visited_backward() {
    let (_dir, store) = open_store();
    let start = at_secs(1_800_000_000);
    let url = "https://example.com/later";

    let newer_at = start + Duration::from_secs(10);
    store.record_visit(url, "New", newer_at).expect("newer");
    let inside = store
        .record_visit(url, "Old", start)
        .expect("older inside")
        .expect("stored");
    assert_eq!(inside.visit_count, 1);
    assert_eq!(inside.title, "New");
    assert_eq!(inside.last_visited, newer_at);

    let newest_at = newer_at + VISIT_COLLAPSE_WINDOW + Duration::from_millis(1);
    let newest = store
        .record_visit(url, "Newest", newest_at)
        .expect("newest")
        .expect("stored");
    assert_eq!(newest.visit_count, 2);
    assert_eq!(newest.title, "Newest");
    assert_eq!(newest.last_visited, newest_at);

    let late = store
        .record_visit(url, "Ancient", start)
        .expect("ancient")
        .expect("stored");
    assert_eq!(late.visit_count, 3);
    assert_eq!(late.title, "Newest");
    assert_eq!(late.last_visited, newest_at);
}

#[test]
fn skips_about_blank_and_non_http_urls() {
    let (dir, store) = open_store();
    let at = at_secs(1_700_000_000);
    let skipped = [
        "about:blank",
        "ABOUT:BLANK",
        "about:blank#ignored",
        "about:srcdoc",
        "javascript:alert(1)",
        "JavaScript:alert(1)",
        "file:///tmp/secret",
        "data:text/html,hi",
        "blob:https://example.com/uuid",
        "mailto:person@example.com",
        "ftp://example.com/file",
        "example.com",
        "",
        "   ",
        "http://",
        "not a url",
    ];
    for url in skipped {
        assert!(
            !should_record_visit(url, false),
            "{url} should not be recordable"
        );
        assert!(store
            .record_visit(url, "Ignored", at)
            .expect("skip")
            .is_none());
    }
    assert!(store.recent(10).is_empty());
    assert!(!history_path(dir.path()).exists());

    store
        .record_visit("https://example.com/kept", "Kept", at)
        .expect("record");
    assert!(store
        .record_visit("about:blank", "Nope", at)
        .expect("skip after record")
        .is_none());
    let recent = store.recent(10);
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].url, "https://example.com/kept");
    assert_eq!(recent[0].visit_count, 1);
}

#[test]
fn private_window_is_a_caller_side_skip() {
    let (_dir, store) = open_store();
    let at = at_secs(1_700_000_000);
    let private_url = "https://example.com/private";
    let normal_url = "https://example.com/normal";

    assert!(!should_record_visit(private_url, true));
    assert!(should_record_visit(normal_url, false));
    assert!(!should_record_visit("about:blank", false));
    assert!(!should_record_visit("about:blank", true));

    // The store has no private-window argument. Callers skip the write here.
    if should_record_visit(private_url, true) {
        store
            .record_visit(private_url, "Private", at)
            .expect("record");
    }
    assert!(store.recent(10).is_empty());

    if should_record_visit(normal_url, false) {
        store
            .record_visit(normal_url, "Normal", at)
            .expect("record");
    }
    assert_eq!(store.recent(10).len(), 1);
    assert_eq!(store.recent(10)[0].url, "https://example.com/normal");
}

#[test]
fn recent_and_search_rank_by_last_visited_then_visit_count() {
    let (_dir, store) = open_store();
    let step = VISIT_COLLAPSE_WINDOW + Duration::from_millis(1);
    let latest = at_secs(1_700_000_100);
    let tied = latest - step;
    let older = tied - step;

    store
        .record_visit("https://example.com/rust-high", "Notes", tied - step)
        .expect("high earlier");
    store
        .record_visit("https://example.com/rust-high", "Notes", tied)
        .expect("high latest");
    store
        .record_visit("https://example.com/low", "Rust Book", tied)
        .expect("low");
    store
        .record_visit("https://example.com/rust-older", "Notes", older)
        .expect("older");
    store
        .record_visit("https://example.com/home", "Home", latest)
        .expect("newer");

    let recent = store.recent(10);
    let recent_urls: Vec<_> = recent.iter().map(|entry| entry.url.as_str()).collect();
    assert_eq!(
        recent_urls,
        [
            "https://example.com/home",
            "https://example.com/rust-high",
            "https://example.com/low",
            "https://example.com/rust-older",
        ]
    );
    assert_eq!(recent[1].visit_count, 2);
    assert_eq!(recent[2].visit_count, 1);
    assert_eq!(
        store
            .recent(2)
            .iter()
            .map(|entry| entry.url.as_str())
            .collect::<Vec<_>>(),
        ["https://example.com/home", "https://example.com/rust-high"]
    );
    assert_eq!(store.recent(0).len(), 0);
    assert_eq!(store.search("", 10), recent);

    let found = store.search("RuSt", 10);
    let found_urls: Vec<_> = found.iter().map(|entry| entry.url.as_str()).collect();
    assert_eq!(
        found_urls,
        [
            "https://example.com/rust-high",
            "https://example.com/low",
            "https://example.com/rust-older",
        ]
    );
    assert_eq!(
        store.search("rust", 1)[0].url,
        "https://example.com/rust-high"
    );
    assert_eq!(store.search("BOOK", 10)[0].url, "https://example.com/low");
    assert_eq!(store.search("notes", 10).len(), 2);
    assert_eq!(
        store.search("EXAMPLE.COM/HOME", 10)[0].url,
        "https://example.com/home"
    );
    assert!(store.search("missing", 10).is_empty());
    assert!(store.search("rust", 0).is_empty());
}

#[test]
fn get_reads_one_canonical_entry() {
    let (_dir, store) = open_store();
    let at = at_secs(1_700_000_000);
    assert!(store.get("https://example.com/missing").is_none());
    assert!(store.get("about:blank").is_none());
    assert!(store.get("ftp://example.com/file").is_none());

    store
        .record_visit("HTTPS://Example.COM/Docs", "Docs", at)
        .expect("record");
    let entry = store.get("https://example.com/Docs").expect("canonical");
    assert_eq!(entry.url, "https://example.com/Docs");
    assert_eq!(entry.title, "Docs");
    assert_eq!(
        store.get("  https://example.com/Docs  ").unwrap().title,
        "Docs"
    );
}

#[test]
fn clear_and_clear_since_drop_entries_by_last_visited() {
    let (dir, store) = open_store();
    let early = at_secs(1_000);
    let later = at_secs(2_000);
    store
        .record_visit("https://example.com/early", "Early", early)
        .expect("early");
    store
        .record_visit("https://example.com/later", "Later", later)
        .expect("later");

    let bytes = fs::read(history_path(dir.path())).expect("read");
    store
        .clear_since(later + Duration::from_secs(1))
        .expect("clear misses");
    assert_eq!(fs::read(history_path(dir.path())).expect("reread"), bytes);
    assert_eq!(store.recent(10).len(), 2);

    store.clear_since(later).expect("clear later");
    let left = store.recent(10);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].url, "https://example.com/early");

    drop(store);
    let store = HistoryStore::open(dir.path()).expect("reopen");
    assert_eq!(store.recent(10)[0].url, "https://example.com/early");

    store.clear_since(early).expect("clear early");
    assert!(store.recent(10).is_empty());

    store
        .record_visit("https://example.com/after", "After", later)
        .expect("after");
    store.clear().expect("clear");
    assert!(store.recent(10).is_empty());
    assert_eq!(
        fs::read(history_path(dir.path())).expect("empty file"),
        b"[]\n"
    );

    drop(store);
    let store = HistoryStore::open(dir.path()).expect("reopen empty");
    assert!(store.recent(10).is_empty());
    store.clear().expect("clear already empty");
    assert_eq!(
        fs::read(history_path(dir.path())).expect("still empty"),
        b"[]\n"
    );
}

#[test]
fn writes_leave_a_complete_file_and_no_temp() {
    let (dir, store) = open_store();
    let start = at_secs(1_700_000_000);
    for step in 0..20 {
        let url = if step % 2 == 0 {
            "https://example.com/one"
        } else {
            "https://example.com/two"
        };
        store
            .record_visit(
                url,
                &format!("Title {step}"),
                start + Duration::from_secs(step),
            )
            .expect("record");
        assert_eq!(file_names(dir.path()), vec!["history.json".to_string()]);
        let bytes = fs::read(history_path(dir.path())).expect("read");
        serde_json::from_slice::<Vec<HistoryEntry>>(&bytes).expect("complete json");
        assert!(bytes.ends_with(b"\n"));
    }

    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(history_path(dir.path())).expect("read")).expect("json");
    assert_eq!(value.as_array().expect("array").len(), 2);
    assert_eq!(
        value[0]["last_visited"],
        serde_json::json!(1_700_000_019_000_u64)
    );
}

#[test]
fn rejects_unreadable_history_files_without_deleting_them() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = history_path(dir.path());

    fs::write(&path, b"").expect("write empty");
    assert!(matches!(
        HistoryStore::open(dir.path()),
        Err(HistoryError::Json { .. })
    ));
    assert_eq!(fs::read(&path).expect("still empty"), b"");

    fs::write(&path, b"{").expect("write broken");
    assert!(matches!(
        HistoryStore::open(dir.path()),
        Err(HistoryError::Json { .. })
    ));
    assert_eq!(fs::read(&path).expect("still broken"), b"{");

    let extra = r#"[{"url":"https://example.com/","title":"A","visit_count":1,"last_visited":1,"extra":true}]"#;
    fs::write(&path, extra).expect("write extra");
    assert!(matches!(
        HistoryStore::open(dir.path()),
        Err(HistoryError::Json { .. })
    ));

    let duplicate = concat!(
        r#"[{"url":"https://example.com/","title":"A","visit_count":1,"last_visited":1},"#,
        r#"{"url":"https://example.com/","title":"B","visit_count":2,"last_visited":2}]"#
    );
    fs::write(&path, duplicate).expect("write duplicate");
    match HistoryStore::open(dir.path()) {
        Err(HistoryError::DuplicateUrl { url }) => assert_eq!(url, "https://example.com/"),
        other => panic!("expected duplicate url, got {other:?}"),
    }
    assert_eq!(
        fs::read(&path).expect("still duplicate"),
        duplicate.as_bytes()
    );
}

#[test]
fn time_before_the_epoch_does_not_change_history() {
    let (dir, store) = open_store();
    let at = at_secs(50);
    store
        .record_visit("https://example.com/", "Kept", at)
        .expect("record");
    let bytes = fs::read(history_path(dir.path())).expect("read");
    let before = UNIX_EPOCH
        .checked_sub(Duration::from_secs(1))
        .expect("before epoch");

    assert_eq!(
        store
            .record_visit("https://example.com/", "Nope", before)
            .expect_err("before epoch"),
        HistoryError::TimeBeforeEpoch
    );
    assert_eq!(
        store.clear_since(before).expect_err("clear before epoch"),
        HistoryError::TimeBeforeEpoch
    );
    assert_eq!(fs::read(history_path(dir.path())).expect("reread"), bytes);
    assert_eq!(store.recent(10)[0].title, "Kept");
    assert_eq!(store.recent(10)[0].visit_count, 1);
}

#[test]
fn visit_count_saturates_instead_of_wrapping() {
    let dir = tempfile::tempdir().expect("temp dir");
    let when = at_secs(1_700_000_000);
    let entry = HistoryEntry {
        url: "https://example.com/".to_string(),
        title: "Full".to_string(),
        visit_count: u64::MAX,
        last_visited: when,
    };
    fs::write(
        history_path(dir.path()),
        serde_json::to_vec_pretty(&[entry]).expect("encode"),
    )
    .expect("write");

    let store = HistoryStore::open(dir.path()).expect("open");
    let updated = store
        .record_visit(
            "https://example.com/",
            "Still",
            when + VISIT_COLLAPSE_WINDOW + Duration::from_millis(1),
        )
        .expect("record")
        .expect("stored");
    assert_eq!(updated.visit_count, u64::MAX);
    assert_eq!(updated.title, "Still");
}
