use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::registry::{read_json, write_json};

pub const DEVELOPMENT: &str = "dev";
pub const TEST: &str = "test";
pub const PRODUCTION: &str = "prod";

pub const COLORS: &[&str] = &["none", "green", "red", "orange", "blue", "violet"];

/// What agents may run on the connections of a type (over MCP, ADR-0033).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AgentAccess {
    /// Everything a person may run; where the type asks first, the agent confirms with the token of the refusal.
    #[default]
    Full,
    /// Reads only, in a read-only session; every change is refused with the reason and left for a person.
    ReadOnly,
    /// Nothing: every statement is refused.
    None,
}

impl AgentAccess {
    pub fn at_most(self, other: AgentAccess) -> bool {
        let rank = |access: AgentAccess| match access {
            AgentAccess::None => 0,
            AgentAccess::ReadOnly => 1,
            AgentAccess::Full => 2,
        };
        rank(self) <= rank(other)
    }
}

/// A connection type: the safety defaults of the connections that have it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionType {
    /// Its stable id (`dev`, `test`, `prod`, or the user's own).
    pub id: String,
    /// Its name as shown.
    pub name: String,
    /// One of the theme's colours: `none`, `green`, `red`, `orange`, `blue` or `violet`.
    pub color: String,
    /// Whether statements commit on their own.
    pub auto_commit: bool,
    /// Whether data-changing statements ask before they run.
    pub confirm_execute: bool,
    /// Whether saving edited data asks first.
    pub confirm_data_change: bool,
    /// Whether a data-changing statement switches to manual commit.
    pub smart_commit: bool,
    /// Whether a commit or rollback returns to auto-commit.
    pub auto_close_transactions: bool,
    /// Seconds after which an idle transaction is ended; `None` never.
    pub end_idle_transactions_after: Option<u64>,
    /// Seconds after which an idle connection is closed; `None` never.
    pub close_idle_after: Option<u64>,
    /// The server-side statement timeout in seconds set at connect; `None` for none.
    #[serde(default)]
    pub statement_timeout: Option<u64>,
    /// Whether its connections are read-only: sessions read-only on the server, changes refused.
    #[serde(default)]
    pub read_only: bool,
    /// What agents may run; absent in a file written before it existed: read only where the type confirms SQL execution, else everything.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agents: Option<AgentAccess>,
    /// Whether it is one of the three built-in types (never stored).
    #[serde(default, skip_deserializing)]
    pub built_in: bool,
}

impl ConnectionType {
    pub fn built_in() -> Vec<ConnectionType> {
        let base = |id: &str, name: &str, color: &str| ConnectionType {
            id: id.to_owned(),
            name: name.to_owned(),
            color: color.to_owned(),
            auto_commit: true,
            confirm_execute: false,
            confirm_data_change: false,
            smart_commit: false,
            auto_close_transactions: true,
            end_idle_transactions_after: None,
            close_idle_after: None,
            statement_timeout: None,
            read_only: false,
            agents: Some(AgentAccess::Full),
            built_in: true,
        };
        vec![
            ConnectionType {
                end_idle_transactions_after: Some(1800),
                close_idle_after: Some(14_400),
                ..base(DEVELOPMENT, "Development", "none")
            },
            ConnectionType {
                confirm_data_change: true,
                end_idle_transactions_after: Some(900),
                close_idle_after: Some(7200),
                ..base(TEST, "Test", "green")
            },
            ConnectionType {
                auto_commit: false,
                confirm_execute: true,
                confirm_data_change: true,
                auto_close_transactions: false,
                end_idle_transactions_after: Some(600),
                close_idle_after: Some(3600),
                statement_timeout: Some(300),
                agents: Some(AgentAccess::ReadOnly),
                ..base(PRODUCTION, "Production", "red")
            },
        ]
    }
}

impl ConnectionType {
    pub fn agent_access(&self) -> AgentAccess {
        self.agents.unwrap_or(if self.confirm_execute { AgentAccess::ReadOnly } else { AgentAccess::Full })
    }

    #[must_use]
    pub fn with_overrides(&self, overrides: &crate::registry::TypeOverrides) -> ConnectionType {
        ConnectionType {
            auto_commit: overrides.auto_commit.unwrap_or(self.auto_commit),
            confirm_execute: overrides.confirm_execute.unwrap_or(self.confirm_execute),
            confirm_data_change: overrides.confirm_data_change.unwrap_or(self.confirm_data_change),
            agents: Some(overrides.agents.unwrap_or_else(|| self.agent_access())),
            ..self.clone()
        }
    }

    pub fn looser_than(&self, other: &ConnectionType) -> bool {
        (other.read_only && !self.read_only)
            || (other.confirm_execute && !self.confirm_execute)
            || (other.confirm_data_change && !self.confirm_data_change)
            || !self.agent_access().at_most(other.agent_access())
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TypesFile {
    #[serde(default = "version")]
    version: u32,
    #[serde(default)]
    types: Vec<ConnectionType>,
    #[serde(flatten)]
    extra: serde_json::Map<String, serde_json::Value>,
}

fn version() -> u32 {
    1
}

#[derive(Clone, Debug)]
pub struct ConnectionTypes {
    types: Vec<ConnectionType>,
    extra: serde_json::Map<String, serde_json::Value>,
}

impl ConnectionTypes {
    pub fn load(file: &Path) -> Self {
        let stored: TypesFile = read_json(file).ok().flatten().unwrap_or_default();
        let mut types = ConnectionType::built_in();
        for kept in stored.types {
            match types.iter_mut().find(|known| known.id == kept.id) {
                Some(known) => *known = ConnectionType { built_in: true, ..kept },
                None => types.push(kept),
            }
        }
        Self { types, extra: stored.extra }
    }

    pub fn save(&self, file: &Path) -> std::io::Result<()> {
        let defaults = ConnectionType::built_in();
        let types = self.types.iter().filter(|kind| !defaults.contains(kind)).cloned().collect();
        write_json(file, &TypesFile { version: 1, types, extra: self.extra.clone() })
    }

    pub fn all(&self) -> &[ConnectionType] {
        &self.types
    }

    pub fn get(&self, id: &str) -> Option<&ConnectionType> {
        self.types.iter().find(|kind| kind.id == id)
    }

    pub fn resolve(&self, id: &str) -> ConnectionType {
        self.get(id).or_else(|| self.get(PRODUCTION)).cloned().unwrap_or_else(|| ConnectionType::built_in().remove(2))
    }

    pub fn put(&mut self, kind: ConnectionType) {
        match self.types.iter_mut().find(|known| known.id == kind.id) {
            Some(known) => {
                let built_in = known.built_in;
                *known = ConnectionType { built_in, ..kind };
            }
            None => self.types.push(ConnectionType { built_in: false, ..kind }),
        }
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.types.len();
        self.types.retain(|kind| kind.built_in || kind.id != id);
        self.types.len() != before
    }
}
