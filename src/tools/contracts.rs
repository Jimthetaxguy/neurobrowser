use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolAction {
    Read,
    Navigate,
    Wait,
    Scroll,
    Click,
    Type,
    Submit,
    Keypress,
    Screenshot,
    Back,
    Forward,
    Reload,
    ClosePage,
    Download,
    Upload,
    Message,
    Auth,
    Purchase,
    Destructive,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRisk {
    pub action: ToolAction,
    pub externally_visible: bool,
    pub sensitive: bool,
}

impl ToolRisk {
    pub fn new(action: ToolAction) -> Self {
        Self {
            action,
            externally_visible: false,
            sensitive: false,
        }
    }

    pub fn externally_visible(mut self, value: bool) -> Self {
        self.externally_visible = value;
        self
    }

    pub fn sensitive(mut self, value: bool) -> Self {
        self.sensitive = value;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolArgumentDefinition {
    pub name: String,
    pub required: bool,
    pub description: String,
    pub sensitive: bool,
}

impl ToolArgumentDefinition {
    pub fn required(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            required: true,
            description: description.to_string(),
            sensitive: false,
        }
    }

    pub fn optional(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            required: false,
            description: description.to_string(),
            sensitive: false,
        }
    }

    pub fn sensitive(mut self, value: bool) -> Self {
        self.sensitive = value;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub arguments: Vec<ToolArgumentDefinition>,
    pub risk: ToolRisk,
}

impl ToolDefinition {
    pub fn new(name: &str, description: &str, risk: ToolRisk) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            arguments: Vec::new(),
            risk,
        }
    }

    pub fn with_arguments(mut self, arguments: Vec<ToolArgumentDefinition>) -> Self {
        self.arguments = arguments;
        self
    }
}
