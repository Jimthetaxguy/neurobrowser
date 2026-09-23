use neuro_memory::index::{BlockIndex, IndexError};
use neuro_memory::{extract_blocks, CapturedPage, SemanticBlock};
use std::path::Path;
use tantivy::schema::{FieldType, IndexRecordOption, Schema};
use url::Url;

fn page(url: &str, html: &str, text: &str, hash: &str, captured_at: u64) -> CapturedPage {
    CapturedPage {
        url: Url::parse(url).expect("fixture url"),
        title: "Guide".to_string(),
        html: html.to_string(),
        text: text.to_string(),
        content_hash: hash.to_string(),
        captured_at,
    }
}

fn guide_page() -> CapturedPage {
    page(
        "https://example.com/docs?topic=install",
        "<h1>Guide</h1><p>Alpha token uniquealpha.</p><h2>Install</h2><p>Beta token uniquebeta.</p>",
        "unused when html yields blocks",
        "guide-hash",
        1_700_000_000_000,
    )
}

fn notes_page() -> CapturedPage {
    page(
        "https://example.com/notes",
        "",
        "First plain paragraph.\n\nSecond plain paragraph.",
        "notes-hash",
        1_700_000_111_111,
    )
}

fn sorted(mut blocks: Vec<SemanticBlock>) -> Vec<SemanticBlock> {
    blocks.sort_by(|left, right| left.block_id.cmp(&right.block_id));
    blocks
}

fn text_field<'a>(schema: &'a Schema, name: &str) -> &'a tantivy::schema::TextOptions {
    let field = schema
        .get_field(name)
        .unwrap_or_else(|_| panic!("missing {name}"));
    match schema.get_field_entry(field).field_type() {
        FieldType::Str(options) => options,
        other => panic!("{name} is {other:?}"),
    }
}

fn assert_text_field(schema: &Schema, name: &str, tokenizer: &str, record: IndexRecordOption) {
    let field = schema
        .get_field(name)
        .unwrap_or_else(|_| panic!("missing {name}"));
    let entry = schema.get_field_entry(field);
    assert!(entry.is_indexed(), "{name} should be indexed");
    assert!(entry.is_stored(), "{name} should be stored");
    let indexing = text_field(schema, name)
        .get_indexing_options()
        .unwrap_or_else(|| panic!("{name} has no indexing options"));
    assert_eq!(indexing.tokenizer(), tokenizer, "{name} tokenizer");
    assert_eq!(indexing.index_option(), record, "{name} record option");
}

#[test]
fn block_index_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<BlockIndex>();
}

#[test]
fn schema_matches_block_fields() {
    let dir = tempfile::tempdir().expect("temp dir");
    let index = BlockIndex::open_or_create(dir.path()).expect("create index");
    let schema = index.schema();

    assert_text_field(&schema, "block_id", "raw", IndexRecordOption::Basic);
    assert_text_field(&schema, "page_url", "raw", IndexRecordOption::Basic);
    assert_text_field(
        &schema,
        "heading_path",
        "default",
        IndexRecordOption::WithFreqsAndPositions,
    );
    assert_text_field(
        &schema,
        "text",
        "default",
        IndexRecordOption::WithFreqsAndPositions,
    );

    let captured_at = schema.get_field("captured_at").expect("captured_at");
    let entry = schema.get_field_entry(captured_at);
    assert!(entry.is_fast(), "captured_at should be a fast field");
    assert!(entry.is_stored(), "captured_at should be stored");
    assert!(
        !entry.is_indexed(),
        "captured_at is a fast u64, not an indexed numeric"
    );
    assert!(
        matches!(entry.field_type(), FieldType::U64(_)),
        "captured_at should be u64"
    );
    assert!(dir.path().join("meta.json").is_file());
}

