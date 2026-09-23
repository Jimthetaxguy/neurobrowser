use neuro_memory::store::{PageStore, StoreError};
use neuro_memory::CapturedPage;
use std::path::Path;
use url::Url;

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

fn entry_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read dir")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn put_get_round_trips_the_json_file_named_by_content_hash() {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = PageStore::open(dir.path()).expect("open");
    assert_eq!(store.pages_dir(), dir.path().join("pages"));

    let stored = store.put(docs_page()).await.expect("put");
    assert_eq!(stored.content_hash, DOCS_HASH);

    let path = dir.path().join("pages").join(format!("{DOCS_HASH}.json"));
    assert!(path.is_file());
    let on_disk = std::fs::read_to_string(&path).expect("read page");
    assert!(on_disk.contains(DOCS_HASH));
    assert!(on_disk.ends_with('\n'));

    assert_eq!(
        store.get(DOCS_HASH).await.expect("get"),
        Some(stored.clone())
    );
    assert_eq!(stored.url.as_str(), "https://example.com/docs");
    assert_eq!(stored.title, "Docs");
    assert_eq!(stored.html, "<h1>Docs</h1><p>Hello</p>");
    assert_eq!(stored.text, "Docs\nHello");
    assert_eq!(stored.captured_at, 1_700_000_000_000);
}

#[tokio::test]
async fn put_keys_by_body_hash_and_repeat_put_keeps_one_file() {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = PageStore::open(dir.path()).expect("open");
    let mut page = docs_page();
    page.content_hash = "../outside".to_string();

    let stored = store.put(page).await.expect("put");
    assert_eq!(stored.content_hash, DOCS_HASH);
    assert!(!dir.path().join("outside.json").exists());
    store.put(stored.clone()).await.expect("put again");

    assert_eq!(
        entry_names(&dir.path().join("pages")),
        vec![format!("{DOCS_HASH}.json")]
    );
    assert_eq!(store.get(DOCS_HASH).await.expect("get"), Some(stored));
}

#[tokio::test]
async fn put_get_round_trips_empty_and_quoted_fields() {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = PageStore::open(dir.path()).expect("open");
    let page = CapturedPage {
        url: Url::parse("https://example.com/search?q=a%20b").expect("url"),
        title: "Say \"hi\"".to_string(),
        html: "<p>a\\b</p>".to_string(),
        text: "line1\nline2".to_string(),
        content_hash: String::new(),
        captured_at: 0,
    };
    let empty = CapturedPage {
        url: Url::parse("https://example.com/empty").expect("url"),
        title: String::new(),
        html: String::new(),
        text: String::new(),
        content_hash: String::new(),
        captured_at: 0,
    };

    let stored = store.put(page).await.expect("put quoted");
    let stored_empty = store.put(empty).await.expect("put empty");
    assert_ne!(stored.content_hash, stored_empty.content_hash);
    assert_ne!(stored.content_hash, DOCS_HASH);
    assert_eq!(stored.url.as_str(), "https://example.com/search?q=a%20b");
    assert_eq!(
        store.get(&stored.content_hash).await.expect("get quoted"),
        Some(stored)
    );
    assert_eq!(
        store
            .get(&stored_empty.content_hash)
            .await
            .expect("get empty"),
        Some(stored_empty)
    );
}

#[tokio::test]
async fn delete_removes_one_page_and_missing_keys_succeed() {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = PageStore::open(dir.path()).expect("open");
    let docs = store.put(docs_page()).await.expect("put docs");
    let mut other = docs_page();
    other.captured_at += 1;
    let other = store.put(other).await.expect("put other");
    assert_ne!(docs.content_hash, other.content_hash);

    store.delete(&docs.content_hash).await.expect("delete docs");
    assert_eq!(store.get(&docs.content_hash).await.expect("get docs"), None);
    assert_eq!(
        store.get(&other.content_hash).await.expect("get other"),
        Some(other.clone())
    );
    assert_eq!(
        entry_names(&dir.path().join("pages")),
        vec![format!("{}.json", other.content_hash)]
    );

    store
        .delete(&docs.content_hash)
        .await
        .expect("delete missing");
    let absent = "ab".repeat(32);
    assert_eq!(store.get(&absent).await.expect("get absent"), None);
    store.delete(&absent).await.expect("delete absent");
    assert_eq!(
        store.get(&other.content_hash).await.expect("other remains"),
        Some(other)
    );
}

