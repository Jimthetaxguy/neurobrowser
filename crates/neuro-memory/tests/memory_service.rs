use neuro_memory::{now_millis, MemoryService};
use std::time::{SystemTime, UNIX_EPOCH};

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

#[test]
fn open_creates_page_store_and_index_directories() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<MemoryService>();

    let dir = tempfile::tempdir().expect("temp dir");
    let service = MemoryService::open(dir.path()).expect("open");
    assert_eq!(service.data_dir(), dir.path());
    assert!(dir.path().join("pages").is_dir());
    assert!(dir.path().join("index").is_dir());
}
