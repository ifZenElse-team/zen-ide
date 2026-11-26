use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use schemars::JsonSchema;
use serde::Serialize;
use zen_core::{CommandError, Session};
use zen_i18n::t;
use zen_syntax::{IndexStateDeclaration, ToolRow, ToolSection, ToolWindowDeclaration, localized};

use super::{
    Done, Key, LIST_TIMEOUT, Listing, RowInfo, RowsParams, ToolState, Values, WindowKey, capture, collect, filled_in,
    folders, program, settle, snapshot, wanted, window_of,
};

pub const EVERY: Duration = Duration::from_secs(30);

pub const BACKOFF: Duration = Duration::from_secs(120);

/// What the cluster says of one node of the index.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClusterRow {
    /// `ok`, `progressing`, `error` or `unknown`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// Its values, one per column of `cluster.columns`.
    pub columns: Vec<String>,
}

/// A column of the cluster state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StateColumn {
    /// Its id, as `${row.<id>}` names it in an action.
    pub id: String,
    /// Its title; empty for a column only actions use.
    pub title: String,
}

/// Where the cluster state of an index section came from, and why it is missing.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClusterInfo {
    /// "State from <context>", naming the values the state was listed under; empty before a listing.
    pub from: String,
    /// The values it was listed under (`kubernetes.context`).
    pub values: BTreeMap<String, String>,
    /// The columns of each node's `cluster`.
    pub columns: Vec<StateColumn>,
    /// Why the state, or part of it, could not be listed (no cluster, a resource type the cluster lacks).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
    /// Whether a listing still runs.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub listing: bool,
    /// After a failure: how long the window waits before it lists again by itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after_ms: Option<u64>,
}

fn valid_values(session: &Session, window: &WindowKey) -> Option<Values> {
    let choices = session.state::<ToolState>()?.choices.get(window)?;
    let values: Option<Values> =
        choices.iter().map(|(id, chosen)| Some((id.clone(), chosen.value.clone().filter(|_| chosen.valid)?))).collect();
    values.filter(|values| !values.is_empty())
}

fn expected(session: &Session, window: &ToolWindowDeclaration) -> Option<Values> {
    window
        .window_values
        .iter()
        .map(|value| {
            let known = valid_values(session, &(value.plugin.clone(), value.window.clone()))?;
            Some((value.name(), known.get(&value.choice)?.clone()))
        })
        .collect()
}

fn clear_if_stale(session: &mut Session, key: &Key, window: &ToolWindowDeclaration) {
    let Some(expected) = expected(session, window) else { return };
    let Some(listed) = session.state_mut::<ToolState>().sections.get_mut(key) else { return };
    if listed.listed && listed.pending.is_none() && listed.values != expected {
        listed.rows.clear();
        listed.values.clear();
        listed.notes.clear();
        listed.error = None;
        listed.failed_at = None;
        listed.listed = false;
    }
}

struct Other {
    name: String,
    choice: String,
    known: Option<Values>,
    wanted: Values,
    plugin: Option<Arc<dyn zen_syntax::TextFeatures>>,
    choices: Vec<zen_syntax::ToolChoice>,
    missing: Option<String>,
}

fn others(session: &mut Session, root: &Path, window: &ToolWindowDeclaration) -> Vec<Other> {
    window
        .window_values
        .iter()
        .map(|value| {
            let key = (value.plugin.clone(), value.window.clone());
            let (plugin, choices, missing) = match window_of(&value.plugin, &value.window) {
                Ok((plugin, declared)) => (Some(plugin), declared.choices, None),
                Err(_) => (None, Vec::new(), Some(t!("workbench-index-state-needs", plugin = value.plugin.as_str()))),
            };
            let known = valid_values(session, &key);
            let wanted = if missing.is_none() { wanted(session, root, &key) } else { Values::new() };
            Other { name: value.name(), choice: value.choice.clone(), known, wanted, plugin, choices, missing }
        })
        .collect()
}

fn resolve(others: Vec<Other>, root: &Path, folders: &[std::path::PathBuf]) -> Result<Values, String> {
    let mut values = Values::new();
    for other in others {
        if let Some(missing) = other.missing {
            return Err(missing);
        }
        let known = match other.known {
            Some(known) => known,
            None => {
                let plugin = other.plugin.ok_or_else(String::new)?;
                let (_, values, ready) = snapshot(plugin.as_ref(), &other.choices, &other.wanted, root, folders);
                ready?;
                values
            }
        };
        let value = known
            .get(&other.choice)
            .cloned()
            .ok_or_else(|| t!("workbench-tool-choose", choice = other.choice.as_str()))?;
        values.insert(other.name, value);
    }
    Ok(values)
}

