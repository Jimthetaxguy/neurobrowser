//! Independently authored evidence-budget and real HTTP boundary checks.
use async_trait::async_trait;
use neurobrowser::{
    BrowserEngine, BrowserInterface, DispatchState, DocumentStamp, ElementInfo, FormInfo,
    FormInputInfo, LinkInfo, ObservationLimits, ObservedTarget, PageConfig, PageObservation,
    PageSnapshot, RuntimeCapabilities, RuntimeKind, TableInfo, TargetAction, TargetCommand,
};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn evidence_snapshot() -> PageSnapshot {
    PageSnapshot {
        url: "https://example.com/evidence?route=north&review=unchanged".into(),
        title: "Shipment evidence".into(),
        text: Some("Depot stock: 17 crates.".into()),
        html: Some("<form><input type=password value=RAW_HTML_CANARY_91></form>".into()),
        forms: vec![FormInfo {
            action: "/dispatch".into(),
            method: "post".into(),
            inputs: vec![FormInputInfo {
                name: "password".into(),
                input_type: "password".into(),
                value: Some("RAW_FORM_VALUE_CANARY_82".into()),
            }],
        }],
        ..PageSnapshot::default()
    }
}

#[test]
fn native_authority_and_destinations_are_preserved_exactly() {
    let authority = format!("https://example.com/manifest?evidence={}", "z".repeat(3800));
    let destination = format!("https://example.com/dispatch?review={}", "r".repeat(3800));
    let mut snapshot = evidence_snapshot();
    snapshot.url = authority.clone();
    snapshot.links = vec![LinkInfo {
        href: destination.clone(),
        text: "Manifest".into(),
    }];
    let mut observed = PageObservation::from_snapshot(
        snapshot,
        RuntimeCapabilities::desktop(),
        ObservationLimits::default(),
    );
    observed.document = Some(DocumentStamp {
        runtime_id: "page-runtime-17".into(),
        document_id: "document-31".into(),
        revision: 5,
    });
    observed.targets.push(ObservedTarget {
        id: "target-19".into(),
        role: "link".into(),
        label: "Manifest".into(),
        tag: "a".into(),
        disabled: false,
        sensitive: false,
        destination: Some(destination.clone()),
    });
    observed.apply_limits(ObservationLimits::default());
    observed.validate().expect("bounded exact authority");
    assert_eq!(observed.url, authority);
    assert_eq!(observed.links[0].href, destination);
    assert_eq!(
        observed.targets[0].destination.as_deref(),
        Some(destination.as_str())
    );
    assert_eq!(observed.document.unwrap().runtime_id, "page-runtime-17");
}

#[test]
fn oversized_target_destination_is_rejected_without_truncation() {
    let destination = format!("https://example.com/dispatch?review={}", "r".repeat(4096));
    let mut observed = PageObservation::from_snapshot(
        evidence_snapshot(),
        RuntimeCapabilities::desktop(),
        ObservationLimits::default(),
    );
    observed.targets.push(ObservedTarget {
        id: "target-oversized".into(),
        role: "link".into(),
        label: "Manifest".into(),
        tag: "a".into(),
        disabled: false,
        sensitive: false,
        destination: Some(destination.clone()),
    });
    observed.apply_limits(ObservationLimits::default());
    assert_eq!(
        observed.targets[0].destination.as_deref(),
        Some(destination.as_str())
    );
    assert!(observed.validate().is_err());
}

#[test]
fn oversized_authority_is_rejected_without_aliasing() {
    let oversized = format!("https://example.com/{}", "a".repeat(4096));
    let mut snapshot = evidence_snapshot();
    snapshot.url = oversized.clone();
    let observed = PageObservation::from_snapshot(
        snapshot,
        RuntimeCapabilities::http(),
        ObservationLimits::default(),
    );
    assert_eq!(
        observed.url, oversized,
        "authority must never be shortened to make validation pass"
    );
    assert!(observed.validate().is_err());
}

