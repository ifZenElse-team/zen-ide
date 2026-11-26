use std::path::PathBuf;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_editor::{Selection, Selections};
use zen_i18n::t;
use zen_text::Edit;

use crate::{Document, Workbench};

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ExpandWord)
}

/// The parameters of `editor.completion.expandWord`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ExpandWordParams {
    /// Whether the words after the caret come first (Cyclic Expand Word (Backward), ⌥⇧/) instead of those before it (Cyclic Expand Word, ⌥/).
    pub forward: bool,
}

/// What `editor.completion.expandWord` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExpandedWord {
    /// The word part left of the primary caret that the cycle completes.
    pub prefix: String,
    /// The text in its place now: a candidate, or the prefix itself past the last candidate.
    pub text: String,
    /// The candidate shown, from 1; 0 when the prefix is shown.
    pub index: usize,
    /// How many distinct candidates the cycle goes through.
    pub count: usize,
}

#[derive(Default)]
struct ExpandCycle(Option<Cycle>);

struct Cycle {
    path: PathBuf,
    version: u64,
    selection_version: u64,
    selections: Selections,
    prefix: String,
    candidates: Vec<String>,
    forward: bool,
    index: usize,
    carets: Vec<usize>,
}

impl Cycle {
    fn shown(&self) -> &str {
        match self.index {
            0 => &self.prefix,
            index => &self.candidates[index - 1],
        }
    }

    fn continues(&self, document: &Document) -> bool {
        self.path == document.path()
            && self.version == document.version()
            && self.selection_version == document.selection_version()
            && self.selections == *document.selections()
    }

    fn answer(&self) -> ExpandedWord {
        ExpandedWord {
            prefix: self.prefix.clone(),
            text: self.shown().to_owned(),
            index: self.index,
            count: self.candidates.len(),
        }
    }
}

fn text_document(session: &Session) -> Option<&Document> {
    session.state::<Workbench>().and_then(Workbench::active_document).filter(|document| document.image().is_none())
}

fn other_texts<'a>(workbench: &'a Workbench, active: &Document) -> Vec<&'a str> {
    let recent = workbench.recent_files();
    let mut others: Vec<&Document> = workbench
        .documents()
        .iter()
        .filter(|document| document.path() != active.path() && document.image().is_none())
        .collect();
    others.sort_by_key(|document| recent.iter().position(|path| path == document.path()).unwrap_or(usize::MAX));
    others.into_iter().map(Document::text).collect()
}

fn prefix_at(document: &Document, head: usize) -> (usize, &str) {
    let text = document.text();
    let caret = document.snapshot().offset_to_byte(head).unwrap_or(text.len());
    (caret, zen_editor::expand_word::word_prefix(&text[..caret]))
}

fn start(session: &Session, document: &Document, forward: bool) -> Option<Cycle> {
    let selections = document.selections();
    let (caret, prefix) = prefix_at(document, selections.primary().head);
    let others = session.state::<Workbench>().map(|workbench| other_texts(workbench, document)).unwrap_or_default();
    let candidates = zen_editor::expand_word::expansions(document.text(), caret, prefix, forward, &others);
    if candidates.is_empty() {
        return None;
    }
    let carets = selections
        .iter()
        .enumerate()
        .filter(|(_, selection)| prefix_at(document, selection.head).1 == prefix)
        .map(|(index, _)| index)
        .collect();
    Some(Cycle {
        path: document.path().to_owned(),
        version: document.version(),
        selection_version: document.selection_version(),
        selections: selections.clone(),
        prefix: prefix.to_owned(),
        candidates,
        forward,
        index: 0,
        carets,
    })
}

pub struct ExpandWord;

