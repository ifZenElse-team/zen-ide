use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_editor::EditOp;
use zen_i18n::t;

use crate::Workbench;
use crate::editing::{SelectionInfo, describe_selections};

pub const LIMIT: &str = "editor.clipboardHistory.limit";

pub const DEFAULT_LIMIT: usize = 20;

pub const MAX_LIMIT: usize = 100;

pub const MAX_ENTRY_BYTES: usize = 1024 * 1024;

pub const PREVIEW_CHARS: usize = 60;

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(AddEntry)?;
    registry.register(ListEntries)?;
    registry.register(PasteEntry)?;
    registry.register(RemoveEntry)?;
    registry.register(ClearEntries)?;
    Ok(())
}

#[derive(Clone, Debug, Default)]
pub struct ClipboardHistory {
    inner: Arc<Mutex<Entries>>,
}

#[derive(Debug, Default)]
struct Entries {
    texts: VecDeque<Arc<str>>,
    generation: u64,
}

impl ClipboardHistory {
    fn lock(&self) -> MutexGuard<'_, Entries> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn change(&self, edit: impl FnOnce(&mut VecDeque<Arc<str>>) -> bool) -> bool {
        let mut entries = self.lock();
        let changed = edit(&mut entries.texts);
        if changed {
            entries.generation += 1;
        }
        changed
    }

    pub fn record(&self, text: &str, limit: usize) -> bool {
        if text.is_empty() || text.len() > MAX_ENTRY_BYTES {
            return false;
        }
        self.change(|texts| {
            if texts.front().is_some_and(|first| &**first == text) {
                return false;
            }
            texts.retain(|entry| &**entry != text);
            texts.push_front(Arc::from(text));
            texts.truncate(limit.max(1));
            true
        });
        true
    }

    pub fn entries(&self) -> Vec<Arc<str>> {
        self.lock().texts.iter().cloned().collect()
    }

    pub fn generation(&self) -> u64 {
        self.lock().generation
    }

    pub fn get(&self, index: usize) -> Option<Arc<str>> {
        self.lock().texts.get(index).cloned()
    }

    pub fn remove(&self, index: usize) -> bool {
        self.change(|texts| texts.remove(index).is_some())
    }

    pub fn clear(&self) {
        self.change(|texts| {
            let had = !texts.is_empty();
            texts.clear();
            had
        });
    }

    pub fn keep(&self, limit: usize) {
        self.change(|texts| {
            let before = texts.len();
            texts.truncate(limit.clamp(1, MAX_LIMIT));
            texts.len() != before
        });
    }

    pub fn len(&self) -> usize {
        self.lock().texts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().texts.is_empty()
    }
}