#[test]
fn caller_limits_use_utf8_bytes_without_splitting_characters() {
    let mut snapshot = evidence_snapshot();
    snapshot.text = Some("é🙂北".repeat(3000));
    snapshot.title = "北".repeat(1000);
    snapshot.links = vec![LinkInfo {
        href: "https://example.com/exact".into(),
        text: "🙂".repeat(500),
    }];
    let observed = PageObservation::from_snapshot(
        snapshot,
        RuntimeCapabilities::http(),
        ObservationLimits {
            max_text_bytes: 11,
            max_targets: 0,
            max_links: 1,
            max_tables: 0,
            max_rows: 0,
            max_cell_bytes: 5,
        },
    );
    assert_eq!(observed.text, "é🙂北é");
    assert_eq!(observed.text.len(), 11);
    assert!(observed.title.len() <= 512);
    assert!(observed.links[0].text.len() <= 240);
    assert!(observed
        .omissions
        .iter()
        .any(|reason| reason == "Page text truncated"));
    observed.validate().unwrap();
}

#[test]
fn entire_serialized_envelope_respects_budget_including_json_escapes() {
    let mut snapshot = evidence_snapshot();
    snapshot.text = Some("\"\\\n".repeat(8000));
    snapshot.links = (0..100)
        .map(|index| LinkInfo {
            href: format!("https://example.com/{index}?q={}", "r".repeat(3800)),
            text: "\"\\".repeat(500),
        })
        .collect();
    snapshot.tables = (0..25)
        .map(|_| TableInfo {
            headers: vec!["\"\\\t".repeat(200); 32],
            rows: vec![vec!["\"\\\n".repeat(200); 32]; 64],
        })
        .collect();
    let mut observed = PageObservation::from_snapshot(
        snapshot,
        RuntimeCapabilities::desktop(),
        ObservationLimits::default(),
    );
    observed.targets = (0..100)
        .map(|index| ObservedTarget {
            id: format!("target-{index}"),
            role: "button".repeat(50),
            label: "\"\\".repeat(500),
            tag: "button".into(),
            disabled: false,
            sensitive: false,
            destination: Some(format!(
                "https://example.com/{index}?q={}",
                "z".repeat(3800)
            )),
        })
        .collect();
    observed.apply_limits(ObservationLimits::default());
    let bytes = serde_json::to_vec(&observed).unwrap();
    assert!(
        bytes.len() <= 64 * 1024,
        "{} serialized bytes exceeds host ceiling",
        bytes.len()
    );
    assert!(observed
        .omissions
        .iter()
        .any(|reason| reason == "Total observation byte budget reached"));
    assert!(observed
        .omissions
        .iter()
        .any(|reason| reason == "Links truncated"));
    assert!(observed
        .omissions
        .iter()
        .any(|reason| reason == "Tables truncated"));
    assert!(observed
        .omissions
        .iter()
        .any(|reason| reason == "Table cells or rows truncated"));
    assert!(observed
        .omissions
        .iter()
        .any(|reason| reason == "Targets truncated"));
    observed.validate().unwrap();
}

#[test]
fn shared_envelope_excludes_raw_html_and_form_value_fields() {
    let observed = PageObservation::from_snapshot(
        evidence_snapshot(),
        RuntimeCapabilities::http(),
        ObservationLimits::default(),
    );
    let value = serde_json::to_value(&observed).unwrap();
    let serialized = value.to_string();
    assert!(value.get("html").is_none());
    assert!(value.get("forms").is_none());
    assert!(value.get("images").is_none());
    assert!(!serialized.contains("RAW_HTML_CANARY_91"));
    assert!(!serialized.contains("RAW_FORM_VALUE_CANARY_82"));
    assert!(observed
        .omissions
        .iter()
        .any(|reason| reason.contains("input values")));
}

// A legacy adapter is deliberately restricted to this test module. It verifies
// that an implementation which does not opt into new capabilities stays conservative.
struct LegacyAdapter;
#[async_trait]
impl BrowserInterface for LegacyAdapter {
    async fn navigate(&self, _: &str) -> Result<(), String> {
        Err("unsupported".into())
    }
    async fn query_selector(&self, _: &str) -> Result<Vec<ElementInfo>, String> {
        Err("unsupported".into())
    }
    async fn get_text(&self, _: &str) -> Result<String, String> {
        Err("unsupported".into())
    }
    async fn click(&self, _: &str) -> Result<(), String> {
        Err("unsupported".into())
    }
    async fn type_text(&self, _: &str, _: &str) -> Result<(), String> {
        Err("unsupported".into())
    }
    async fn submit_form(&self, _: &str) -> Result<(), String> {
        Err("unsupported".into())
    }
    async fn scroll_to(&self, _: &str) -> Result<(), String> {
        Err("unsupported".into())
    }
    async fn scroll_by(&self, _: f32, _: f32) -> Result<(), String> {
        Err("unsupported".into())
    }
    async fn snapshot(&self) -> Result<PageSnapshot, String> {
        Ok(evidence_snapshot())
    }
}

