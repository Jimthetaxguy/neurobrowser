use crate::agent::{AgentConfig, ReActAgent};
use crate::browser::{BrowserEngine, PageConfig};
use crate::providers::create_provider;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub struct SessionManager {
    sessions: Mutex<HashMap<String, SessionState>>,
    browser_config: PageConfig,
    agent_config: AgentConfig,
    page_counter: Mutex<usize>,
}

struct SessionState {
    id: String,
    created_at: u64,
    pages: Vec<PageHandle>,
    #[allow(dead_code)]
    active_page: Option<usize>,
}

impl SessionManager {
    pub fn new(browser_config: PageConfig, agent_config: AgentConfig) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            browser_config,
            agent_config,
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
        let mut sessions = self.sessions.lock().unwrap();

        let session = sessions.get_mut(session_id).ok_or("Session not found")?;

        let browser = Arc::new(BrowserEngine::new(self.browser_config.clone()));

        let provider = create_provider(&self.agent_config.provider_config);
        let agent = Arc::new(ReActAgent::new(self.agent_config.clone(), provider));

        let mut counter = self.page_counter.lock().unwrap();
        let page_id = *counter;
        *counter += 1;

        let handle = PageHandle {
            id: page_id,
            browser,
            agent,
        };

        session.pages.push(handle.clone());

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
}

#[derive(Clone)]
pub struct PageHandle {
    pub id: usize,
    pub browser: Arc<BrowserEngine>,
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