pub fn preview(text: &str, chars: usize) -> String {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default();
    if line.chars().count() <= chars {
        return line.to_owned();
    }
    let mut cut: String = line.chars().take(chars.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

fn limit(session: &Session) -> usize {
    session.settings().get_or(LIMIT, DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

fn history(session: &mut Session) -> (ClipboardHistory, usize) {
    let limit = limit(session);
    let history = session.state_mut::<ClipboardHistory>().clone();
    history.keep(limit);
    (history, limit)
}

fn describe(history: &ClipboardHistory) -> Vec<EntryInfo> {
    history
        .entries()
        .iter()
        .enumerate()
        .map(|(index, text)| EntryInfo {
            index,
            preview: preview(text, PREVIEW_CHARS),
            lines: text.lines().count().max(1),
            chars: text.chars().count(),
        })
        .collect()
}

/// An entry of the history as commands report it: never the whole text, only a short preview.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntryInfo {
    /// The entry's place, from 0 for the newest.
    pub index: usize,
    /// The first line with text in it, trimmed and cut to 60 characters.
    pub preview: String,
    /// How many lines the entry has.
    pub lines: usize,
    /// How many characters the entry has.
    pub chars: usize,
}

/// The parameters of `editor.clipboardHistory.add`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct AddParams {
    /// The copied text.
    pub text: String,
}

pub struct AddEntry;

impl Command for AddEntry {
    type Params = AddParams;
    type Output = bool;
    const ID: &'static str = "editor.clipboardHistory.add";
    const TITLE: &'static str = "Add to Clipboard History";
    const DESCRIPTION: &'static str = "Puts a text first in Paste from History, as copying it in the IDE does; the system \
         clipboard is left as it is. Returns whether the text was kept: an empty one or one over 1 MiB is not.";

    fn run(&self, session: &mut Session, params: AddParams) -> Result<bool, CommandError> {
        let (history, limit) = history(session);
        Ok(history.record(&params.text, limit))
    }
}

pub struct ListEntries;

impl Command for ListEntries {
    type Params = NoParams;
    type Output = Vec<EntryInfo>;
    const ID: &'static str = "editor.clipboardHistory.list";
    const TITLE: &'static str = "Clipboard History";
    const DESCRIPTION: &'static str = "Lists the texts copied and cut in the IDE during this run, newest first (Paste from \
         History, ⇧⌘V). Copied text may hold secrets, so each entry comes as its first line cut to 60 characters, with \
         its numbers of lines and characters, never whole.";

    fn run(&self, session: &mut Session, _params: NoParams) -> Result<Vec<EntryInfo>, CommandError> {
        Ok(describe(&history(session).0))
    }
}

/// The parameters of `editor.clipboardHistory.paste`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
pub struct PasteParams {
    /// The entry to paste, from 0 for the newest; the newest when neither this nor `indices` is given.
    pub index: Option<usize>,
    /// Several entries to paste at once, joined by line breaks in this order.
    #[serde(default)]
    pub indices: Vec<usize>,
}

pub struct PasteEntry;

impl Command for PasteEntry {
    type Params = PasteParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.clipboardHistory.paste";
    const TITLE: &'static str = "Paste from History";
    const DESCRIPTION: &'static str = "Pastes an entry of the clipboard history (`index`, from 0 for the newest), or \
         several joined by line breaks (`indices`), at every caret of the active file, as Paste does; what is pasted \
         becomes the newest entry. Returns the selections.";

    fn is_available(&self, session: &Session) -> bool {
        session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .is_some_and(|document| document.image().is_none())
    }

    fn run(&self, session: &mut Session, params: PasteParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let (history, limit) = history(session);
        let indices = if params.indices.is_empty() { vec![params.index.unwrap_or(0)] } else { params.indices };
        let mut pieces: Vec<Arc<str>> = Vec::with_capacity(indices.len());
        for index in indices {
            let entry = history.get(index).ok_or_else(|| {
                CommandError::new(t!("workbench-error-clipboard-entry", index = index, count = history.len()))
            })?;
            pieces.push(entry);
        }
        let text = pieces.join("\n");
        let document = session
            .state_mut::<Workbench>()
            .active_document_mut()
            .filter(|document| document.image().is_none())
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let pasted = document.apply_op(&EditOp::Paste { text: text.clone() });
        let selections = describe_selections(document);
        if pasted {
            history.record(&text, limit);
        }
        Ok(selections)
    }
}

/// The parameters of `editor.clipboardHistory.remove`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct RemoveParams {
    /// The entry to remove, from 0 for the newest.
    pub index: usize,
}

pub struct RemoveEntry;

impl Command for RemoveEntry {
    type Params = RemoveParams;
    type Output = Vec<EntryInfo>;
    const ID: &'static str = "editor.clipboardHistory.remove";
    const TITLE: &'static str = "Remove from Clipboard History";
    const DESCRIPTION: &'static str = "Removes an entry (`index`, from 0 for the newest) from Paste from History. Returns the entries left as previews.";

    fn run(&self, session: &mut Session, params: RemoveParams) -> Result<Vec<EntryInfo>, CommandError> {
        let (history, _) = history(session);
        if !history.remove(params.index) {
            return Err(CommandError::new(t!(
                "workbench-error-clipboard-entry",
                index = params.index,
                count = history.len()
            )));
        }
        Ok(describe(&history))
    }
}

pub struct ClearEntries;

impl Command for ClearEntries {
    type Params = NoParams;
    type Output = Vec<EntryInfo>;
    const ID: &'static str = "editor.clipboardHistory.clear";
    const TITLE: &'static str = "Clear Clipboard History";
    const DESCRIPTION: &'static str =
        "Forgets every entry of Paste from History; the system clipboard is left as it is.";

    fn run(&self, session: &mut Session, _params: NoParams) -> Result<Vec<EntryInfo>, CommandError> {
        let (history, _) = history(session);
        history.clear();
        Ok(describe(&history))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn session(contents: &str) -> (tempfile::TempDir, Registry, Session) {
        zen_i18n::register(crate::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        std::fs::write(&file, contents).unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        (dir, registry, session)
    }

    fn text(session: &Session) -> String {
        session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
    }

    fn previews(list: &serde_json::Value) -> Vec<&str> {
        list.as_array().unwrap().iter().map(|entry| entry["preview"].as_str().unwrap()).collect()
    }

    #[test]
    fn the_history_keeps_the_newest_first_without_repeats() {
        let history = ClipboardHistory::default();
        assert!(history.record("one", 3));
        assert!(history.record("two", 3));
        assert!(history.record("one", 3));
        let texts =
            |history: &ClipboardHistory| history.entries().iter().map(|entry| entry.to_string()).collect::<Vec<_>>();
        assert_eq!(texts(&history), ["one", "two"]);
        history.record("three", 3);
        history.record("four", 3);
        assert_eq!(texts(&history), ["four", "three", "one"]);
        assert!(!history.record("", 3));
        assert!(!history.record(&"x".repeat(MAX_ENTRY_BYTES + 1), 3));
        assert_eq!(history.len(), 3);
        let shared = history.clone();
        shared.record("five", 3);
        assert_eq!(history.get(0).as_deref(), Some("five"));
    }

    #[test]
    fn previews_take_the_first_line_with_text_cut_short() {
        assert_eq!(preview("\n  first line  \nsecond", 60), "first line");
        assert_eq!(preview("abcdefgh", 5), "abcd…");
        assert_eq!(preview("абвгд", 5), "абвгд");
        assert_eq!(preview("\n\n", 5), "");
    }

    #[test]
    fn commands_list_add_paste_remove_and_clear() {
        let (_dir, registry, mut session) = session("x\n");
        let run = |session: &mut Session, id: &str, params: serde_json::Value| {
            registry.dispatch(session, id, params).unwrap()
        };
        assert_eq!(run(&mut session, "editor.clipboardHistory.list", json!({})), json!([]));
        assert_eq!(run(&mut session, "editor.clipboardHistory.add", json!({ "text": "alpha" })), json!(true));
        let secret = format!("token={}\nsecond line", "s".repeat(100));
        let added = run(&mut session, "editor.clipboardHistory.add", json!({ "text": secret }));
        assert_eq!(added, json!(true), "adding answers nothing about the entries");
        let huge = "x".repeat(MAX_ENTRY_BYTES + 1);
        assert_eq!(run(&mut session, "editor.clipboardHistory.add", json!({ "text": huge })), json!(false));
        let list = run(&mut session, "editor.clipboardHistory.list", json!({}));
        assert_eq!(list[0]["preview"].as_str().unwrap().chars().count(), PREVIEW_CHARS);
        assert!(!list.to_string().contains("second line"), "never the whole text: {list}");
        assert_eq!((list[0]["lines"].as_u64(), list[0]["chars"].as_u64()), (Some(2), Some(118)));
        assert_eq!(list[1], json!({ "index": 1, "preview": "alpha", "lines": 1, "chars": 5 }));

        run(&mut session, "editor.caret.move", json!({ "movement": { "kind": "documentStart" } }));
        run(&mut session, "editor.clipboardHistory.paste", json!({ "index": 1 }));
        assert_eq!(text(&session), "alphax\n");
        let list = run(&mut session, "editor.clipboardHistory.list", json!({}));
        assert_eq!(list[0]["preview"], "alpha", "what is pasted comes first");

        run(&mut session, "editor.clipboardHistory.add", json!({ "text": "beta" }));
        run(&mut session, "editor.clipboardHistory.paste", json!({ "indices": [1, 0] }));
        assert_eq!(text(&session), "alphaalpha\nbetax\n");
        let list = run(&mut session, "editor.clipboardHistory.list", json!({}));
        assert_eq!(previews(&list)[..3], ["alpha", "beta", "alpha"]);

        let error =
            registry.dispatch(&mut session, "editor.clipboardHistory.paste", json!({ "index": 9 })).unwrap_err();
        assert!(error.to_string().contains('9'), "{error}");
        let list = run(&mut session, "editor.clipboardHistory.remove", json!({ "index": 0 }));
        let cut = format!("token={}…", "s".repeat(PREVIEW_CHARS - 7));
        assert_eq!(previews(&list), ["beta", "alpha", cut.as_str()]);
        assert!(registry.dispatch(&mut session, "editor.clipboardHistory.remove", json!({ "index": 5 })).is_err());
        assert_eq!(run(&mut session, "editor.clipboardHistory.clear", json!({})), json!([]));
    }

    #[test]
    fn a_paste_gives_each_caret_its_line_when_they_match_and_undoes_in_one_step() {
        let (_dir, registry, mut session) = session("a\nb\nc\n");
        let run = |session: &mut Session, id: &str, params: serde_json::Value| {
            registry.dispatch(session, id, params).unwrap()
        };
        let carets = json!({ "selections": [{ "head": 0 }, { "head": 2 }, { "head": 4 }] });
        run(&mut session, "editor.selection.set", carets.clone());
        run(&mut session, "editor.clipboardHistory.add", json!({ "text": "1\n2\n3" }));
        run(&mut session, "editor.clipboardHistory.paste", json!({}));
        assert_eq!(text(&session), "1a\n2b\n3c\n", "a line at each caret, as Paste does");
        run(&mut session, "editor.edit.undo", json!({}));
        assert_eq!(text(&session), "a\nb\nc\n", "one undo takes the whole paste back");

        run(&mut session, "editor.selection.set", carets);
        run(&mut session, "editor.clipboardHistory.add", json!({ "text": "x\ny" }));
        run(&mut session, "editor.clipboardHistory.paste", json!({}));
        assert_eq!(text(&session), "x\nya\nx\nyb\nx\nyc\n", "otherwise the whole entry at every caret");
        run(&mut session, "editor.edit.undo", json!({}));
        assert_eq!(text(&session), "a\nb\nc\n");
    }

    #[test]
    fn keeping_fewer_drops_the_oldest_and_moves_the_generation() {
        let history = ClipboardHistory::default();
        for text in ["a", "b", "c"] {
            history.record(text, 10);
        }
        let generation = history.generation();
        history.record("c", 10);
        assert_eq!(history.generation(), generation, "copying the newest again changes nothing");
        history.keep(2);
        assert_eq!(history.entries().iter().map(|entry| &**entry).collect::<Vec<_>>(), ["c", "b"]);
        assert!(history.generation() > generation);
    }

    #[test]
    fn the_setting_limits_the_entries() {
        let (_dir, registry, mut session) = session("");
        for index in 0..30 {
            registry
                .dispatch(&mut session, "editor.clipboardHistory.add", json!({ "text": format!("{index}") }))
                .unwrap();
        }
        let list = registry.dispatch(&mut session, "editor.clipboardHistory.list", json!({})).unwrap();
        assert_eq!(list.as_array().unwrap().len(), DEFAULT_LIMIT);
        assert_eq!(list[0]["preview"], "29");
        session.settings_mut().set(LIMIT, 5).unwrap();
        let list = registry.dispatch(&mut session, "editor.clipboardHistory.list", json!({})).unwrap();
        assert_eq!(previews(&list), ["29", "28", "27", "26", "25"]);
    }

    #[test]
    fn pasting_needs_a_file() {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "editor.clipboardHistory.add", json!({ "text": "a" })).unwrap();
        assert!(registry.dispatch(&mut session, "editor.clipboardHistory.paste", json!({})).is_err());
    }
}