#[tokio::test]
async fn unknown_adapter_does_not_invent_execution_guarantees() {
    let adapter = LegacyAdapter;
    let observed = adapter.observe(ObservationLimits::default()).await.unwrap();
    let capabilities = observed.capabilities;
    assert_eq!(capabilities.runtime, RuntimeKind::Unknown);
    assert!(!capabilities.javascript && !capabilities.interaction && !capabilities.scoped_targets);
    assert!(
        !capabilities.native_url
            && !capabilities.screenshots
            && !capabilities.background_javascript
    );
    assert!(!capabilities.enforcing_subresource_network);
    assert!(observed.document.is_none());
    assert!(observed.targets.is_empty());
    let error = adapter
        .dispatch_target(&TargetCommand {
            document: DocumentStamp {
                runtime_id: "test".into(),
                document_id: "test".into(),
                revision: 0,
            },
            target_id: "test".into(),
            action: TargetAction::Click,
            text: None,
        })
        .await
        .unwrap_err();
    assert_eq!(error.state, DispatchState::NotDispatched);
}

#[tokio::test]
async fn default_http_engine_denies_loopback_before_any_real_tcp_request() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let requests = Arc::new(AtomicUsize::new(0));
    let server_stopped = Arc::clone(&stopped);
    let server_requests = Arc::clone(&requests);
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !server_stopped.load(Ordering::SeqCst) && Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    server_requests.fetch_add(1, Ordering::SeqCst);
                    stream
                        .set_read_timeout(Some(Duration::from_millis(100)))
                        .unwrap();
                    let mut input = [0; 2048];
                    let _ = stream.read(&mut input);
                    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 16\r\nConnection: close\r\n\r\nLoopback canary.");
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(error) => panic!("local server accept failed: {error}"),
            }
        }
    });
    let engine = BrowserEngine::new(PageConfig::default());
    let result = engine
        .navigate(&format!("http://{address}/sensitive"))
        .await;
    // Leave a short observation interval after the returned refusal, then join our server.
    std::thread::sleep(Duration::from_millis(100));
    stopped.store(true, Ordering::SeqCst);
    server.join().unwrap();
    assert!(result.unwrap_err().contains("loopback"));
    assert_eq!(
        requests.load(Ordering::SeqCst),
        0,
        "engine contacted a denied loopback server"
    );
    assert!(engine.snapshot().await.unwrap().url.is_empty());
}

#[tokio::test]
#[ignore = "requires live public HTTPS access; run explicitly as an integration check"]
async fn real_public_http_engine_observes_example_domain() {
    let engine = BrowserEngine::new(PageConfig::default());
    engine
        .navigate("https://example.com/")
        .await
        .expect("real public HTTPS navigation");
    let observation = engine
        .observe(ObservationLimits::default())
        .await
        .expect("real HTML observation");
    observation.validate().unwrap();
    assert_eq!(observation.url, "https://example.com/");
    assert_eq!(observation.title, "Example Domain");
    assert!(observation.text.contains("Example Domain"));
    // The site's current editorial links are evidence, not part of our contract.
    // Log them for the live receipt without depending on their spelling or destination.
    eprintln!(
        "Live HTTP evidence: url={} title={:?} text_bytes={} links={:?}",
        observation.url,
        observation.title,
        observation.text.len(),
        observation.links
    );
    assert!(serde_json::to_vec(&observation).unwrap().len() <= 64 * 1024);
    assert_eq!(observation.capabilities.runtime, RuntimeKind::Http);
    assert!(observation.capabilities.enforcing_subresource_network);
    assert!(!observation.capabilities.javascript && !observation.capabilities.interaction);
    assert!(observation.document.is_none() && observation.targets.is_empty());
}
