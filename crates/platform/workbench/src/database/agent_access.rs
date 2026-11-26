use std::borrow::Cow;
use std::collections::HashSet;
use std::path::PathBuf;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::de::{DeserializeOwned, Error as _};
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_db::types::AgentAccess;
use zen_i18n::t;

use super::safety::{AGENT_NOT_ALLOWED, agent_access};
use super::sources::files;
use super::{Database, WORKSPACE};
use crate::workspaces::Workspaces;

pub const OPEN_TO_AGENTS: &[&str] = &[
    "db.dataSource.list",
    "db.dataSource.get",
    "db.dataSource.add",
    "db.dataSource.update",
    "db.dataSource.test",
    "db.connectionType.list",
    "db.connectionType.create",
    "db.connectionType.update",
    "db.connectionType.delete",
    "db.connection.connect",
    "db.connection.disconnect",
    "db.connection.disconnectAll",
    "db.connection.invalidate",
    "db.connection.cancel",
    "db.connection.status",
    "db.connection.setSecret",
    "db.connection.readOnly",
    "db.connection.clientCommand",
    "db.connection.openTerminal",
    "db.driver.list",
    "db.driver.log",
    "db.detect.list",
    "db.detect.accept",
    "db.detect.dismiss",
    "db.import.dbeaver",
    "db.import.jetbrains",
    "db.databaseFile.create",
    "db.settings.get",
    "db.settings.update",
    "db.copy.settings",
    "db.export.settings",
    "db.csvImport.settings",
    "db.history.settings",
    "db.history.clear",
    "db.ssh.prompts",
    "db.ssh.answer",
    "db.navigator.options",
    "db.navigator.collapse",
    "db.result.toggleValuePanel",
    "db.script.split",
    "db.sql.format",
];

pub fn register<C>(registry: &mut Registry, command: C) -> Result<(), RegistryError>
where
    C: Command,
    C::Params: DeserializeOwned + JsonSchema,
{
    if OPEN_TO_AGENTS.contains(&C::ID) {
        registry.register(command)
    } else {
        registry.register(ClosedToAgents(command))
    }
}

pub struct Given<P> {
    json: Value,
    params: P,
}

impl<'de, P: DeserializeOwned> Deserialize<'de> for Given<P> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let json = Value::deserialize(deserializer)?;
        let params = serde_json::from_value(json.clone()).map_err(D::Error::custom)?;
        Ok(Given { json, params })
    }
}

impl<P: JsonSchema> JsonSchema for Given<P> {
    fn inline_schema() -> bool {
        P::inline_schema()
    }

    fn schema_name() -> Cow<'static, str> {
        P::schema_name()
    }

    fn schema_id() -> Cow<'static, str> {
        P::schema_id()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        P::json_schema(generator)
    }
}

pub struct ClosedToAgents<C>(pub C);

impl<C> Command for ClosedToAgents<C>
where
    C: Command,
    C::Params: DeserializeOwned + JsonSchema,
{
    type Params = Given<C::Params>;
    type Output = Value;
    const ID: &'static str = C::ID;
    const TITLE: &'static str = C::TITLE;
    const DESCRIPTION: &'static str = C::DESCRIPTION;

    fn is_available(&self, session: &Session) -> bool {
        self.0.is_available(session)
    }

    fn run(&self, session: &mut Session, given: Given<C::Params>) -> Result<Value, CommandError> {
        let closed = if zen_core::macros::by_agent(session) { closed(session) } else { HashSet::new() };
        let takes = taken::<C::Params>();
        if let Some(name) = named(session, (C::ID, &takes), &given.json).into_iter().find(|id| closed.contains(id)) {
            return Err(refusal(session, &name));
        }
        let answer = self.0.run(session, given.params)?;
        let mut answer = serde_json::to_value(answer).map_err(|error| CommandError::new(error.to_string()))?;
        if !closed.is_empty()
            && let Some(transfers) = answer.get_mut("transfers").and_then(Value::as_array_mut)
        {
            transfers.retain(|transfer| {
                ["source", "target"].iter().all(|side| transfer[side].as_str().is_none_or(|id| !closed.contains(id)))
            });
        }
        if !closed.is_empty()
            && let Some(name) = scrub(&mut answer, &closed)
        {
            return Err(refusal(session, &name));
        }
        Ok(answer)
    }
}

pub fn closed(session: &Session) -> HashSet<String> {
    files(session)
        .entries()
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| agent_access(session, entry) == AgentAccess::None)
        .map(|entry| entry.source.id)
        .collect()
}