#[tokio::test]
async fn malformed_content_hashes_are_rejected() {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = PageStore::open(dir.path()).expect("open");
    let pages = dir.path().join("pages");
    let before = entry_names(&pages);

    let cases = [
        String::new(),
        "abc".to_string(),
        "a".repeat(63),
        "a".repeat(65),
        "A".repeat(64),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".replace('e', "E"),
        "../secret".to_string(),
        format!("{}/x", "a".repeat(64)),
        format!("../{}", "ab"),
        format!("{DOCS_HASH}\n"),
        format!(".{DOCS_HASH}"),
    ];

    for hash in cases {
        assert_eq!(
            store.get(&hash).await.expect_err("get"),
            StoreError::InvalidContentHash,
            "get {hash:?}"
        );
        assert_eq!(
            store.delete(&hash).await.expect_err("delete"),
            StoreError::InvalidContentHash,
            "delete {hash:?}"
        );
    }

    assert_eq!(entry_names(&pages), before);
}

#[tokio::test]
async fn corrupt_json_and_tampered_body_are_errors() {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = PageStore::open(dir.path()).expect("open");
    let stored = store.put(docs_page()).await.expect("put");
    let path = dir
        .path()
        .join("pages")
        .join(format!("{}.json", stored.content_hash));

    let original = std::fs::read_to_string(&path).expect("read");
    let tampered = original.replace("Docs", "Docs!");
    assert_ne!(tampered, original);
    std::fs::write(&path, tampered).expect("tamper");
    assert_eq!(
        store
            .get(&stored.content_hash)
            .await
            .expect_err("tampered get"),
        StoreError::HashMismatch {
            content_hash: stored.content_hash.clone(),
        }
    );

    let garbage = "a".repeat(64);
    std::fs::write(
        dir.path().join("pages").join(format!("{garbage}.json")),
        b"not-json",
    )
    .expect("write garbage");
    assert!(matches!(
        store.get(&garbage).await.expect_err("garbage get"),
        StoreError::Json { .. }
    ));

    let empty_object = "b".repeat(64);
    std::fs::write(
        dir.path()
            .join("pages")
            .join(format!("{empty_object}.json")),
        b"{}",
    )
    .expect("write empty object");
    assert!(matches!(
        store.get(&empty_object).await.expect_err("empty object"),
        StoreError::Json { .. }
    ));
}

#[test]
fn open_creates_pages_dir_and_rejects_a_file_path() {
    let dir = tempfile::tempdir().expect("temp dir");
    let data = dir.path().join("memory");
    assert!(!data.exists());

    let store = PageStore::open(&data).expect("open");
    assert!(data.join("pages").is_dir());
    assert!(entry_names(store.pages_dir()).is_empty());
    let again = PageStore::open(&data).expect("open again");
    assert_eq!(again.pages_dir(), store.pages_dir());

    let file = dir.path().join("not-a-dir");
    std::fs::write(&file, b"x").expect("write file");
    assert!(matches!(PageStore::open(&file), Err(StoreError::Io { .. })));

    std::fs::write(data.join("pages-blocked"), b"x").expect("write sibling");
    let blocked = data.join("blocked");
    std::fs::create_dir_all(&blocked).expect("blocked parent");
    std::fs::write(blocked.join("pages"), b"nope").expect("pages file");
    assert!(matches!(
        PageStore::open(&blocked),
        Err(StoreError::Io { .. })
    ));
}

#[tokio::test]
async fn concurrent_puts_keep_both_pages() {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = PageStore::open(dir.path()).expect("open");
    let first = docs_page();
    let mut second = docs_page();
    second.url = Url::parse("https://example.com/other").expect("url");
    second.title = "Other".to_string();

    let (first, second) = tokio::join!(store.put(first), store.put(second));
    let first = first.expect("put first");
    let second = second.expect("put second");
    assert_ne!(first.content_hash, second.content_hash);

    assert_eq!(
        store.get(&first.content_hash).await.expect("get first"),
        Some(first)
    );
    assert_eq!(
        store.get(&second.content_hash).await.expect("get second"),
        Some(second)
    );
    assert_eq!(entry_names(&dir.path().join("pages")).len(), 2);
}
