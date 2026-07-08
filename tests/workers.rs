//! Tests for Phase E — Worker model.
//!
//! Covers:
//! - E1: `SessionManager::spawn_worker` returns a handle with a unique id,
//!   `list_workers` returns it.
//! - E2: `record_observation` + `cross_worker_observations` round-trip an
//!   AgentEvent across the session.
//! - E4: `send_message` + `drain_inbox` route messages between workers in
//!   the same session.

use neurobrowser::agent::memory::AgentEvent;
use neurobrowser::agent::worker::{WorkerMessage, WorkerMessageKind, WorkerSpec, WorkerStatus};
use neurobrowser::providers::{ProviderConfig, ProviderType};
use neurobrowser::{ActionPolicy, AgentConfig, PageConfig, SessionManager};
use serde_json::json;

fn stub_provider_config() -> ProviderConfig {
    ProviderConfig {
        provider_type: ProviderType::Custom,
        api_key: None,
        base_url: None,
        model: "stub".to_string(),
        max_tokens: Some(64),
        temperature: Some(0.0),
    }
}

#[test]
fn spawn_worker_yields_unique_id_and_lists_in_session() {
    let manager = SessionManager::new(
        PageConfig::default(),
        AgentConfig {
            max_iterations: 5,
            provider_config: stub_provider_config(),
        },
    );
    let sid = manager.create_session();
    let worker_a = manager
        .spawn_worker(
            &sid,
            WorkerSpec {
                name: "A".to_string(),
                goal: "first".to_string(),
                policy: ActionPolicy::default(),
                max_iterations: 5,
                pinned_page_id: None,
            },
        )
        .expect("spawn A");
    let worker_b = manager
        .spawn_worker(
            &sid,
            WorkerSpec {
                name: "B".to_string(),
                goal: "second".to_string(),
                policy: ActionPolicy::default(),
                max_iterations: 5,
                pinned_page_id: None,
            },
        )
        .expect("spawn B");
    assert_ne!(worker_a.worker_id, worker_b.worker_id);
    let listed = manager.list_workers(&sid).expect("list workers");
    assert_eq!(listed.len(), 2);
    let names: Vec<&str> = listed.iter().map(|w| w.name.as_str()).collect();
    assert!(names.contains(&"A"));
    assert!(names.contains(&"B"));
}

#[test]
fn get_worker_returns_snapshot_with_goal_and_policy() {
    let manager = SessionManager::new(
        PageConfig::default(),
        AgentConfig {
            max_iterations: 5,
            provider_config: stub_provider_config(),
        },
    );
    let sid = manager.create_session();
    let worker = manager
        .spawn_worker(
            &sid,
            WorkerSpec {
                name: "snap".to_string(),
                goal: "do a thing".to_string(),
                policy: ActionPolicy::default(),
                max_iterations: 7,
                pinned_page_id: None,
            },
        )
        .expect("spawn");
    let snap = manager
        .get_worker(&sid, &worker.worker_id)
        .expect("snapshot");
    assert_eq!(snap.summary.name, "snap");
    assert_eq!(snap.goal, "do a thing");
    assert_eq!(snap.summary.status, WorkerStatus::Idle);
}

#[test]
fn cross_worker_observations_round_trip() {
    let manager = SessionManager::new(
        PageConfig::default(),
        AgentConfig {
            max_iterations: 5,
            provider_config: stub_provider_config(),
        },
    );
    let sid = manager.create_session();
    // Record 3 observations; we should see all 3 via cross_worker_observations.
    for i in 0..3 {
        manager
            .record_observation(
                &sid,
                AgentEvent::Navigation {
                    url: format!("https://example.com/n{i}"),
                    timestamp: i,
                },
            )
            .expect("record");
    }
    let recent = manager
        .cross_worker_observations(&sid, 2)
        .expect("read observations");
    assert_eq!(recent.len(), 2);
    let last = recent.last().unwrap();
    assert_eq!(
        last.get("url").and_then(serde_json::Value::as_str),
        Some("https://example.com/n2"),
    );
}

