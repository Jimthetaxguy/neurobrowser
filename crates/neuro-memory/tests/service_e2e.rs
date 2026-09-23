use neuro_memory::{CapturePolicy, CapturedPage, MemoryError, MemoryService, SearchRequest};
use std::path::Path;
use url::Url;

fn url(raw: &str) -> Url {
    Url::parse(raw).expect("url")
}

fn page(raw_url: &str, html: &str, text: &str) -> CapturedPage {
    CapturedPage {
        url: url(raw_url),
        title: "Notes".to_string(),
        html: html.to_string(),
        text: text.to_string(),
        content_hash: "pending".to_string(),
        captured_at: 1_700_000_000_000,
    }
}

fn json_files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.join("pages"))
        .expect("pages dir")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.ends_with(".json"))
        .collect();
    names.sort();
    names
}

#[tokio::test]
async fn capture_search_forget_removes_the_page_and_tombstones_the_domain() {
    let dir = tempfile::tempdir().expect("temp dir");
    let service = MemoryService::open(dir.path()).expect("open");
    let mut policy = CapturePolicy::default();

    let remembered = page(
        "https://example.com/docs",
        "<h1>Notes</h1><p>zephyrquartz lives in the body.</p>",
        "zephyrquartz lives in the body.",
    );
    let kept = page(
        "https://other.test/notes",
        "<h1>Other</h1><p>otherhosttoken stays indexed.</p>",
        "otherhosttoken stays indexed.",
    );
    let remembered_url = remembered.url.clone();

    service
        .capture(remembered, &policy)
        .await
        .expect("capture example.com");
    service
        .capture(kept, &policy)
        .await
        .expect("capture other.test");
    assert_eq!(json_files(dir.path()).len(), 2);

    let query = SearchRequest {
        query: "zephyrquartz".to_string(),
        limit: 5,
    };
    let hits = service.search(query.clone()).await.expect("search");
    assert_eq!(hits.len(), 1, "{hits:?}");
    assert_eq!(hits[0].page_url, remembered_url);
    assert!(hits[0].text.contains("zephyrquartz"), "{hits:?}");
    assert!(hits[0].explain.is_none());

    let explained = service.explain(&query, &hits[0]).await.expect("explain");
    assert!(
        explained
            .snippets
            .iter()
            .any(|snippet| snippet.to_lowercase().contains("zephyrquartz")),
        "{explained:?}"
    );
    assert!(
        explained
            .breakdown
            .iter()
            .any(|component| component.field == "text" && component.score > 0.0),
        "{explained:?}"
    );

    service
        .forget(&remembered_url, &mut policy)
        .await
        .expect("forget");
    assert_eq!(policy.denied_domains, vec!["example.com".to_string()]);

    let gone = service
        .search(query.clone())
        .await
        .expect("search after forget");
    assert!(gone.is_empty(), "{gone:?}");

    let kept_hits = service
        .search(SearchRequest {
            query: "otherhosttoken".to_string(),
            limit: 5,
        })
        .await
        .expect("search kept page");
    assert_eq!(kept_hits.len(), 1, "{kept_hits:?}");
    assert_eq!(kept_hits[0].page_url.as_str(), "https://other.test/notes");

    let remaining = json_files(dir.path());
    assert_eq!(remaining.len(), 1);
    let stored = std::fs::read_to_string(dir.path().join("pages").join(&remaining[0]))
        .expect("read remaining page");
    assert!(stored.contains("https://other.test/notes"));
    assert!(!stored.contains("https://example.com/docs"));

    let again = page(
        "https://example.com/docs",
        "<h1>Notes</h1><p>zephyrquartz lives in the body.</p>",
        "zephyrquartz lives in the body.",
    );
    let denied = service
        .capture(again, &policy)
        .await
        .expect_err("recapture");
    assert!(
        matches!(denied, MemoryError::Denied { ref reason } if reason.contains("example.com")),
        "{denied:?}"
    );
    assert!(service
        .search(query)
        .await
        .expect("search after denied recapture")
        .is_empty());
    assert_eq!(json_files(dir.path()).len(), 1);

    let child = page(
        "https://docs.example.com/guide",
        "<h1>Guide</h1><p>childtoken should stay out.</p>",
        "childtoken should stay out.",
    );
    let child_denied = service
        .capture(child, &policy)
        .await
        .expect_err("subdomain recapture");
    assert!(
        matches!(child_denied, MemoryError::Denied { .. }),
        "{child_denied:?}"
    );

    service
        .forget(&remembered_url, &mut policy)
        .await
        .expect("forget again");
    assert_eq!(policy.denied_domains, vec!["example.com".to_string()]);
    assert_eq!(
        service
            .search(SearchRequest {
                query: "otherhosttoken".to_string(),
                limit: 5,
            })
            .await
            .expect("kept page survives a second forget")
            .len(),
        1
    );
}