#[test]
fn add_commit_remove_round_trip_survives_reopen() {
    let dir = tempfile::tempdir().expect("temp dir");
    let guide = guide_page();
    let notes = notes_page();
    let guide_blocks = extract_blocks(&guide);
    let notes_blocks = extract_blocks(&notes);

    assert!(
        guide_blocks.len() >= 2,
        "guide fixture should split on headings"
    );
    assert!(
        guide_blocks
            .iter()
            .any(|block| block.heading_path.len() >= 2),
        "nested heading path should survive extraction"
    );
    assert!(guide_blocks
        .iter()
        .any(|block| block.text.contains("uniquealpha")));
    assert_eq!(notes_blocks.len(), 1);
    assert!(notes_blocks[0].heading_path.is_empty());
    assert!(notes_blocks[0].text.contains("First plain paragraph."));

    let index = BlockIndex::open_or_create(dir.path()).expect("create index");
    for block in &guide_blocks {
        index.add_block(block).expect("queue guide block");
    }
    assert!(index
        .blocks_for_url(&guide.url)
        .expect("read uncommitted")
        .is_empty());

    index.commit().expect("commit guide");
    assert_eq!(
        sorted(index.blocks_for_url(&guide.url).expect("read guide")),
        sorted(guide_blocks.clone())
    );

    for block in &notes_blocks {
        index.add_block(block).expect("queue notes block");
    }
    assert!(index
        .blocks_for_url(&notes.url)
        .expect("notes still uncommitted")
        .is_empty());
    index.commit().expect("commit notes");
    assert_eq!(
        sorted(index.blocks_for_url(&notes.url).expect("read notes")),
        sorted(notes_blocks.clone())
    );

    index.remove_by_url(&guide.url).expect("queue delete");
    assert_eq!(
        sorted(
            index
                .blocks_for_url(&guide.url)
                .expect("delete uncommitted")
        ),
        sorted(guide_blocks.clone())
    );
    assert_eq!(
        sorted(index.blocks_for_url(&notes.url).expect("notes untouched")),
        sorted(notes_blocks.clone())
    );

    index.commit().expect("commit delete");
    assert!(index
        .blocks_for_url(&guide.url)
        .expect("guide gone")
        .is_empty());
    assert_eq!(
        sorted(index.blocks_for_url(&notes.url).expect("notes remain")),
        sorted(notes_blocks.clone())
    );

    let recaptured = page(
        guide.url.as_str(),
        "<h1>Guide</h1><p>Replaced body uniquereplace.</p>",
        "",
        "guide-hash-2",
        1_700_000_222_222,
    );
    let recaptured_blocks = extract_blocks(&recaptured);
    assert!(!recaptured_blocks.is_empty());
    index
        .remove_by_url(&guide.url)
        .expect("queue replace delete");
    for block in &recaptured_blocks {
        index.add_block(block).expect("queue replacement");
    }
    index.commit().expect("commit replace");
    assert_eq!(
        sorted(index.blocks_for_url(&guide.url).expect("read replacement")),
        sorted(recaptured_blocks.clone())
    );
    assert_eq!(
        sorted(
            index
                .blocks_for_url(&notes.url)
                .expect("notes still remain")
        ),
        sorted(notes_blocks.clone())
    );

    drop(index);
    let reopened = BlockIndex::open_or_create(dir.path()).expect("reopen");
    assert_eq!(
        sorted(reopened.blocks_for_url(&guide.url).expect("reopened guide")),
        sorted(recaptured_blocks)
    );
    assert_eq!(
        sorted(reopened.blocks_for_url(&notes.url).expect("reopened notes")),
        sorted(notes_blocks)
    );
}

#[test]
fn open_or_create_creates_missing_directory_and_rejects_conflicts() {
    let dir = tempfile::tempdir().expect("temp dir");
    let nested = dir.path().join("memory").join("index");
    assert!(!nested.exists());
    let index = BlockIndex::open_or_create(&nested).expect("create nested");
    assert!(nested.join("meta.json").is_file());
    drop(index);

    let held = BlockIndex::open_or_create(&nested).expect("hold writer");
    let busy = BlockIndex::open_or_create(&nested).expect_err("second writer");
    assert!(
        matches!(
            busy,
            IndexError::Tantivy(tantivy::TantivyError::LockFailure(..))
        ),
        "expected a writer lock error, got {busy}"
    );
    drop(held);

    let foreign = dir.path().join("foreign");
    std::fs::create_dir_all(&foreign).expect("foreign dir");
    write_foreign_index(&foreign);
    let mismatch = BlockIndex::open_or_create(&foreign).expect_err("foreign schema");
    assert!(
        matches!(
            mismatch,
            IndexError::Tantivy(tantivy::TantivyError::SchemaError(_))
        ),
        "expected a schema mismatch, got {mismatch}"
    );
    assert!(
        foreign.join("meta.json").is_file(),
        "a foreign index must be left in place"
    );
}

fn write_foreign_index(dir: &Path) {
    let mut builder = Schema::builder();
    builder.add_text_field("other", tantivy::schema::TEXT);
    let schema = builder.build();
    tantivy::Index::create_in_dir(dir, schema).expect("foreign index");
}