impl Command for ExpandWord {
    type Params = ExpandWordParams;
    type Output = ExpandedWord;
    const ID: &'static str = "editor.completion.expandWord";
    const TITLE: &'static str = "Cyclic Expand Word";
    const DESCRIPTION: &'static str = "Replaces the word part left of the caret with the nearest word of the open \
         files that starts with it, without a popup or a language server: the words before the caret nearest first, \
         then those after it, then the other open files' (`forward`: those after the caret first). Run again with \
         nothing else done in between, it puts the next distinct word in place of the previous one, and the prefix \
         itself after the last one; the other direction steps back. Every caret with the same word part changes \
         alike; each change is one undo step.";

    fn is_available(&self, session: &Session) -> bool {
        text_document(session).is_some_and(|document| document.read_only().is_none())
    }

    fn run(&self, session: &mut Session, params: ExpandWordParams) -> Result<ExpandedWord, CommandError> {
        let previous = session.state_mut::<ExpandCycle>().0.take();
        let document = text_document(session).ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let mut cycle = match previous.filter(|cycle| cycle.continues(document)) {
            Some(cycle) => cycle,
            None => match start(session, document, params.forward) {
                Some(cycle) => cycle,
                None => {
                    let prefix = prefix_at(document, document.selections().primary().head).1.to_owned();
                    return Ok(ExpandedWord { text: prefix.clone(), prefix, index: 0, count: 0 });
                }
            },
        };
        let shown = cycle.shown().chars().count();
        let steps = cycle.candidates.len() + 1;
        cycle.index =
            if params.forward == cycle.forward { (cycle.index + 1) % steps } else { (cycle.index + steps - 1) % steps };
        let replacement = cycle.shown().to_owned();
        let items: Vec<Selection> = cycle.selections.iter().copied().collect();
        let edits: Vec<Edit> = cycle
            .carets
            .iter()
            .rev()
            .map(|&index| {
                let head = items[index].head;
                Edit::replace(head - shown..head, replacement.clone())
            })
            .collect();
        let carets: Vec<Selection> = items
            .iter()
            .enumerate()
            .map(
                |(index, selection)| {
                    if cycle.carets.contains(&index) { Selection::caret(selection.head) } else { *selection }
                },
            )
            .collect();
        let selections = Selections::from_vec(carets, cycle.selections.primary_index()).mapped(&edits);
        let workbench = session.state_mut::<Workbench>();
        let Some(document) = workbench.active_document_mut() else {
            return Err(CommandError::new(t!("workbench-error-no-document")));
        };
        if !document.apply_action(edits, selections) {
            return Ok(ExpandedWord { text: cycle.prefix.clone(), prefix: cycle.prefix, index: 0, count: 0 });
        }
        cycle.version = document.version();
        cycle.selection_version = document.selection_version();
        cycle.selections = document.selections().clone();
        let answer = cycle.answer();
        session.state_mut::<ExpandCycle>().0 = Some(cycle);
        Ok(answer)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use zen_core::{Registry, Session};

    use crate::Workbench;

    struct Fixture {
        registry: Registry,
        session: Session,
        _folder: tempfile::TempDir,
    }

    impl Fixture {
        fn new(files: &[(&str, &str)]) -> Self {
            let folder = tempfile::tempdir().unwrap();
            let mut registry = Registry::new();
            crate::register(&mut registry).unwrap();
            let mut session = Session::new();
            registry.dispatch(&mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
            for (name, text) in files {
                let path = folder.path().join(name);
                std::fs::write(&path, text).unwrap();
                registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
            }
            Self { registry, session, _folder: folder }
        }

        fn run(&mut self, id: &str, params: Value) -> Value {
            self.registry.dispatch(&mut self.session, id, params).unwrap()
        }

        fn expand(&mut self, forward: bool) -> Value {
            self.run("editor.completion.expandWord", json!({ "forward": forward }))
        }

        fn text(&self) -> String {
            self.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
        }

        fn carets(&self) -> Vec<usize> {
            let workbench = self.session.state::<Workbench>().unwrap();
            workbench.active_document().unwrap().selections().iter().map(|selection| selection.head).collect()
        }

        fn caret_at(&mut self, offset: usize) {
            self.run("editor.selection.set", json!({ "selections": [{ "head": offset }] }));
        }
    }

    #[test]
    fn backward_cycles_through_the_words_above_below_and_in_other_files_then_the_prefix() {
        let mut fixture =
            Fixture::new(&[("other.txt", "colour"), ("main.go", "counter := 1 // count\nco\nx := \"column\"\n")]);
        fixture.caret_at(24);
        let first = fixture.expand(false);
        assert_eq!(first, json!({ "prefix": "co", "text": "count", "index": 1, "count": 4 }));
        assert_eq!(fixture.text(), "counter := 1 // count\ncount\nx := \"column\"\n");
        assert_eq!(fixture.carets(), [27]);
        assert_eq!(fixture.expand(false)["text"], "counter");
        assert_eq!(fixture.text(), "counter := 1 // count\ncounter\nx := \"column\"\n");
        assert_eq!(fixture.expand(false)["text"], "column", "then the words below");
        assert_eq!(fixture.expand(false)["text"], "colour", "then the other open files");
        assert_eq!(fixture.expand(false), json!({ "prefix": "co", "text": "co", "index": 0, "count": 4 }));
        assert_eq!(fixture.text(), "counter := 1 // count\nco\nx := \"column\"\n", "the prefix comes back");
        assert_eq!(fixture.expand(false)["text"], "count", "and the cycle starts over");
        assert_eq!(fixture.expand(true)["text"], "co", "the other direction steps back");
        fixture.run("editor.edit.undo", json!({}));
        assert_eq!(fixture.text(), "counter := 1 // count\ncount\nx := \"column\"\n", "each step is one undo step");
    }

    #[test]
    fn forward_starts_below_and_another_action_ends_the_cycle() {
        let mut fixture = Fixture::new(&[("notes.txt", "counter\nco\ncolumn and coal\n")]);
        fixture.caret_at(10);
        assert_eq!(fixture.expand(true)["text"], "column");
        assert_eq!(fixture.expand(true)["text"], "coal");
        fixture.caret_at(12);
        let fresh = fixture.expand(true);
        assert_eq!(fresh["prefix"], "coal", "setting the caret ends the cycle: the word part is read again");
        assert_eq!(fresh["count"], 0);
        assert_eq!(fixture.text(), "counter\ncoal\ncolumn and coal\n");
        fixture.run("editor.edit.backspace", json!({}));
        fixture.run("editor.edit.backspace", json!({}));
        fixture.run("editor.edit.insert", json!({ "text": "u" }));
        assert_eq!(fixture.expand(false)["text"], "counter", "typing starts a new cycle from the new prefix");
        assert_eq!(fixture.text(), "counter\ncounter\ncolumn and coal\n");
    }

    #[test]
    fn every_caret_with_the_same_word_part_changes() {
        let mut fixture = Fixture::new(&[("a.txt", "values\nva\nva\nv\n")]);
        fixture.run(
            "editor.selection.set",
            json!({ "selections": [{ "head": 9 }, { "head": 12 }, { "head": 14 }], "primary": 0 }),
        );
        assert_eq!(fixture.expand(false)["text"], "values");
        assert_eq!(fixture.text(), "values\nvalues\nvalues\nv\n");
        assert_eq!(fixture.carets(), [13, 20, 22]);
        let none = fixture.expand(false);
        assert_eq!(none["index"], 0);
        assert_eq!(fixture.text(), "values\nva\nva\nv\n");
    }

    #[test]
    fn nothing_to_expand_leaves_the_text() {
        let mut fixture = Fixture::new(&[("a.txt", "alpha beta\n")]);
        fixture.caret_at(5);
        let answer = fixture.expand(false);
        assert_eq!(answer, json!({ "prefix": "alpha", "text": "alpha", "index": 0, "count": 0 }));
        fixture.caret_at(6);
        assert_eq!(fixture.expand(false)["count"], 0, "no word part left of the caret");
        assert_eq!(fixture.text(), "alpha beta\n");
    }
}