fn list(
    session: &mut Session,
    key: &Key,
    window: &ToolWindowDeclaration,
    section: &ToolSection,
    declared: &IndexStateDeclaration,
    plugin: Arc<dyn zen_syntax::TextFeatures>,
    root: &Path,
) {
    let folders = folders(session);
    let others = others(session, root, window);
    let listing: Listing = Arc::new(Mutex::new(None));
    let state = session.state_mut::<ToolState>().sections.entry(key.clone()).or_default();
    if state.pending.is_some() {
        return;
    }
    state.listed = true;
    state.pending = Some(Arc::clone(&listing));
    let (lists, name, root) =
        (declared.lists.clone(), format!("{}{}", zen_syntax::STATE_SECTION_PREFIX, section.id), root.to_path_buf());
    std::thread::spawn(move || {
        let done = match resolve(others, &root, &folders) {
            Err(error) => Done { rows: Err(error), choices: Vec::new(), values: Values::new(), notes: Vec::new() },
            Ok(values) => {
                let mut rows: Vec<ToolRow> = Vec::new();
                let mut notes = Vec::new();
                for command in &lists {
                    let listed = filled_in(&command.args, None, &[], &values).and_then(|args| {
                        let found = program(plugin.as_ref(), &command.program, &folders)?;
                        capture(&found, &args, &root, &folders, LIST_TIMEOUT)
                    });
                    match listed {
                        Ok(output) => {
                            for row in plugin.tool_rows(&name, &output).unwrap_or_default() {
                                if !rows.iter().any(|known| known.id == row.id) {
                                    rows.push(row);
                                }
                            }
                        }
                        Err(error) => notes.push(error),
                    }
                }
                if notes.len() == lists.len() && !lists.is_empty() {
                    let error = notes.join("\n");
                    Done { rows: Err(error), choices: Vec::new(), values, notes: Vec::new() }
                } else {
                    Done { rows: Ok(rows), choices: Vec::new(), values, notes }
                }
            }
        };
        if let Ok(mut slot) = listing.lock() {
            *slot = Some(done);
        }
    });
}

pub(super) fn listed(
    session: &mut Session,
    params: &RowsParams,
    plugin: Arc<dyn zen_syntax::TextFeatures>,
    window: &ToolWindowDeclaration,
    section: &ToolSection,
    declared: &IndexStateDeclaration,
) -> (ClusterInfo, BTreeMap<String, ToolRow>) {
    let language = zen_i18n::language();
    let columns = declared
        .columns
        .iter()
        .map(|id| StateColumn {
            id: id.clone(),
            title: declared.column_titles.get(id).map(|titles| localized(titles, &language)).unwrap_or_default(),
        })
        .collect();
    let mut info = ClusterInfo { columns, ..ClusterInfo::default() };
    let root = match crate::run::project_root(session) {
        Ok(root) if crate::trust::trusted(session, &root) => root,
        _ => {
            info.errors.push(t!("workbench-index-state-untrusted"));
            return (info, BTreeMap::new());
        }
    };
    let key = (params.plugin.clone(), params.window.clone(), params.section.clone());
    let busy = settle(session, &key);
    clear_if_stale(session, &key, window);
    let known = session.state::<ToolState>().and_then(|state| state.sections.get(&key));
    let waiting = known.and_then(|listed| listed.failed_at).is_some_and(|at| at.elapsed() < BACKOFF);
    let first = known.is_none_or(|listed| !listed.listed);
    if !busy && (first || (params.refresh && (params.force || !waiting))) {
        list(session, &key, window, section, declared, plugin, &root);
    }
    collect(session, &key, Duration::from_millis(params.wait_ms.unwrap_or(0).min(20_000)));
    clear_if_stale(session, &key, window);
    let Some(listed) = session.state::<ToolState>().and_then(|state| state.sections.get(&key)) else {
        return (info, BTreeMap::new());
    };
    if !listed.values.is_empty() {
        let context = listed.values.values().cloned().collect::<Vec<_>>().join(", ");
        info.from = t!("workbench-index-state-from", context = context.as_str());
    }
    info.values = listed.values.clone();
    info.errors.extend(listed.error.iter().cloned());
    info.errors.extend(listed.notes.iter().cloned());
    info.listing = listed.pending.is_some();
    info.retry_after_ms =
        listed.failed_at.map(|at| u64::try_from(BACKOFF.saturating_sub(at.elapsed()).as_millis()).unwrap_or(u64::MAX));
    let rows = listed.rows.iter().map(|row| (row.id.clone(), row.clone())).collect();
    (info, rows)
}

pub(super) fn attach(rows: &mut [RowInfo], state: &BTreeMap<String, ToolRow>) {
    for row in rows {
        if let Some(found) = state.get(&row.id) {
            row.cluster = Some(ClusterRow { state: found.state.clone(), columns: found.columns.clone() });
        }
    }
}

pub(super) fn scope(session: &Session, key: &Key, window: &ToolWindowDeclaration) -> Result<Values, CommandError> {
    let listed = session.state::<ToolState>().and_then(|state| state.sections.get(key));
    let Some(values) = listed.map(|listed| listed.values.clone()).filter(|values| !values.is_empty()) else {
        return Err(CommandError::new(t!("workbench-tool-list-again")));
    };
    if expected(session, window).is_some_and(|expected| expected != values) {
        return Err(CommandError::new(t!("workbench-tool-list-again")));
    }
    Ok(values)
}
