use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::Workbench;

/// The parameters of `editor.occurrences.list`.
#[derive(Default, Deserialize, JsonSchema)]
pub struct OccurrencesParams {
    /// The character offset of the name; the primary caret when absent.
    #[serde(default)]
    pub offset: Option<usize>,
}

/// What `editor.occurrences.list` answers.
#[derive(Serialize, JsonSchema)]
pub struct Occurrences {
    /// The name, or `None` when the caret is not at one.
    pub name: Option<String>,
    /// Where it occurs, as character ranges `[start, end]`, in order, the one at the caret included.
    pub ranges: Vec<[usize; 2]>,
}

pub struct ListOccurrences;

impl Command for ListOccurrences {
    type Params = OccurrencesParams;
    type Output = Occurrences;
    const ID: &'static str = "editor.occurrences.list";
    const TITLE: &'static str = "Occurrences of the Name at the Caret";
    const DESCRIPTION: &'static str = "The name at the caret (or at an offset) in the active file and every place it \
         occurs there as a whole word, as the editor highlights them.";

    fn is_available(&self, session: &Session) -> bool {
        session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .is_some_and(|document| document.image().is_none())
    }

    fn run(&self, session: &mut Session, params: OccurrencesParams) -> Result<Occurrences, CommandError> {
        let document = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .filter(|document| document.image().is_none())
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let offset = params.offset.unwrap_or_else(|| document.selections().primary().head);
        let text = document.text();
        let name = zen_editor::occurrences::name_at(text, offset)
            .map(|range| text.chars().skip(range.start).take(range.len()).collect());
        let ranges = zen_editor::occurrences(text, offset).into_iter().map(|range| [range.start, range.end]).collect();
        Ok(Occurrences { name, ranges })
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListOccurrences)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn the_name_at_the_caret_and_its_occurrences() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("main.go");
        std::fs::write(&path, "var ёж string\n\nvar b string\n").unwrap();
        let mut registry = zen_core::Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = zen_core::Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        let found = registry.dispatch(&mut session, "editor.occurrences.list", json!({ "offset": 9 })).unwrap();
        assert_eq!(found, json!({ "name": "string", "ranges": [[7, 13], [21, 27]] }));
        let none = registry.dispatch(&mut session, "editor.occurrences.list", json!({ "offset": 14 })).unwrap();
        assert_eq!(none, json!({ "name": null, "ranges": [] }));
    }
}
