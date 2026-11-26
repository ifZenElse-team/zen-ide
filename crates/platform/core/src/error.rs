use std::fmt;

use serde::Serialize;

use crate::{CommandId, InvalidCommandId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CommandError {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Map<String, serde_json::Value>>,
}

impl CommandError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), code: None, data: None }
    }

    pub fn with_code(code: &str, message: impl Into<String>) -> Self {
        Self { message: message.into(), code: Some(code.to_owned()), data: None }
    }

    #[must_use]
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        if let serde_json::Value::Object(fields) = data {
            self.data = Some(fields);
        }
        self
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CommandError {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DispatchError {
    UnknownCommand { id: String },
    Unavailable { id: CommandId },
    InvalidParams { id: CommandId, message: String },
    Failed { id: CommandId, error: CommandError },
    InvalidOutput { id: CommandId, message: String },
}

impl fmt::Display for DispatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DispatchError::UnknownCommand { id } => write!(f, "unknown command `{id}`"),
            DispatchError::Unavailable { id } => write!(f, "command `{id}` is not available now"),
            DispatchError::InvalidParams { id, message } => write!(f, "invalid parameters for `{id}`: {message}"),
            DispatchError::Failed { id, error } => write!(f, "`{id}` failed: {error}"),
            DispatchError::InvalidOutput { id, message } => {
                write!(f, "`{id}` produced output that is not JSON: {message}")
            }
        }
    }
}

impl DispatchError {
    pub fn text(&self) -> String {
        match self {
            DispatchError::Failed { error, .. } => error.message.clone(),
            other => other.to_string(),
        }
    }
}

impl std::error::Error for DispatchError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryError {
    InvalidId(InvalidCommandId),
    Duplicate(CommandId),
    UnknownTarget(CommandId),
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegistryError::InvalidId(error) => error.fmt(f),
            RegistryError::Duplicate(id) => write!(f, "command id `{id}` is already registered"),
            RegistryError::UnknownTarget(id) => write!(f, "alias target `{id}` is not a registered command"),
        }
    }
}

impl std::error::Error for RegistryError {}

impl From<InvalidCommandId> for RegistryError {
    fn from(error: InvalidCommandId) -> Self {
        RegistryError::InvalidId(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_code_is_serialised_only_when_set() {
        let plain = serde_json::to_value(CommandError::new("no")).unwrap();
        assert_eq!(plain, serde_json::json!({ "message": "no" }));
        let coded = serde_json::to_value(CommandError::with_code("debuggerMissing", "no dlv")).unwrap();
        assert_eq!(coded, serde_json::json!({ "message": "no dlv", "code": "debuggerMissing" }));
        let data = CommandError::with_code("not_unique", "three").with_data(serde_json::json!({ "count": 3 }));
        let data = serde_json::to_value(data).unwrap();
        assert_eq!(data, serde_json::json!({ "message": "three", "code": "not_unique", "data": { "count": 3 } }));
    }
}
