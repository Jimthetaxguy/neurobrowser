use crate::agent::{AgentConfig, ReActAgent};
use crate::providers::{create_provider, ProviderConfig};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct SessionManager {
    sessions: Mutex<HashMap<String, SessionState>>,
    agent_config: Mutex<AgentConfig>,
    page_counter: Mutex<usize>,
    operations: Mutex<HashMap<usize, Arc<tokio::sync::Mutex<()>>>>,
}

struct SessionState {
    pages: Vec<PageHandle>,
}

impl SessionManager {
    pub fn new(agent_config: AgentConfig) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            agent_config: Mutex::new(agent_config),
            page_counter: Mutex::new(0),
            operations: Mutex::new(HashMap::new()),
        }
    }

    pub fn create_session(&self) -> String {
        let id = uuid_v4();

        let mut sessions = self.sessions.lock().unwrap();
        sessions.insert(id.clone(), SessionState { pages: Vec::new() });

        id
    }

    pub fn create_page(&self, session_id: &str) -> Result<PageHandle, String> {
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
        self.operations
            .lock()
            .map_err(|e| e.to_string())?
            .insert(page_id, Arc::new(tokio::sync::Mutex::new(())));

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

    pub fn close_page(&self, session_id: &str, page_id: usize) -> Result<(), String> {
        let mut sessions = self.sessions.lock().unwrap();
        let session = sessions.get_mut(session_id).ok_or("Session not found")?;

        let pos = session
            .pages
            .iter()
            .position(|p| p.id == page_id)
            .ok_or("Page not found")?;

        session.pages.remove(pos);
        self.operations
            .lock()
            .map_err(|e| e.to_string())?
            .remove(&page_id);

        Ok(())
    }

    /// Serialize host and agent operations on a page. This is an execution lock,
    /// not persisted session storage; ownership is checked before acquiring it.
    pub async fn lock_page_operation(
        &self,
        session_id: &str,
        page_id: usize,
    ) -> Result<tokio::sync::OwnedMutexGuard<()>, String> {
        self.get_page(session_id, page_id)?;
        let operation = self
            .operations
            .lock()
            .map_err(|e| e.to_string())?
            .get(&page_id)
            .cloned()
            .ok_or("Page is closed")?;
        let guard = operation.lock_owned().await;
        self.get_page(session_id, page_id)?;
        Ok(guard)
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

#[derive(Clone)]
pub struct PageHandle {
    pub id: usize,
    pub runtime_id: String,
    pub agent: Arc<ReActAgent>,
}

fn uuid_v4() -> String {
    uuid::Uuid::new_v4().to_string()
}
