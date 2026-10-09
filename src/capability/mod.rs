//! Versioned, bounded evidence shared by human and agent clients.
use crate::tools::{LinkInfo, PageSnapshot, TableInfo};
use serde::{Deserialize, Serialize};

pub mod tools;

pub const CAPABILITY_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    Unknown,
    Http,
    DesktopWebview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeCapabilities {
    pub schema_version: u16,
    pub runtime: RuntimeKind,
    pub javascript: bool,
    pub interaction: bool,
    pub scoped_targets: bool,
    pub native_url: bool,
    pub screenshots: bool,
    pub background_javascript: bool,
    pub enforcing_subresource_network: bool,
}
impl RuntimeCapabilities {
    pub fn unknown() -> Self {
        Self {
            schema_version: 1,
            runtime: RuntimeKind::Unknown,
            javascript: false,
            interaction: false,
            scoped_targets: false,
            native_url: false,
            screenshots: false,
            background_javascript: false,
            enforcing_subresource_network: false,
        }
    }
    pub fn http() -> Self {
        Self {
            runtime: RuntimeKind::Http,
            native_url: true,
            enforcing_subresource_network: true,
            ..Self::unknown()
        }
    }
    pub fn desktop() -> Self {
        Self {
            runtime: RuntimeKind::DesktopWebview,
            javascript: true,
            interaction: true,
            scoped_targets: true,
            native_url: true,
            ..Self::unknown()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocumentStamp {
    pub runtime_id: String,
    pub document_id: String,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedTarget {
    pub id: String,
    pub role: String,
    pub label: String,
    pub tag: String,
    pub disabled: bool,
    pub sensitive: bool,
    pub destination: Option<String>,
}

/// Safe concrete evidence for a human reviewing a pending grant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalContext {
    pub url: String,
    pub document: Option<DocumentStamp>,
    pub target: Option<ObservedTarget>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationLimits {
    pub max_text_bytes: usize,
    pub max_targets: usize,
    pub max_links: usize,
    pub max_tables: usize,
    pub max_rows: usize,
    pub max_cell_bytes: usize,
}
impl Default for ObservationLimits {
    fn default() -> Self {
        Self {
            max_text_bytes: 12000,
            max_targets: 80,
            max_links: 40,
            max_tables: 6,
            max_rows: 20,
            max_cell_bytes: 240,
        }
    }
}
impl ObservationLimits {
    /// Caller limits can narrow evidence but cannot expand host ceilings.
    pub fn bounded(self) -> Self {
        let ceiling = Self::default();
        Self {
            max_text_bytes: self.max_text_bytes.min(ceiling.max_text_bytes),
            max_targets: self.max_targets.min(ceiling.max_targets),
            max_links: self.max_links.min(ceiling.max_links),
            max_tables: self.max_tables.min(ceiling.max_tables),
            max_rows: self.max_rows.min(ceiling.max_rows),
            max_cell_bytes: self.max_cell_bytes.min(ceiling.max_cell_bytes),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageObservation {
    pub schema_version: u16,
    pub document: Option<DocumentStamp>,
    pub url: String,
    pub title: String,
    pub text: String,
    pub links: Vec<LinkInfo>,
    pub tables: Vec<TableInfo>,
    pub targets: Vec<ObservedTarget>,
    pub omissions: Vec<String>,
    pub capabilities: RuntimeCapabilities,
    pub collected_at_ms: u64,
}
impl PageObservation {
    pub fn from_snapshot(
        snapshot: PageSnapshot,
        capabilities: RuntimeCapabilities,
        limits: ObservationLimits,
    ) -> Self {
        let mut observation = Self {
            schema_version: 1,
            document: None,
            url: snapshot.url,
            title: snapshot.title,
            text: snapshot.text.unwrap_or_default(),
            links: snapshot.links,
            tables: snapshot.tables,
            targets: Vec::new(),
            omissions: vec![
                "Raw HTML, input values, frames and visual evidence are excluded".into(),
            ],
            capabilities,
            collected_at_ms: neuro_memory::now_millis(),
        };
        observation.apply_limits(limits);
        observation
    }
    /// Reject malformed authority identities instead of trimming them into aliases.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != CAPABILITY_SCHEMA_VERSION
            || self.capabilities.schema_version != CAPABILITY_SCHEMA_VERSION
            || self.url.len() > 4096
        {
            return Err("Unsupported or oversized observation authority".into());
        }
        if let Some(document) = &self.document {
            if document.runtime_id.is_empty()
                || document.runtime_id.len() > 200
                || document.document_id.is_empty()
                || document.document_id.len() > 200
            {
                return Err("Invalid document identity".into());
            }
        }
        let mut ids = std::collections::HashSet::new();
        for target in &self.targets {
            if target.id.is_empty()
                || target.id.len() > 200
                || !ids.insert(&target.id)
                || target
                    .destination
                    .as_ref()
                    .is_some_and(|url| url.len() > 4096)
            {
                return Err("Invalid or ambiguous observed target authority".into());
            }
        }
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > 64 * 1024 {
            return Err("Observation exceeds the host byte budget".into());
        }
        Ok(())
    }

    pub fn apply_limits(&mut self, limits: ObservationLimits) {
        let limits = limits.bounded();
        if truncate(&mut self.text, limits.max_text_bytes) {
            self.omissions.push("Page text truncated".into());
        }
        truncate(&mut self.title, 512);
        // Authority URLs remain exact. Dropping oversized collections below is
        // safer than truncating a URL into a different authority target.
        if self.targets.len() > limits.max_targets {
            self.omissions.push("Targets truncated".into());
        }
        self.targets.truncate(limits.max_targets);
        for target in &mut self.targets {
            truncate(&mut target.label, 240);
            truncate(&mut target.role, 80);
            truncate(&mut target.tag, 80);
            // Keep the full destination used by the policy decision.
        }
        if self.links.len() > limits.max_links {
            self.omissions.push("Links truncated".into());
        }
        self.links.truncate(limits.max_links);
        for link in &mut self.links {
            truncate(&mut link.text, 240);
        }
        if self.tables.len() > limits.max_tables {
            self.omissions.push("Tables truncated".into());
        }
        self.tables.truncate(limits.max_tables);
        for table in &mut self.tables {
            if table.rows.len() > limits.max_rows
                || table.headers.len() > 20
                || table.rows.iter().any(|r| r.len() > 20)
            {
                self.omissions.push("Table cells or rows truncated".into());
            }
            table.headers.truncate(20);
            table.rows.truncate(limits.max_rows);
            for cell in &mut table.headers {
                if truncate(cell, limits.max_cell_bytes) {
                    self.omissions.push("Table cell text truncated".into());
                }
            }
            for row in &mut table.rows {
                row.truncate(20);
                for cell in row {
                    if truncate(cell, limits.max_cell_bytes) {
                        self.omissions.push("Table cell text truncated".into());
                    }
                }
            }
        }
        self.omissions.sort();
        self.omissions.dedup();
        self.omissions.truncate(16);
        // Bound the entire envelope, including JSON escaping and all collections.
        // Never shorten document IDs, target IDs or authority URLs to fit a budget.
        const MAX_OBSERVATION_BYTES: usize = 64 * 1024;
        let size = |value: &Self| {
            serde_json::to_vec(value)
                .map(|bytes| bytes.len())
                .unwrap_or(usize::MAX)
        };
        if size(self) > MAX_OBSERVATION_BYTES {
            self.omissions
                .push("Total observation byte budget reached".into());
            while size(self) > MAX_OBSERVATION_BYTES {
                if self.links.pop().is_some() {
                    continue;
                }
                if self.tables.pop().is_some() {
                    continue;
                }
                if self.targets.pop().is_some() {
                    continue;
                }
                if !self.text.is_empty() {
                    self.text.clear();
                    continue;
                }
                break;
            }
        }
    }
}
fn truncate(value: &mut String, max: usize) -> bool {
    if value.len() <= max {
        return false;
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetAction {
    Click,
    Type,
    Submit,
    Scroll,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetCommand {
    pub document: DocumentStamp,
    pub target_id: String,
    pub action: TargetAction,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchState {
    NotDispatched,
    Acknowledged,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationState {
    NotRequested,
    Satisfied,
    Unsatisfied,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionReceipt {
    pub schema_version: u16,
    pub receipt_id: String,
    pub document: DocumentStamp,
    pub target_id: String,
    pub action: TargetAction,
    pub dispatch: DispatchState,
    pub page_ready: bool,
    pub verification: VerificationState,
    pub message: String,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{message}")]
pub struct TargetDispatchError {
    pub state: DispatchState,
    pub message: String,
}
impl TargetDispatchError {
    pub fn rejected(message: impl Into<String>) -> Self {
        Self {
            state: DispatchState::NotDispatched,
            message: message.into(),
        }
    }
    pub fn unknown(message: impl Into<String>) -> Self {
        Self {
            state: DispatchState::Unknown,
            message: message.into(),
        }
    }
}
