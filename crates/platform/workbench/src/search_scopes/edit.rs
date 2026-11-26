use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_index::scope::Pattern;
use zen_project::settings::Scope;

use super::{error_message, root_of, scope_set};

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(SaveScope)?;
    registry.register(DeleteScope)?;
    Ok(())
}

pub const OWN_SCOPES: &str = ".zen/scopes.json";

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct OwnScopes {
    scopes: Vec<Scope>,
}

pub fn own_scopes(root: &Path) -> Vec<Scope> {
    std::fs::read(root.join(OWN_SCOPES))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<OwnScopes>(&bytes).ok())
        .map(|own| own.scopes)
        .unwrap_or_default()
}

fn write_own(root: &Path, scopes: Vec<Scope>) -> Result<(), CommandError> {
    let failed = |error: &dyn std::fmt::Display| {
        CommandError::new(t!("workbench-scope-write-failed", error = error.to_string()))
    };
    let file = root.join(OWN_SCOPES);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|error| failed(&error))?;
    }
    let text = serde_json::to_vec_pretty(&OwnScopes { scopes }).unwrap_or_default();
    zen_vfs::write_atomic(&file, &text).map_err(|error| failed(&error))
}

/// What `search.scope.save` takes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct SaveParams {
    /// The scope's name.
    pub name: String,
    /// Its pattern in the scope language, e.g. `file:internal//*&&!file:*_test.go`.
    pub pattern: String,
    /// The name it had before, when it is renamed.
    pub previous: Option<String>,
}

/// What `search.scope.delete` takes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct DeleteParams {
    /// The scope's name.
    pub name: String,
}

fn project(session: &Session) -> Result<PathBuf, CommandError> {
    root_of(session).ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))
}

pub struct SaveScope;

impl Command for SaveScope {
    type Params = SaveParams;
    type Output = ();
    const ID: &'static str = "search.scope.save";
    const TITLE: &'static str = "Save Search Scope";
    const DESCRIPTION: &'static str = "Adds or changes a named scope of the project, kept in .zen/scopes.json: its name \
         and its pattern in the scope language; `previous` renames one. A scope of the same name from .idea/scopes is \
         hidden by it, never rewritten. An unusable pattern is refused with the reason.";

    fn run(&self, session: &mut Session, params: SaveParams) -> Result<(), CommandError> {
        let root = project(session)?;
        let name = params.name.trim().to_owned();
        let pattern = params.pattern.trim().to_owned();
        if name.is_empty() {
            return Err(CommandError::new(t!("workbench-scope-name-empty")));
        }
        let parsed = Pattern::parse(&pattern).map_err(|error| CommandError::new(error_message(&error)))?;
        let mut set = scope_set(session);
        set.add(&name, &pattern);
        set.check(&parsed).map_err(|error| CommandError::new(error_message(&error)))?;
        let mut scopes = own_scopes(&root);
        let previous = params.previous.unwrap_or_else(|| name.clone());
        let saved = Scope { name: name.clone(), pattern };
        let at = scopes.iter().position(|scope| scope.name == previous || scope.name == name);
        scopes.retain(|scope| scope.name != previous && scope.name != name);
        scopes.insert(at.unwrap_or(scopes.len()).min(scopes.len()), saved);
        write_own(&root, scopes)
    }
}

pub struct DeleteScope;

impl Command for DeleteScope {
    type Params = DeleteParams;
    type Output = ();
    const ID: &'static str = "search.scope.delete";
    const TITLE: &'static str = "Delete Search Scope";
    const DESCRIPTION: &'static str = "Removes a named scope the project keeps in .zen/scopes.json; the scopes of \
         .idea/scopes are read-only.";

    fn run(&self, session: &mut Session, params: DeleteParams) -> Result<(), CommandError> {
        let root = project(session)?;
        let mut scopes = own_scopes(&root);
        let before = scopes.len();
        scopes.retain(|scope| scope.name != params.name);
        if scopes.len() == before {
            return Err(CommandError::new(t!("workbench-scope-not-own", name = params.name)));
        }
        write_own(&root, scopes)
    }
}
