use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Serialize;
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;

use super::sources::{IdParams, find};
use super::{Database, WORKSPACE};
use crate::workspaces::{Workspaces, close_inner_unguarded};

const CLOSED_KINDS: [&str; 4] = [super::objects::KIND, super::plans::KIND, super::sessions::KIND, super::keys::KEY_TAB];

#[derive(Clone, Debug, Default)]
pub struct Removal {
    tabs: Vec<(String, String)>,
    consoles: Vec<PathBuf>,
    results: Vec<u64>,
}

impl Removal {
    pub fn closing(&self) -> usize {
        self.tabs.len() + self.results.len()
    }

    pub fn consoles(&self) -> &[PathBuf] {
        &self.consoles
    }
}

pub fn of(session: &Session, id: &str) -> Removal {
    let Some(workspaces) = session.state::<Workspaces>() else { return Removal::default() };
    let mut removal = Removal::default();
    for tab in workspaces.tabs(WORKSPACE) {
        match &tab.path {
            Some(path) => {
                if workspaces.is_console(path) && super::consoles::connection_of(session, path).as_deref() == Some(id) {
                    removal.consoles.push(path.clone());
                }
            }
            None if CLOSED_KINDS.contains(&tab.kind.as_str()) && tab.data["connection"].as_str() == Some(id) => {
                removal.tabs.push((tab.id.clone(), tab.kind.clone()));
            }
            None => {}
        }
    }
    if let Some(consoles) = session.state::<super::consoles::Consoles>() {
        for path in &removal.consoles {
            removal.results.extend(consoles.tabs(path).iter().map(|tab| tab.result));
        }
    }
    removal
}

pub fn refuse_unsaved(session: &Session, removal: &Removal, retry: serde_json::Value) -> Result<(), CommandError> {
    let alterations = session.state::<super::alter::Alterations>();
    for (tab, kind) in &removal.tabs {
        if kind == super::objects::KIND && alterations.is_some_and(|alterations| alterations.staged(tab)) {
            let title = session
                .state::<Workspaces>()
                .and_then(|workspaces| workspaces.tabs(WORKSPACE).iter().find(|known| known.id == *tab))
                .map(|known| known.title.clone())
                .unwrap_or_default();
            return Err(CommandError::with_code(
                super::alter::PENDING_STRUCTURE,
                t!("database-alter-question-close", name = title.as_str()),
            )
            .with_data(serde_json::json!({
                "tab": tab,
                "retry": { "command": "db.dataSource.remove", "params": retry },
            })));
        }
    }
    let mut results = removal.results.clone();
    for (tab, kind) in &removal.tabs {
        if kind == super::objects::KIND {
            results.extend(super::objects::results_of(session, tab));
        }
    }
    super::edits::refuse_staged(session, &results, ("db.dataSource.remove", retry))
}

pub fn apply(session: &mut Session, removal: &Removal, name: &str) {
    for (tab, _) in &removal.tabs {
        let _ = close_inner_unguarded(session, WORKSPACE, tab);
    }
    super::objects::step(session);
    if let Some(engine) = session.state::<Database>().and_then(Database::started) {
        for result in &removal.results {
            engine.results().remove(*result);
        }
    }
    for path in &removal.consoles {
        super::consoles::unbind(session, path, name);
    }
}

/// What removing a connection would close and unbind.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RemovalAnswer {
    /// The connection's id.
    pub id: String,
    /// How many tabs close: its object, plan, sessions and key tabs and its consoles' result tabs.
    pub tabs: usize,
    /// How many open consoles stay without a connection.
    pub consoles: usize,
    /// The question to ask before removing it, naming those numbers.
    pub message: String,
}

pub struct RemovalPreview;

impl Command for RemovalPreview {
    type Params = IdParams;
    type Output = RemovalAnswer;
    const ID: &'static str = "db.dataSource.removal";
    const TITLE: &'static str = "Preview Removing a Database Connection";
    const DESCRIPTION: &'static str = "Tells what db.dataSource.remove would do to the window for connection `id`: how many tabs close (its object, plan, sessions and key tabs and its consoles' result tabs) and how many open consoles stay without a connection, with the confirmation question naming them (`message`). Changes nothing.";

    fn run(&self, session: &mut Session, params: IdParams) -> Result<RemovalAnswer, CommandError> {
        let entry = find(session, &params.id)?;
        let removal = of(session, &params.id);
        let (tabs, consoles) = (removal.closing(), removal.consoles.len());
        let message =
            t!("database-remove-question", name = entry.source.name.as_str(), tabs = tabs, consoles = consoles);
        Ok(RemovalAnswer { id: params.id, tabs, consoles, message })
    }
}
