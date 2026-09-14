use crate::agent::worker::{WorkerSnapshot, WorkerSummary};
use crate::agent::{AgentConfig, ReActAgent};
use crate::browser::PageConfig;
use crate::providers::{create_provider, ProviderConfig};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct SessionManager {
    sessions: Mutex<HashMap<String, SessionState>>,
    agent_config: Mutex<AgentConfig>,
    page_counter: Mutex<usize>,
}

struct SessionState {
    id: String,
    created_at: u64,
    pages: Vec<PageHandle>,
    active_page: Option<usize>,
}

impl SessionManager {
    pub fn new(_browser_config: PageConfig, agent_config: AgentConfig) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
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

    /// Empty-map reader. Worker spawn/inbox is unwired; Tauri still compiles against this.
    pub fn list_workers(&self, session_id: &str) -> Result<Vec<WorkerSummary>, String> {
        let sessions = self.sessions.lock().map_err(|e| e.to_string())?;
        if !sessions.contains_key(session_id) {
            return Err("Session not found".to_string());
        }
        Ok(Vec::new())
    }

    /// Empty-map reader. Worker spawn/inbox is unwired; Tauri still compiles against this.
    pub fn get_worker(&self, session_id: &str, _worker_id: &str) -> Result<WorkerSnapshot, String> {
        let sessions = self.sessions.lock().map_err(|e| e.to_string())?;
        if !sessions.contains_key(session_id) {
            return Err("Session not found".to_string());
        }
        Err("Worker not found".to_string())
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