#[test]
fn send_message_routes_between_workers() {
    let manager = SessionManager::new(
        PageConfig::default(),
        AgentConfig {
            max_iterations: 5,
            provider_config: stub_provider_config(),
        },
    );
    let sid = manager.create_session();
    let a = manager
        .spawn_worker(
            &sid,
            WorkerSpec {
                name: "sender".to_string(),
                goal: String::new(),
                policy: ActionPolicy::default(),
                max_iterations: 1,
                pinned_page_id: None,
            },
        )
        .expect("spawn A");
    let b = manager
        .spawn_worker(
            &sid,
            WorkerSpec {
                name: "receiver".to_string(),
                goal: String::new(),
                policy: ActionPolicy::default(),
                max_iterations: 1,
                pinned_page_id: None,
            },
        )
        .expect("spawn B");

    let msg = WorkerMessage::now(
        WorkerMessageKind::Observation,
        &a.worker_id,
        &b.worker_id,
        json!({ "note": "page loaded" }),
    );
    manager.send_message(&sid, msg).expect("send");

    let delivered = manager
        .drain_inbox(&sid, Some(&b.worker_id))
        .expect("drain");
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].from, a.worker_id);
    assert_eq!(delivered[0].to, b.worker_id);
    assert_eq!(
        delivered[0]
            .payload
            .get("note")
            .and_then(serde_json::Value::as_str),
        Some("page loaded"),
    );
}

#[test]
fn drain_inbox_for_non_recipient_returns_empty() {
    let manager = SessionManager::new(
        PageConfig::default(),
        AgentConfig {
            max_iterations: 5,
            provider_config: stub_provider_config(),
        },
    );
    let sid = manager.create_session();
    let a = manager
        .spawn_worker(
            &sid,
            WorkerSpec {
                name: "sender".to_string(),
                goal: String::new(),
                policy: ActionPolicy::default(),
                max_iterations: 1,
                pinned_page_id: None,
            },
        )
        .expect("A");
    let b = manager
        .spawn_worker(
            &sid,
            WorkerSpec {
                name: "receiver".to_string(),
                goal: String::new(),
                policy: ActionPolicy::default(),
                max_iterations: 1,
                pinned_page_id: None,
            },
        )
        .expect("B");
    let other = manager
        .spawn_worker(
            &sid,
            WorkerSpec {
                name: "uninvolved".to_string(),
                goal: String::new(),
                policy: ActionPolicy::default(),
                max_iterations: 1,
                pinned_page_id: None,
            },
        )
        .expect("C");

    let msg = WorkerMessage::now(
        WorkerMessageKind::Handoff,
        &a.worker_id,
        &b.worker_id,
        json!({ "ok": true }),
    );
    manager.send_message(&sid, msg).expect("send");

    let for_other = manager
        .drain_inbox(&sid, Some(&other.worker_id))
        .expect("drain for other");
    assert!(for_other.is_empty());

    let for_b = manager
        .drain_inbox(&sid, Some(&b.worker_id))
        .expect("drain for b");
    assert_eq!(for_b.len(), 1);
}

#[test]
fn set_worker_status_updates_summary() {
    let manager = SessionManager::new(
        PageConfig::default(),
        AgentConfig {
            max_iterations: 5,
            provider_config: stub_provider_config(),
        },
    );
    let sid = manager.create_session();
    let worker = manager
        .spawn_worker(
            &sid,
            WorkerSpec {
                name: "runner".to_string(),
                goal: String::new(),
                policy: ActionPolicy::default(),
                max_iterations: 1,
                pinned_page_id: None,
            },
        )
        .expect("spawn");
    manager
        .set_worker_status(&sid, &worker.worker_id, WorkerStatus::Running)
        .expect("set Running");
    let snap = manager
        .get_worker(&sid, &worker.worker_id)
        .expect("snapshot");
    assert_eq!(snap.summary.status, WorkerStatus::Running);
    manager
        .set_worker_status(&sid, &worker.worker_id, WorkerStatus::Completed)
        .expect("set Completed");
    let snap = manager
        .get_worker(&sid, &worker.worker_id)
        .expect("snapshot");
    assert_eq!(snap.summary.status, WorkerStatus::Completed);
}
