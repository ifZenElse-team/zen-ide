use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_syntax::Feature;

use crate::Workbench;
use crate::language::{byte_offset, char_offset};

/// The parameters of `editor.emmet.expand`.
#[derive(Deserialize, JsonSchema)]
pub struct ExpandParams {
    /// The file, open in the editor.
    pub path: PathBuf,
    /// The caret, a character offset: the abbreviation ends there.
    pub offset: usize,
}

/// What `editor.emmet.expand` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct ExpandResult {
    /// Whether an abbreviation was expanded.
    pub applied: bool,
}

pub(crate) fn expand(session: &mut Session, path: &Path, offset: usize) -> Result<bool, CommandError> {
    let text = session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .filter(|document| document.image().is_none())
        .map(|document| document.text().to_owned())
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let Some(plugin) = super::plugin_for(session, path, Feature::Expansion) else { return Ok(false) };
    let caret = byte_offset(&text, offset);
    let Some(expansion) = plugin.expand(&text, caret) else { return Ok(false) };
    let start = char_offset(&text, expansion.range.start);
    let end = char_offset(&text, expansion.range.end);
    let caret_in = expansion.caret.filter(|at| expansion.text.is_char_boundary(*at)).unwrap_or(expansion.text.len());
    let document = session
        .state_mut::<Workbench>()
        .documents
        .iter_mut()
        .find(|document| document.path() == path)
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let at = start + document.with_line_separator(expansion.text[..caret_in].to_owned()).chars().count();
    if !document.replace_ranges(vec![(start..end, expansion.text)]) {
        return Ok(false);
    }
    document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(at)));
    Ok(true)
}

pub(crate) fn tab_expands(session: &Session) -> Option<(PathBuf, usize)> {
    let document = session.state::<Workbench>()?.active_document()?;
    let selections = document.selections();
    let primary = selections.primary();
    if selections.len() != 1 || primary.anchor != primary.head {
        return None;
    }
    super::plugin_for(session, document.path(), Feature::Expansion)?;
    Some((document.path().to_owned(), primary.head))
}

pub struct ExpandAbbreviation;

impl Command for ExpandAbbreviation {
    type Params = ExpandParams;
    type Output = ExpandResult;
    const ID: &'static str = "editor.emmet.expand";
    const TITLE: &'static str = "Expand Abbreviation";
    const DESCRIPTION: &'static str = "Expands the abbreviation before a character offset of an open file with the file's \
         plug-in, e.g. `ul>li` into a list; `applied` is false when there is none.";

    fn run(&self, session: &mut Session, params: ExpandParams) -> Result<ExpandResult, CommandError> {
        Ok(ExpandResult { applied: expand(session, &params.path, params.offset)? })
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ExpandAbbreviation)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::plugins::testing::session;

    #[test]
    fn expands_and_tab_falls_back() {
        let (registry, mut session, folder, _plugin) = session(&[("page.web", "é ul>li\n"), ("notes.txt", "ul>li")]);
        let path = folder.path().join("page.web");
        assert!(registry.dispatch(&mut session, "editor.emmet.expand", json!({ "path": path, "offset": 7 })).is_err());
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        let expand = |session: &mut Session, offset: usize| {
            registry.dispatch(session, "editor.emmet.expand", json!({ "path": path, "offset": offset })).unwrap()
        };
        assert_eq!(expand(&mut session, 3), json!({ "applied": false }));
        assert_eq!(expand(&mut session, 7), json!({ "applied": true }));
        let document = session.state::<Workbench>().unwrap().active_document().unwrap();
        assert_eq!(document.text(), "é <ul><li></li></ul>\n");
        assert_eq!(document.selections().primary().head, 10, "the caret inside the item");
        registry.dispatch(&mut session, "editor.edit.undo", json!({})).unwrap();
        let undone = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        assert_eq!(undone, "é ul>li\n", "one undo step");

        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 7 }] })).unwrap();
        registry.dispatch(&mut session, "editor.edit.tab", json!({})).unwrap();
        let text = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        assert_eq!(text, "é <ul><li></li></ul>\n", "Tab expands first");
        registry.dispatch(&mut session, "editor.edit.tab", json!({})).unwrap();
        let text = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        assert_eq!(text, "é <ul><li>    </li></ul>\n", "and types the indentation when there is nothing to expand");

        let notes = folder.path().join("notes.txt");
        registry.dispatch(&mut session, "file.document.open", json!({ "path": notes })).unwrap();
        registry.dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": 5 }] })).unwrap();
        registry.dispatch(&mut session, "editor.edit.tab", json!({})).unwrap();
        let text = session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        assert_eq!(text, "ul>li    ", "a file without a plug-in");
    }
}
