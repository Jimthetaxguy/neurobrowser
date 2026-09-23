use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
}

impl ToolArgumentDefinition {
    pub fn required(name: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            required: true,
            description: description.to_string(),
        }
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

    /// Required arguments that `arguments` omits or sets to `""`, in
    /// declaration order.
    ///
    /// Whitespace is a value (a space key, typed spaces). The tool decides
    /// whether it is valid.
    pub(crate) fn missing_required_arguments(
        &self,
        arguments: &HashMap<String, String>,
    ) -> Vec<&str> {
        self.arguments
            .iter()
            .filter(|argument| argument.required)
            .filter(|argument| arguments.get(&argument.name).is_none_or(String::is_empty))
            .map(|argument| argument.name.as_str())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{ToolAction, ToolArgumentDefinition, ToolDefinition, ToolRisk};
    use std::collections::HashMap;

    fn args(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn missing_required_arguments_flags_absent_and_empty_values_only() {
        let definition = ToolDefinition::new("type", "Type", ToolRisk::new(ToolAction::Type))
            .with_arguments(vec![
                ToolArgumentDefinition::required("selector", "CSS selector"),
                ToolArgumentDefinition::required("text", "Text"),
                ToolArgumentDefinition {
                    name: "delay".to_string(),
                    required: false,
                    description: "Optional".to_string(),
                },
            ]);

        assert_eq!(
            definition.missing_required_arguments(&args(&[])),
            vec!["selector", "text"]
        );
        assert_eq!(
            definition.missing_required_arguments(&args(&[("selector", "#q"), ("text", "")])),
            vec!["text"]
        );
        // A space is a value; the optional `delay` may be omitted.
        assert!(definition
            .missing_required_arguments(&args(&[("selector", "#q"), ("text", " ")]))
            .is_empty());
    }
}
