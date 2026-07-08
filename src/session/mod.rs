use crate::agent::worker::{
    CrossWorkerObservations, WorkerHandle, WorkerMessage, WorkerSnapshot, WorkerSpec, WorkerStatus,
    WorkerSummary,
};
use crate::agent::{AgentConfig, ReActAgent};
use crate::browser::PageConfig;
use crate::providers::{create_provider, ProviderConfig};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct SessionManager {
    sessions: Mutex<HashMap<String, SessionState>>,
    #[allow(dead_code)]
    browser_config: PageConfig,
    agent_config: Mutex<AgentConfig>,
    page_counter: Mutex<usize>,
}

struct SessionState {
    id: String,
    created_at: u64,
    pages: Vec<PageHandle>,
    #[allow(dead_code)]
    active_page: Option<usize>,
    workers: HashMap<String, WorkerHandle>,
    inbox: Vec<WorkerMessage>,
    observations: CrossWorkerObservations,
}

impl SessionManager {
    pub fn new(browser_config: PageConfig, agent_config: AgentConfig) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            browser_config,
            agent_config: Mutex::new(agent_config),
            page_counter: Mutex::new(0),
        }
    }

    pub fn create_session(&self) -> String {
        let id = uuid_v4();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(
            id.clone(),
            SessionState {
                id: id.clone(),
                created_at: now,
                pages: Vec::new(),
                active_page: None,
                workers: HashMap::new(),
                inbox: Vec::new(),
                observations: CrossWorkerObservations::default(),
            },
        );

        id
    }

    pub fn get_session(&self, id: &str) -> Option<SessionInfo> {
        self.sessions.lock().unwrap().get(id).map(|s| SessionInfo {
            id: s.id.clone(),
            created_at: s.created_at,
            page_count: s.pages.len(),
        })
    }

    pub fn create_page(&self, session_id: &str) -> Result<PageHandle, String> {
        // Allocate the page id *before* taking the sessions lock. Acquiring
        // page_counter while holding sessions would deadlock if any other
        // call path acquires sessions first and then page_counter (e.g. a
        // future parallel-create helper or a test).
        let page_id = {
            let mut counter = self.page_counter.lock().unwrap();
            let id = *counter;
            *counter += 1;
            id
        };

        let agent_config = self.agent_config.lock().unwrap().clone();
        let provider = create_provider(&agent_config.provider_config);
        let agent = Arc::new(ReActAgent::new(agent_config, provider));

        let handle = PageHandle {
            id: page_id,
            runtime_id: format!("page-runtime-{page_id}"),
            agent,
        };

        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions.get_mut(session_id).ok_or("Session not found")?;
        session.pages.push(handle.clone());
        session.active_page = Some(page_id);

        Ok(handle)
    }

    pub fn get_page(&self, session_id: &str, page_id: usize) -> Result<PageHandle, String> {
        let sessions = self.sessions.lock().unwrap();
        let session = sessions.get(session_id).ok_or("Session not found")?;
        session
            .pages
            .iter()
            .find(|p| p.id == page_id)
            .cloned()
            .ok_or("Page not found".to_string())
    }

    pub fn list_sessions(&self) -> Vec<SessionInfo> {
        self.sessions
            .lock()
            .unwrap()
            .values()
            .map(|s| SessionInfo {
                id: s.id.clone(),
                created_at: s.created_at,
                page_count: s.pages.len(),
            })
            .collect()
    }

    pub fn set_active_page(&self, session_id: &str, page_id: usize) -> Result<(), String> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions.get_mut(session_id).ok_or("Session not found")?;
        if session.pages.iter().any(|page| page.id == page_id) {
            session.active_page = Some(page_id);
            Ok(())
        } else {
            Err("Page not found".to_string())
        }
    }

    pub fn close_page(&self, session_id: &str, page_id: usize) -> Result<(), String> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions.get_mut(session_id).ok_or("Session not found")?;

        let pos = session
            .pages
            .iter()
            .position(|p| p.id == page_id)
            .ok_or("Page not found")?;

        session.pages.remove(pos);

        // If we removed the active page, update active_page
        if let Some(active) = session.active_page {
            if active == page_id {
                session.active_page = session.pages.first().map(|p| p.id);
            }
        }

        Ok(())
    }

    pub fn set_provider_config(&self, provider_config: ProviderConfig) -> Result<(), String> {
        {
            let mut agent_config = self.agent_config.lock().map_err(|e| e.to_string())?;
            agent_config.provider_config = provider_config.clone();
        }

        let sessions = self.sessions.lock().map_err(|e| e.to_string())?;
        for session in sessions.values() {
            for page in &session.pages {
                page.agent.set_provider_config(provider_config.clone())?;
            }
        }

        Ok(())
    }
}