pub fn hidden_paths(session: &Session) -> Vec<PathBuf> {
    let closed = closed(session);
    let Some(data) = session.state::<Workspaces>().and_then(Workspaces::data_dir).map(PathBuf::from) else {
        return Vec::new();
    };
    if closed.is_empty() {
        return Vec::new();
    }
    let databases = data.join("databases");
    let mut paths = vec![
        data.join("caches").join("databases"),
        databases.join(zen_db::history::JOURNAL),
        databases.join("recovery-*"),
        databases.join("filters.json"),
    ];
    let folders = |folder: PathBuf| {
        std::fs::read_dir(folder).into_iter().flatten().filter_map(Result::ok).map(|entry| entry.path())
    };
    paths.extend(folders(databases.clone()).filter(|path| path.is_dir()).map(|project| project.join("navigator.json")));
    for project in folders(data.join("consoles")) {
        paths.extend(closed.iter().map(|id| project.join(id)));
    }
    let entries = files(session).entries().unwrap_or_default();
    for entry in entries.iter().filter(|entry| closed.contains(&entry.source.id)) {
        let name = super::compare::file_part(&entry.source.name);
        paths.push(databases.join("snapshots").join(format!("{name}-*")));
    }
    paths
}

pub fn hidden_tab(session: &Session, tab: &crate::workspaces::InnerTab) -> bool {
    let closed = closed(session);
    if closed.is_empty() {
        return false;
    }
    let connection = tab
        .data
        .get("connection")
        .and_then(text)
        .or_else(|| tab.path.as_deref().and_then(|path| super::consoles::connection_of(session, path)));
    connection.is_some_and(|connection| closed.contains(&connection))
}

fn refusal(session: &Session, id: &str) -> CommandError {
    let entry = super::sources::find(session, id).ok();
    let name = entry.as_ref().map_or(id, |entry| entry.source.name.as_str());
    let kind = entry.as_ref().map(|entry| super::safety::type_of(session, entry).name).unwrap_or_default();
    CommandError::with_code(AGENT_NOT_ALLOWED, t!("database-error-agent-closed", name = name, kind = kind.as_str()))
}

fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::trim).filter(|text| !text.is_empty()).map(str::to_owned)
}

fn taken<P: JsonSchema>() -> HashSet<String> {
    let schema = SchemaGenerator::default().into_root_schema_for::<P>();
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|properties| properties.keys().cloned().collect())
        .unwrap_or_default()
}

fn named(session: &Session, (id, takes): (&str, &HashSet<String>), json: &Value) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    found.extend(json.get("connection").and_then(text));
    if matches!(id, "db.dataSource.remove" | "db.dataSource.removal" | "db.dataSource.duplicate") {
        found.extend(json.get("id").and_then(text));
    }
    if let Some(list) = json.get("connections").and_then(Value::as_array) {
        found.extend(list.iter().filter_map(text));
    }
    for side in ["source", "target"] {
        found.extend(json.get(side).and_then(|side| side.get("connection")).and_then(text));
    }
    if id.starts_with("db.transfer.") {
        for side in ["source", "target"] {
            found.extend(json.get(side).and_then(text));
        }
        let engine = session.state::<Database>().and_then(Database::started);
        if let Some(state) = json.get("id").and_then(Value::as_u64).and_then(|op| engine?.transfer_state(op)) {
            found.extend([state.source, state.target]);
        }
    }
    let console = match json.get("path") {
        Some(Value::String(path)) => Some(PathBuf::from(path)),
        None | Some(Value::Null) if takes.contains("path") && about_console(id) => {
            super::consoles::console_path(session, None).ok()
        }
        _ => None,
    };
    if let Some(path) = console {
        found.extend(super::consoles::connection_of(session, &path));
    }
    let workspaces = session.state::<Workspaces>();
    let tab = match json.get("tab") {
        Some(Value::String(tab)) => workspaces.and_then(|w| w.tabs(WORKSPACE).iter().find(|known| known.id == *tab)),
        None | Some(Value::Null) if takes.contains("tab") && about_tab(id) => {
            workspaces.and_then(|w| w.active_tab(WORKSPACE))
        }
        _ => None,
    };
    found.extend(tab.and_then(|tab| tab.data.get("connection")).and_then(text));
    if id.starts_with("db.result.") && (takes.contains("id") || takes.contains("selection")) {
        let result = json
            .get("id")
            .or_else(|| json.get("selection").and_then(|selection| selection.get("id")))
            .and_then(Value::as_u64)
            .or_else(|| super::filters::active_result(session));
        let engine = session.state::<Database>().and_then(Database::started);
        found.extend(result.and_then(|result| engine?.results().get(result)).map(|set| set.connection().to_owned()));
    }
    found
}

fn about_console(id: &str) -> bool {
    ["db.console.", "db.sql.", "db.script.", "db.transaction."].iter().any(|prefix| id.starts_with(prefix))
}

fn about_tab(id: &str) -> bool {
    ["db.object.", "db.structure.", "db.plan."].iter().any(|prefix| id.starts_with(prefix))
}

fn scrub(answer: &mut Value, closed: &HashSet<String>) -> Option<String> {
    match answer {
        Value::Object(fields) => {
            let mut refers = None;
            for (key, value) in fields.iter_mut() {
                let here = match value {
                    Value::String(id) if key == "connection" && closed.contains(id.as_str()) => Some(id.clone()),
                    _ => scrub(value, closed),
                };
                refers = refers.or(here);
            }
            refers
        }
        Value::Array(items) => {
            items.retain_mut(|item| scrub(item, closed).is_none());
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
