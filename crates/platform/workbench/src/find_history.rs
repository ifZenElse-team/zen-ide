use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListFindHistory)?;
    registry.register(AddFindHistory)?;
    registry.register(ClearFindHistory)?;
    Ok(())
}

pub const FIND_HISTORY: usize = 50;

#[derive(Clone, Debug, Default)]
pub struct FindHistory {
    inner: Arc<Mutex<FindHistoryState>>,
}

#[derive(Debug, Default)]
struct FindHistoryState {
    file: Option<PathBuf>,
    data: FindHistoryData,
}

/// What the find history holds.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FindHistoryData {
    /// The queries searched for, newest first.
    pub queries: Vec<String>,
    /// The replacements used, newest first.
    pub replacements: Vec<String>,
    /// The file masks of Find in Files, newest first.
    pub masks: Vec<String>,
}

impl FindHistory {
    pub fn load(file: &Path) -> Self {
        let data = std::fs::read(file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        Self { inner: Arc::new(Mutex::new(FindHistoryState { file: Some(file.to_owned()), data })) }
    }

    fn lock(&self) -> MutexGuard<'_, FindHistoryState> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn data(&self) -> FindHistoryData {
        self.lock().data.clone()
    }

    pub fn remember(&self, query: Option<&str>, replacement: Option<&str>, mask: Option<&str>) {
        let mut state = self.lock();
        let before = state.data.clone();
        if let Some(query) = query.filter(|query| !query.trim().is_empty()) {
            put_first(&mut state.data.queries, query);
        }
        if let Some(replacement) = replacement.filter(|replacement| !replacement.is_empty()) {
            put_first(&mut state.data.replacements, replacement);
        }
        if let Some(mask) = mask.map(str::trim).filter(|mask| !mask.is_empty()) {
            put_first(&mut state.data.masks, mask);
        }
        if state.data != before {
            save(&state);
        }
    }

    pub fn clear(&self) {
        let mut state = self.lock();
        state.data = FindHistoryData::default();
        save(&state);
    }
}

fn put_first(list: &mut Vec<String>, value: &str) {
    list.retain(|other| other != value);
    list.insert(0, value.to_owned());
    list.truncate(FIND_HISTORY);
}

fn save(state: &FindHistoryState) {
    let Some(file) = &state.file else { return };
    let Ok(text) = serde_json::to_string_pretty(&state.data) else { return };
    if let Some(folder) = file.parent() {
        let _ = std::fs::create_dir_all(folder);
    }
    let _ = zen_vfs::write_atomic(file, text.as_bytes());
}

pub struct ListFindHistory;

impl Command for ListFindHistory {
    type Params = NoParams;
    type Output = FindHistoryData;
    const ID: &'static str = "search.findHistory.list";
    const TITLE: &'static str = "Find History";
    const DESCRIPTION: &'static str = "The queries and replacements used in the search bar and in Find in Files, and the \
         file masks of Find in Files, newest first, as ⌥↓ and the mask's list show them.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<FindHistoryData, CommandError> {
        Ok(session.state_mut::<FindHistory>().data())
    }
}

/// What to add to the find history.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct AddHistoryParams {
    /// A query searched for.
    pub query: Option<String>,
    /// A replacement used.
    pub replacement: Option<String>,
    /// A file mask of Find in Files.
    pub mask: Option<String>,
}

pub struct AddFindHistory;

impl Command for AddFindHistory {
    type Params = AddHistoryParams;
    type Output = FindHistoryData;
    const ID: &'static str = "search.findHistory.add";
    const TITLE: &'static str = "Add to Find History";
    const DESCRIPTION: &'static str = "Puts a query, a replacement and a file mask first in the history the search bar \
         and Find in Files share, as searching with them does.";

    fn run(&self, session: &mut Session, params: AddHistoryParams) -> Result<FindHistoryData, CommandError> {
        let history = session.state_mut::<FindHistory>();
        history.remember(params.query.as_deref(), params.replacement.as_deref(), params.mask.as_deref());
        Ok(history.data())
    }
}

pub struct ClearFindHistory;

impl Command for ClearFindHistory {
    type Params = NoParams;
    type Output = FindHistoryData;
    const ID: &'static str = "search.findHistory.clear";
    const TITLE: &'static str = "Clear Find History";
    const DESCRIPTION: &'static str =
        "Forgets the queries, replacements and file masks of the search bar and Find in Files.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<FindHistoryData, CommandError> {
        let history = session.state_mut::<FindHistory>();
        history.clear();
        Ok(history.data())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_history_keeps_the_newest_first_without_repeats_and_survives_a_restart() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("find-history.json");
        let mut registry = Registry::new();
        register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(FindHistory::load(&file));
        let add = |session: &mut Session, params| registry.dispatch(session, "search.findHistory.add", params).unwrap();
        add(&mut session, json!({ "query": "Version", "mask": "*.go" }));
        add(&mut session, json!({ "query": "main", "replacement": "Main", "mask": " *.rs " }));
        let data = add(&mut session, json!({ "query": "Version", "mask": "" }));
        assert_eq!(data["queries"], json!(["Version", "main"]));
        assert_eq!(data["replacements"], json!(["Main"]));
        assert_eq!(data["masks"], json!(["*.rs", "*.go"]), "trimmed, an empty mask is not kept");
        let again = FindHistory::load(&file).data();
        assert_eq!(again.masks, ["*.rs", "*.go"]);
        assert_eq!(again.queries, ["Version", "main"]);
        let older: FindHistoryData = serde_json::from_str(r#"{"queries": ["a"]}"#).unwrap();
        assert!(older.masks.is_empty(), "a file without masks loads");
        let cleared = registry.dispatch(&mut session, "search.findHistory.clear", json!({})).unwrap();
        assert_eq!(cleared["masks"], json!([]));
    }
}