impl SessionManager {
    // ---------------------------------------------------------------------
    // Worker model — Phase E1/E2/E4
    //
    // Workers are long-running ReAct agents with their own ActionPolicy and
    // AgentMemory. They live inside a session and share `observations` so
    // siblings can read what other workers have recently discovered.
    // ---------------------------------------------------------------------

    /// Spawn a new worker inside `session_id`. The worker takes ownership
    /// of either an explicitly pinned page or, if `pinned_page_id` is
    /// None, the session's currently-active page. Returns a `WorkerHandle`
    /// with a fresh `worker_id`.
    pub fn spawn_worker(&self, session_id: &str, spec: WorkerSpec) -> Result<WorkerHandle, String> {
        let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| "Session not found".to_string())?;

        let agent_config = self.agent_config.lock().map_err(|e| e.to_string())?.clone();
        let provider = create_provider(&agent_config.provider_config);
        let agent = Arc::new(ReActAgent::new(agent_config, provider));

        let handle = WorkerHandle::new(session_id.to_string(), spec, agent.clone());
        session
            .workers
            .insert(handle.worker_id.clone(), handle.clone());
        Ok(handle)
    }

    /// Return summaries of all workers in `session_id`, sorted by last
    /// update (most recently active first).
    pub fn list_workers(&self, session_id: &str) -> Result<Vec<WorkerSummary>, String> {
        let sessions = self.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| "Session not found".to_string())?;
        let mut out: Vec<WorkerSummary> =
            session.workers.values().map(|h| h.summary(None)).collect();
        out.sort_by_key(|w| std::cmp::Reverse(w.last_update_ms));
        Ok(out)
    }

    pub fn get_worker(&self, session_id: &str, worker_id: &str) -> Result<WorkerSnapshot, String> {
        let sessions = self.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| "Session not found".to_string())?;
        let handle = session
            .workers
            .get(worker_id)
            .ok_or_else(|| "Worker not found".to_string())?;
        Ok(handle.snapshot(None))
    }

    /// Update a worker's status (called from the agent loop / Tauri command
    /// after `start_agent_run` completes).
    pub fn set_worker_status(
        &self,
        session_id: &str,
        worker_id: &str,
        status: WorkerStatus,
    ) -> Result<(), String> {
        let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| "Session not found".to_string())?;
        let worker = session
            .workers
            .get_mut(worker_id)
            .ok_or_else(|| "Worker not found".to_string())?;
        worker.status = status;
        Ok(())
    }

    /// Send a `WorkerMessage` from one worker to another within the same
    /// session. The recipient pulls its inbox via `drain_inbox`.
    pub fn send_message(&self, session_id: &str, message: WorkerMessage) -> Result<(), String> {
        let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| "Session not found".to_string())?;
        if !session.workers.contains_key(&message.to) {
            return Err(format!("Recipient worker not found: {}", message.to));
        }
        session.inbox.push(message);
        Ok(())
    }

    /// Drain the entire session's worker inbox. Returns messages addressed
    /// to `worker_id` (or all if worker_id is None) and clears them.
    pub fn drain_inbox(
        &self,
        session_id: &str,
        worker_id: Option<&str>,
    ) -> Result<Vec<WorkerMessage>, String> {
        let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| "Session not found".to_string())?;
        let (kept, drained): (Vec<_>, Vec<_>) = session
            .inbox
            .drain(..)
            .partition(|m| worker_id.is_some_and(|w| w != m.to));
        // Re-stash the kept (= not for this worker) so future drain calls see them.
        session.inbox = kept;
        Ok(drained)
    }

    /// Read the latest N observations (Phase E2). Workers call this at the
    /// start of each iteration to fold sibling discoveries into their
    /// context.
    pub fn cross_worker_observations(
        &self,
        session_id: &str,
        n: usize,
    ) -> Result<Vec<serde_json::Value>, String> {
        let sessions = self.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| "Session not found".to_string())?;
        Ok(session
            .observations
            .latest_n(n)
            .into_iter()
            .filter_map(|e| serde_json::to_value(e).ok())
            .collect())
    }

    /// Record an observation about the session (Phase E2 hook). Workers
    /// call this after each `execute_tool`.
    pub fn record_observation(
        &self,
        session_id: &str,
        event: crate::agent::memory::AgentEvent,
    ) -> Result<(), String> {
        let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| "Session not found".to_string())?;
        session.observations.push(event);
        Ok(())
    }
}

#[derive(Clone)]
pub struct PageHandle {
    pub id: usize,
    pub runtime_id: String,
    pub agent: Arc<ReActAgent>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionInfo {
    pub id: String,
    pub created_at: u64,
    pub page_count: usize,
}

fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}
