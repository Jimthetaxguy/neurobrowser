use neuro_memory::{
    now_millis, CapturedPage, MemoryError, MemoryService, ScoreComponent, SearchExplain,
    SearchRequest, SearchResult, SemanticBlock,
};
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

fn url(raw: &str) -> Url {
    Url::parse(raw).expect("fixture url")
}

fn captured_page() -> CapturedPage {
    CapturedPage {
        url: url("https://example.com/docs"),
        title: "Docs".to_string(),
        html: "<h1>Docs</h1><p>Hello</p>".to_string(),
        text: "Docs\nHello".to_string(),
        content_hash: "pending".to_string(),
        captured_at: 1_700_000_000_000,
    }
}

fn block() -> SemanticBlock {
    SemanticBlock {
        block_id: "block-1".to_string(),
        page_url: url("https://example.com/docs"),
        heading_path: vec!["Docs".to_string()],
        text: "Hello".to_string(),
        captured_at: 1_700_000_000_000,
    }
}

fn request() -> SearchRequest {
    SearchRequest {
        query: "Hello".to_string(),
        limit: 5,
    }
}

fn hit() -> SearchResult {
    let block = block();
    SearchResult {
        block_id: block.block_id,
        page_url: block.page_url,
        heading_path: block.heading_path,
        text: block.text,
        score: 1.5,
        captured_at: block.captured_at,
        explain: Some(SearchExplain {
            breakdown: vec![ScoreComponent {
                field: "text".to_string(),
                score: 1.5,
            }],
            snippets: vec!["Hello".to_string()],
        }),
    }
}

#[test]
fn now_millis_matches_system_time() {
    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_millis() as u64;
    let stamped = now_millis();
    let after = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_millis() as u64;

    assert!(stamped >= before);
    assert!(stamped <= after);
}

#[tokio::test]
async fn open_records_data_dir_and_methods_are_stubs() {
    let dir = tempfile::tempdir().expect("temp dir");
    let service = MemoryService::open(dir.path()).expect("open");
    assert_eq!(service.data_dir(), dir.path());

    let page = captured_page();
    let query = request();
    let result = hit();

    assert_eq!(
        service.capture(page).await.unwrap_err(),
        MemoryError::NotImplemented { task: "M1.7" }
    );
    assert_eq!(
        service.search(query.clone()).await.unwrap_err(),
        MemoryError::NotImplemented { task: "M1.6" }
    );
    assert_eq!(
        service.explain(&query, &result).await.unwrap_err(),
        MemoryError::NotImplemented { task: "M1.6" }
    );
    assert_eq!(
        service
            .forget(&url("https://example.com/docs"))
            .await
            .unwrap_err(),
        MemoryError::NotImplemented { task: "M1.7" }
    );
}
