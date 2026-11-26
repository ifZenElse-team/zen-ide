use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_workbench::Workbench;
use zen_workbench::compare::{ThreeWayView, Whitespace};
use zen_workbench::vcs::commit::DiffView;

use egui::Ui;
use zen_workbench::compare_folders::{EntryKind, EntrySide, EntryState, SyncAction, SyncItem, SyncResult};

use super::IdeView;
use super::agent::ActionError;
use super::diff_view::{DiffOutcome, DiffTab};
use super::folder_diff::{self, FolderDiffTab};
use super::front::FrontKind;

#[cfg(test)]
thread_local! {
    pub(super) static ASKED_OTHER: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

impl IdeView {
    pub(super) fn show_three(&mut self, session: &Session, view: ThreeWayView) {
        let name = view.paths.get(1).and_then(|path| path.file_name()).map(|name| name.to_string_lossy().into_owned());
        let highlighter = self.highlighter_for(&name.unwrap_or_default());
        self.diff = Some(DiffTab::three(view, highlighter));
        self.bring_front(FrontKind::Diff, session);
        self.diff_from_workspace = false;
        self.close_text_dialogs();
    }

    pub(super) fn compare_paths(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        paths: &[PathBuf],
    ) -> Result<Value, ActionError> {
        let result = match paths {
            [left, right] => registry.dispatch(session, "file.compare.files", json!({ "left": left, "right": right })),
            [left, middle, right] => registry.dispatch(
                session,
                "file.compare.three",
                json!({ "left": left, "middle": middle, "right": right }),
            ),
            _ => return Err(ActionError::Refused("Compare Files compares two or three files".to_owned())),
        }
        .map_err(ActionError::Dispatch)?;
        if paths.len() == 3 {
            if let Ok(view) = serde_json::from_value::<ThreeWayView>(result.clone()) {
                self.show_three(session, view);
            }
        } else if let Ok(view) = serde_json::from_value::<DiffView>(result.clone()) {
            self.show_diff(session, view);
        }
        Ok(result)
    }

    pub(super) fn after_diff(&mut self, registry: &Registry, session: &mut Session, outcome: DiffOutcome) {
        if let Some((path, range, text)) = outcome.write {
            self.write_diff_side(registry, session, &path, range, text);
        }
        if outcome.rehunk {
            self.rehunk_diff(registry, session);
        }
    }

    fn write_diff_side(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        path: &Path,
        range: Option<[usize; 2]>,
        text: String,
    ) {
        let params = json!({ "path": path, "text": text, "range": range });
        if let Ok(written) = registry.dispatch(session, "file.compare.writeSide", params)
            && let Some(diff) = &mut self.diff
            && let Ok(range) = serde_json::from_value::<[usize; 2]>(written["range"].clone())
            && let Some(document) = session
                .state::<Workbench>()
                .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        {
            diff.written(range, document.version(), document.snapshot().clone());
        }
    }

    pub(super) fn follow_diff_file(&mut self, registry: &Registry, session: &mut Session) {
        let Some(diff) = &mut self.diff else { return };
        let Some(path) = diff.file_path().map(Path::to_path_buf) else { return };
        let Some(document) = session
            .state::<Workbench>()
            .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        else {
            return;
        };
        if diff.follow(document) {
            self.rehunk_diff(registry, session);
        }
    }

    pub(super) fn rehunk_diff(&mut self, registry: &Registry, session: &mut Session) {
        let Some(diff) = &self.diff else { return };
        let (texts, whitespace) = (diff.texts(), diff.whitespace);
        let compare = |session: &mut Session, left: &str, right: &str| {
            let params = json!({ "left": left, "right": right, "whitespace": whitespace });
            registry
                .dispatch(session, "file.compare.texts", params)
                .ok()
                .and_then(|view| serde_json::from_value::<DiffView>(view).ok())
                .map(|view| view.hunks)
        };
        let (hunks, right) = match texts.as_slice() {
            [left, middle, right] => (compare(session, left, middle), compare(session, middle, right)),
            [before, after] => (compare(session, before, after), None),
            _ => return,
        };
        if let (Some(diff), Some(hunks)) = (&mut self.diff, hunks) {
            diff.set_hunks(hunks, right);
        }
    }

    fn file_in_context(&self, session: &Session, params: &Value) -> Option<PathBuf> {
        let given = params.get("path").and_then(Value::as_str).map(PathBuf::from);
        let active = session
            .state::<Workbench>()
            .and_then(Workbench::active_document)
            .filter(|_| self.editor_in_front(session))
            .map(|document| document.path().to_owned());
        let tree = self.selection.lead().cloned().filter(|path| path.is_file());
        let focused = tree.clone().filter(|_| self.tree_focused);
        given.or(focused).or(active).or(tree).filter(|path| path.is_file())
    }

    fn editor_in_front(&self, session: &Session) -> bool {
        !self.diff_in_front(session)
            && !self.git_in_front(session)
            && !self.rebase_in_front(session)
            && !self.conflict_in_front(session)
            && zen_workbench::workspaces::tab_in_front(session).is_none()
    }

    pub(super) fn compare_with(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        params: &Value,
        agent: bool,
    ) -> Result<Value, ActionError> {
        let given: Vec<PathBuf> =
            params.get("paths").and_then(|paths| serde_json::from_value(paths.clone()).ok()).unwrap_or_default();
        let folders: Vec<PathBuf> =
            self.selection.items(&self.visible_rows).into_iter().filter(|path| path.is_dir()).collect();
        let tree_folders = params.get("path").is_none() && given.is_empty() && self.tree_focused && folders.len() == 2;
        if (given.len() == 2 && given.iter().all(|path| path.is_dir())) || tree_folders {
            let paths = if tree_folders { folders } else { given };
            let asked = json!({ "paths": paths });
            return self.folder_action(ctx, registry, session, "view.folderDiff.compare", &asked, agent);
        }
        let selected: Vec<PathBuf> =
            self.selection.items(&self.visible_rows).into_iter().filter(|path| path.is_file()).collect();
        let paths = if !given.is_empty() {
            given
        } else if params.get("path").is_none() && self.tree_focused && (2..=3).contains(&selected.len()) {
            selected
        } else {
            Vec::new()
        };
        if !paths.is_empty() {
            return self.compare_paths(registry, session, &paths);
        }
        if agent {
            return Err(ActionError::Refused(
                "Compare With… asks for the other file in a system dialog; give `paths` (two or three files) or \
                 call file.compare.files with both files"
                    .to_owned(),
            ));
        }
        let Some(path) = self.file_in_context(session, params) else {
            return Err(ActionError::Refused("Compare With… needs the `path` of a file".to_owned()));
        };
        self.pending_tree_action = Some(super::TreeAction::CompareWith(path));
        Ok(Value::Null)
    }

    pub(super) fn diff_action(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        action: &str,
        params: &Value,
    ) -> Result<Value, ActionError> {
        use ActionError::{Dispatch, Refused};
        match action {
            "view.compare.clipboard" => {
                let Some(path) = self.file_in_context(session, params) else {
                    return Err(Refused("Compare with Clipboard needs a file in the editor or a `path`".to_owned()));
                };
                let mut asked = json!({ "path": path });
                for key in ["text", "selection", "whitespace"] {
                    if let Some(value) = params.get(key) {
                        asked[key] = value.clone();
                    }
                }
                let result = registry.dispatch(session, "file.compare.clipboard", asked).map_err(Dispatch)?;
                if let Ok(view) = serde_json::from_value::<DiffView>(result.clone()) {
                    self.show_diff(session, view);
                }
                Ok(result)
            }
            "view.diff.blank" => {
                let text = |key: &str| params.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
                let asked = json!({ "left": text("left"), "right": text("right") });
                let result = registry.dispatch(session, "file.compare.texts", asked).map_err(Dispatch)?;
                if let Ok(view) = serde_json::from_value::<DiffView>(result) {
                    let unified = self.diff_unified;
                    self.diff_unified = false;
                    self.show_diff(session, view);
                    self.diff_unified = unified;
                }
                Ok(self.diff.as_ref().map(DiffTab::state).unwrap_or_default())
            }
            _ => {
                self.follow_diff_file(registry, session);
                let Some(diff) = &mut self.diff else {
                    return Err(Refused("no diff is open: compare files first".to_owned()));
                };
                let mut outcome = DiffOutcome::default();
                match action {
                    "view.diff.swapSides" => diff.swap(),
                    "view.diff.next" | "view.diff.previous" => diff.step(action == "view.diff.next"),
                    "view.diff.whitespace" => {
                        let mode = match params.get("mode") {
                            Some(mode) => serde_json::from_value::<Whitespace>(mode.clone())
                                .map_err(|_| Refused("`mode` is one of `none`, `trim` and `ignore`".to_owned()))?,
                            None => {
                                let at = Whitespace::ALL.iter().position(|mode| *mode == diff.whitespace).unwrap_or(0);
                                Whitespace::ALL[(at + 1) % Whitespace::ALL.len()]
                            }
                        };
                        outcome.rehunk = diff.set_whitespace(mode);
                    }
                    "view.diff.viewer" => {
                        let unified = params.get("unified").and_then(Value::as_bool).unwrap_or(!diff.unified);
                        if diff.three.is_some() && unified {
                            return Err(Refused("three files are only shown side by side".to_owned()));
                        }
                        diff.unified = unified;
                        self.diff_unified = unified;
                    }
                    "view.diff.setText" => {
                        let after = match params.get("side").and_then(Value::as_str) {
                            Some("left") => false,
                            Some("right") => true,
                            _ => return Err(Refused("`side` is `left` or `right`".to_owned())),
                        };
                        let path = params.get("path").and_then(Value::as_str).map(PathBuf::from);
                        let (text, title) = match (params.get("text").and_then(Value::as_str), &path) {
                            (Some(text), _) => (text.to_owned(), None),
                            (None, Some(path)) => (
                                std::fs::read_to_string(path)
                                    .map_err(|error| Refused(format!("cannot read {}: {error}", path.display())))?,
                                Some(path.display().to_string()),
                            ),
                            (None, None) => {
                                return Err(Refused(
                                    "give the side's `text`, or the `path` of a file dropped there".to_owned(),
                                ));
                            }
                        };
                        let Some(write) = diff.set_side(after, text, title) else {
                            return Err(Refused("that side of the diff cannot be typed in".to_owned()));
                        };
                        outcome.write = write;
                        outcome.rehunk = true;
                    }
                    _ => return Err(Refused(format!("{action} is not a diff action"))),
                }
                self.after_diff(registry, session, outcome);
                Ok(self.diff.as_ref().map(DiffTab::state).unwrap_or_default())
            }
        }
    }
}

impl IdeView {
    pub(super) fn show_folder_diff(
        &mut self,
        ctx: &egui::Context,
        session: &Session,
        left: PathBuf,
        right: PathBuf,
        now: bool,
    ) {
        let mut tab = FolderDiffTab::new(left, right);
        if now {
            tab.compare_now();
        } else {
            tab.start(ctx);
        }
        self.folder_diff = Some(tab);
        self.bring_front(FrontKind::Folder, session);
        self.tree_focused = false;
        self.close_text_dialogs();
    }

    pub(super) fn folder_in_front(&self, session: &Session) -> bool {
        self.is_in_front(FrontKind::Folder, session)
    }

    pub(super) fn open_folder_entry(
        &mut self,
        registry: &Registry,
        session: &mut Session,
        relative: &str,
    ) -> Result<Value, ActionError> {
        use ActionError::{Dispatch, Refused};
        let Some(tab) = &self.folder_diff else { return Err(Refused("no folders are compared".to_owned())) };
        let Some(entry) = tab.entry(relative).cloned() else {
            return Err(Refused(format!("{relative} is not an entry of the compared folders")));
        };
        let (left, right) = (tab.left.join(&entry.relative), tab.right.join(&entry.relative));
        let file = |side: Option<&EntrySide>| side.is_none_or(|side| side.kind == EntryKind::File);
        if !file(entry.left.as_ref()) || !file(entry.right.as_ref()) {
            return Err(Refused(format!("{relative} is not a file on both sides; only files open in the diff")));
        }
        if entry.left.is_some() && entry.right.is_some() {
            return self.compare_paths(registry, session, &[left, right]);
        }
        let read = |path: &Path, present: bool| -> Result<String, ActionError> {
            if !present {
                return Ok(String::new());
            }
            std::fs::read_to_string(path).map_err(|error| Refused(format!("cannot read {}: {error}", path.display())))
        };
        let params = json!({
            "left": read(&left, entry.left.is_some())?,
            "right": read(&right, entry.right.is_some())?,
            "leftTitle": left.display().to_string(),
            "rightTitle": right.display().to_string(),
        });
        let result = registry.dispatch(session, "file.compare.texts", params).map_err(Dispatch)?;
        if let Ok(mut view) = serde_json::from_value::<DiffView>(result.clone()) {
            view.before_editable = false;
            view.after_editable = false;
            view.relative = relative.to_owned();
            self.show_diff(session, view);
        }
        Ok(result)
    }

    pub(super) fn synchronize_folders(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        items: Vec<SyncItem>,
        confirm: bool,
        agent: bool,
    ) -> Result<Value, ActionError> {
        use ActionError::{Dispatch, Refused};
        let Some(tab) = &self.folder_diff else { return Err(Refused("no folders are compared".to_owned())) };
        if items.is_empty() {
            return Err(Refused("nothing to synchronise: no entry shown (or selected) has an action".to_owned()));
        }
        let params = json!({ "left": tab.left, "right": tab.right, "items": items, "confirm": confirm });
        let result = registry.dispatch(session, "file.compare.synchronize", params).map_err(Dispatch)?;
        let asked: SyncResult = serde_json::from_value(result.clone()).unwrap_or_default();
        if !asked.needs_confirmation.is_empty() {
            if agent {
                let listed: Vec<String> = asked
                    .needs_confirmation
                    .iter()
                    .map(|item| format!("{} {}", folder_diff::action_name(Some(item.action)), item.relative))
                    .collect();
                return Err(Refused(format!(
                    "synchronising would overwrite or delete {}; nothing was done: run it again with `confirm: true`",
                    listed.join(", ")
                )));
            }
            if let Some(tab) = &mut self.folder_diff {
                tab.confirm = Some((items, asked.needs_confirmation.clone()));
            }
            return Ok(result);
        }
        if let Some(tab) = &mut self.folder_diff {
            if agent {
                tab.compare_now();
            } else {
                tab.start(ctx);
            }
        }
        Ok(result)
    }

    pub(super) fn folder_frame(&mut self, ui: &mut Ui, registry: &Registry, session: &mut Session) {
        let p = self.palette;
        let keys = !self.tree_focused
            && ui.memory(|memory| memory.focused().is_none())
            && !egui::Popup::is_any_open(ui.ctx())
            && self.folder_diff.as_ref().is_some_and(|tab| tab.confirm.is_none());
        let Some(tab) = &mut self.folder_diff else { return };
        let pass = ui.ctx().cumulative_pass_nr();
        let back = tab.drawn_at.is_some_and(|drawn| drawn + 1 < pass);
        tab.drawn_at = Some(pass);
        if back && !tab.comparing() {
            tab.start(ui.ctx());
        }
        let outcome = tab.show(ui, &p, keys);
        if let Some((items, risky)) = tab.confirm.clone() {
            match folder_diff::confirm_dialog(ui, &p, &risky) {
                Some(true) => {
                    tab.confirm = None;
                    if let Err(error) = self.synchronize_folders(ui.ctx(), registry, session, items, true, false) {
                        self.action_failed(ui, error);
                    }
                }
                Some(false) => tab.confirm = None,
                None => {}
            }
            return;
        }
        if outcome.refresh {
            tab.start(ui.ctx());
        }
        if let Some(all) = outcome.synchronize {
            let items = tab.items(all);
            if let Err(error) = self.synchronize_folders(ui.ctx(), registry, session, items, false, false) {
                self.action_failed(ui, error);
            }
        }
        if let Some(relative) = outcome.open
            && let Err(error) = self.open_folder_entry(registry, session, &relative)
        {
            self.action_failed(ui, error);
        }
    }

    fn action_failed(&mut self, ui: &Ui, error: ActionError) {
        match error {
            ActionError::Dispatch(error) => self.report(ui, error),
            ActionError::Refused(message) => self.post(ui, message),
        }
    }

    pub(super) fn folder_action(
        &mut self,
        ctx: &egui::Context,
        registry: &Registry,
        session: &mut Session,
        action: &str,
        params: &Value,
        agent: bool,
    ) -> Result<Value, ActionError> {
        use ActionError::Refused;
        if action == "view.folderDiff.compare" {
            let given: Vec<PathBuf> =
                params.get("paths").and_then(|paths| serde_json::from_value(paths.clone()).ok()).unwrap_or_default();
            let path = |key: &str| params.get(key).and_then(Value::as_str).map(PathBuf::from);
            let selected: Vec<PathBuf> =
                self.selection.items(&self.visible_rows).into_iter().filter(|path| path.is_dir()).collect();
            let folders = match (path("left"), path("right")) {
                (Some(left), Some(right)) => vec![left, right],
                _ if !given.is_empty() => given,
                _ => selected,
            };
            let [left, right] = folders.as_slice() else {
                return Err(Refused("Compare Directories needs two folders: `left` and `right`".to_owned()));
            };
            let resolve = |session: &mut Session, path: &Path| session.state_mut::<Workbench>().resolve(path);
            let (left, right) = (resolve(session, left), resolve(session, right));
            for folder in [&left, &right] {
                if !folder.is_dir() {
                    return Err(Refused(format!("{} is not a folder", folder.display())));
                }
            }
            self.show_folder_diff(ctx, session, left, right, agent);
            return Ok(self.folder_diff.as_ref().map(|tab| tab.agent_state(session)).unwrap_or_default());
        }
        let Some(tab) = &mut self.folder_diff else {
            return Err(Refused("no folders are compared: run view.folderDiff.compare first".to_owned()));
        };
        let wait = params.get("wait").and_then(Value::as_bool).unwrap_or(agent);
        if wait && tab.comparing() {
            tab.wait(std::time::Duration::from_secs(120));
        }
        match action {
            "view.folderDiff.refresh" => {
                if agent {
                    tab.compare_now();
                } else {
                    tab.start(ctx);
                }
            }
            "view.folderDiff.filter" => {
                if let Some(states) = params.get("states") {
                    let states: Vec<EntryState> = serde_json::from_value(states.clone()).map_err(|_| {
                        Refused("`states` lists `onlyLeft`, `onlyRight`, `different` and `equal`".to_owned())
                    })?;
                    for state in EntryState::ALL {
                        tab.set_shown(state, states.contains(&state));
                    }
                }
                if let Some(text) = params.get("text").and_then(Value::as_str) {
                    tab.set_filter(text);
                }
            }
            "view.folderDiff.select" => {
                let paths: Vec<String> = params
                    .get("paths")
                    .and_then(|paths| serde_json::from_value(paths.clone()).ok())
                    .ok_or_else(|| Refused("give the entries' `paths`, relative to the folders".to_owned()))?;
                tab.select(&paths);
            }
            "view.folderDiff.setAction" => {
                let relative = params.get("path").and_then(Value::as_str).unwrap_or_default().to_owned();
                let action = match params.get("action").and_then(Value::as_str) {
                    Some("none") => None,
                    Some(name) => Some(
                        serde_json::from_value::<SyncAction>(json!(name))
                            .map_err(|_| Refused("`action` is `toRight`, `toLeft`, `delete` or `none`".to_owned()))?,
                    ),
                    None => return Err(Refused("give the entry's `path` and its `action`".to_owned())),
                };
                if !tab.set_action(&relative, action) {
                    return Err(Refused(format!(
                        "{relative} cannot be given {}: an entry on one side copies over or is deleted, a different one copies either way",
                        folder_diff::action_name(action)
                    )));
                }
            }
            "view.folderDiff.synchronize" => {
                let all = params.get("all").and_then(Value::as_bool).unwrap_or(false);
                let items = tab.items(all);
                let confirm = params.get("confirm").and_then(Value::as_bool).unwrap_or(false);
                let result = self.synchronize_folders(ctx, registry, session, items, confirm, agent)?;
                let mut state = self.folder_diff.as_ref().map(|tab| tab.agent_state(session)).unwrap_or_default();
                state["synchronized"] = result;
                return Ok(state);
            }
            "view.folderDiff.open" => {
                let relative = params
                    .get("path")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| tab.lead().map(str::to_owned));
                let Some(relative) = relative else {
                    return Err(Refused("give the entry's `path`, or select one first".to_owned()));
                };
                if agent && self.folder_entry_hidden(session, &relative) {
                    return Err(Refused(format!("{relative} is hidden from agents")));
                }
                return self.open_folder_entry(registry, session, &relative);
            }
            _ => return Err(Refused(format!("{action} is not an action of Compare Directories"))),
        }
        Ok(self.folder_diff.as_ref().map(|tab| tab.agent_state(session)).unwrap_or_default())
    }

    fn folder_entry_hidden(&self, session: &Session, relative: &str) -> bool {
        let Some(tab) = &self.folder_diff else { return false };
        let patterns = session
            .state::<zen_workbench::hidden_files::HiddenFromAgents>()
            .map(|hidden| hidden.patterns().to_vec())
            .unwrap_or_default();
        let root = session.state::<Workbench>().and_then(Workbench::project);
        !patterns.is_empty()
            && zen_workbench::compare_folders::is_hidden_entry(&tab.left, &tab.right, relative, &patterns, root)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use egui::vec2;
    use serde_json::json;

    #[test]
    fn the_diff_viewers_draw_in_english_and_russian() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let [a, b, c] = ["a.go", "b.go", "c.go"].map(|name| dir.path().join(name));
        std::fs::write(&a, "package main\n\nfunc a() {\n\treturn 1\n}\n\nfunc keep() {}\n").unwrap();
        std::fs::write(&b, "package main\n\nfunc b() {\n    return 1\n}\n\nfunc keep() {}\n").unwrap();
        std::fs::write(&c, "package main\n\nfunc c() {\n    return 2\n}\n\nfunc keep() {}\n\nfunc more() {}\n")
            .unwrap();
        let mut registry = zen_core::Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = zen_core::Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let product = test_product;
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let paint = snapshots.is_some();
        let languages: &[&str] = if paint { &["en", "ru"] } else { &["en"] };
        for language in languages {
            zen_i18n::with_language(language, || {
                let mut view =
                    super::super::IdeView::new(std::sync::Arc::new(product()), crate::plugins::Plugins::new(None));
                let mut screen = crate::offscreen::Offscreen::new(vec2(1200.0, 520.0), 1.5);
                let mut shots = Vec::new();
                let act = |screen: &mut crate::offscreen::Offscreen,
                           view: &mut super::super::IdeView,
                           session: &mut zen_core::Session,
                           action: &str,
                           params| {
                    let ctx = screen.context().clone();
                    view.perform_action(&ctx, &registry, session, action, params, true).unwrap();
                    for _ in 0..3 {
                        screen.step(Vec::new(), |ui| view.show(ui, &registry, session));
                        view.requests.clear();
                    }
                    let image = paint.then(|| screen.frame(Vec::new(), |ui| view.show(ui, &registry, session)));
                    view.requests.clear();
                    image
                };
                shots.push((
                    "two",
                    act(&mut screen, &mut view, &mut session, "view.project.compareWith", json!({ "paths": [a, b] })),
                ));
                for events in click(egui::pos2(900.0, 250.0)) {
                    screen.step(events, |ui| view.show(ui, &registry, &mut session));
                    view.requests.clear();
                }
                let target = view.diff.as_ref().unwrap().jump_target();
                assert!(target.as_ref().is_some_and(|(path, line)| *path == b && *line > 0), "{target:?}");
                for (name, theme) in [
                    ("two-caret-light", zen_theme::ResolvedTheme::zen_light()),
                    ("two-caret-dark", zen_theme::ResolvedTheme::zen_dark()),
                ] {
                    view.set_theme(&theme);
                    view.palette.apply(screen.context());
                    screen.step(Vec::new(), |ui| view.show(ui, &registry, &mut session));
                    view.requests.clear();
                    let image = paint.then(|| screen.frame(Vec::new(), |ui| view.show(ui, &registry, &mut session)));
                    view.requests.clear();
                    shots.push((name, image));
                }
                act(&mut screen, &mut view, &mut session, "view.diff.whitespace", json!({ "mode": "trim" }));
                shots.push(("two-trim", act(&mut screen, &mut view, &mut session, "view.diff.next", json!({}))));
                shots.push((
                    "unified",
                    act(&mut screen, &mut view, &mut session, "view.diff.viewer", json!({ "unified": true })),
                ));
                act(&mut screen, &mut view, &mut session, "view.diff.viewer", json!({ "unified": false }));
                shots.push((
                    "three",
                    act(
                        &mut screen,
                        &mut view,
                        &mut session,
                        "view.project.compareWith",
                        json!({ "paths": [a, b, c] }),
                    ),
                ));
                registry.dispatch(&mut session, "file.document.open", json!({ "path": c })).unwrap();
                let clipboard = json!({ "path": c, "text": "package main\n\nfunc c() {\n\treturn 3\n}\n" });
                shots.push((
                    "clipboard",
                    act(&mut screen, &mut view, &mut session, "view.compare.clipboard", clipboard),
                ));
                let text = |session: &zen_core::Session| {
                    let workbench = session.state::<zen_workbench::Workbench>().unwrap();
                    workbench.documents().iter().find(|document| document.path() == c).unwrap().text().to_owned()
                };
                let original = text(&session);
                let at = egui::pos2(900.0, 165.0);
                let button = |pressed| egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                };
                let undo = egui::Event::Key {
                    key: egui::Key::Z,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::COMMAND,
                };
                let typed: Vec<Vec<egui::Event>> = vec![
                    vec![egui::Event::PointerMoved(at), button(true)],
                    vec![button(false)],
                    vec![egui::Event::Text("X".to_owned())],
                    Vec::new(),
                ];
                for events in typed {
                    screen.step(events, |ui| view.show(ui, &registry, &mut session));
                    view.requests.clear();
                }
                assert_ne!(text(&session), original, "typing in the file's side changes its document");
                assert!(text(&session).contains('X'), "{}", text(&session));
                for events in [vec![undo], Vec::new()] {
                    screen.step(events, |ui| view.show(ui, &registry, &mut session));
                    view.requests.clear();
                }
                assert_eq!(text(&session), original, "⌘Z in the diff undoes the typing there");
                act(&mut screen, &mut view, &mut session, "view.diff.blank", json!({}));
                act(
                    &mut screen,
                    &mut view,
                    &mut session,
                    "view.diff.setText",
                    json!({ "side": "left", "text": "alpha\nbeta\ngamma\n" }),
                );
                let right = json!({ "side": "right", "text": "alpha\nBETA\ngamma\ndelta\n" });
                shots.push(("blank", act(&mut screen, &mut view, &mut session, "view.diff.setText", right)));
                shots.push((
                    "blank-swapped",
                    act(&mut screen, &mut view, &mut session, "view.diff.swapSides", json!({})),
                ));
                let diff = view.diff.as_ref().unwrap();
                assert!(diff.swapped && diff.typed());
                assert_eq!(diff.state()["texts"], json!(["alpha\nBETA\ngamma\ndelta\n", "alpha\nbeta\ngamma\n"]));
                assert_eq!(diff.change_count(), 2);
                for (name, image) in &shots {
                    if let (Some(folder), Some(image)) = (&snapshots, image) {
                        assert_eq!(image.size, [1800, 780], "{name}");
                        crate::offscreen::write_png(&folder.join(format!("diff-{name}-{language}.png")), image)
                            .unwrap();
                    }
                }
            });
        }
    }

    fn test_product() -> super::super::IdeProduct {
        super::super::IdeProduct {
            name: "Test",
            id: "test",
            version: "0.0.0",
            highlighters: Vec::new(),
            plugins: None,
            mcp_port: 0,
            modules: Vec::new(),
            areas: Vec::new(),
            settings_pages: Vec::new(),
            code_styles: Vec::new(),
            folding_options: Vec::new(),
            debugger: None,
            project_models: Vec::new(),
            outliners: Vec::new(),
            run_providers: Vec::new(),
            language_servers: Vec::new(),
            templates: Vec::new(),
            inspectors: Vec::new(),
        }
    }

    fn put(root: &std::path::Path, relative: &str, text: &str) {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn click(at: egui::Pos2) -> Vec<Vec<egui::Event>> {
        let button = |pressed| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        vec![vec![egui::Event::PointerMoved(at), button(true)], vec![button(false)]]
    }

    fn key(key: egui::Key) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }
    }

    fn entry_state(view: &super::super::IdeView, relative: &str) -> Option<serde_json::Value> {
        let state = view.folder_diff.as_ref()?.state();
        state["entries"].as_array()?.iter().find(|entry| entry["relative"] == relative).cloned()
    }

    #[test]
    fn compare_directories_tells_agents_nothing_of_hidden_files() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let mut registry = zen_core::Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (left, right) = (dir.path().join("left"), dir.path().join("right"));
        put(&left, ".env", "TOKEN=a\n");
        put(&right, ".env", "TOKEN=b\n");
        put(&left, "main.go", "package main\n");
        let mut session = zen_core::Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        session.state_mut::<zen_workbench::hidden_files::HiddenFromAgents>().set(&[".env".to_owned()]);
        let mut view =
            super::super::IdeView::new(std::sync::Arc::new(test_product()), crate::plugins::Plugins::new(None));
        let ctx = egui::Context::default();
        let compare = json!({ "left": left, "right": right });
        let told =
            view.perform_action(&ctx, &registry, &mut session, "view.folderDiff.compare", compare, true).unwrap();
        assert!(!told.to_string().contains(".env"), "{told}");
        assert_eq!(told["hiddenFiles"], 1);
        assert_eq!(told["counts"]["different"], 0);
        assert!(entry_state(&view, ".env").is_some(), "the user still sees it");
        let opened = json!({ "path": ".env" });
        assert!(view.perform_action(&ctx, &registry, &mut session, "view.folderDiff.open", opened, true).is_err());
    }

    #[test]
    fn compare_directories_draws_and_works_with_the_pointer_in_english_and_russian() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let mut registry = zen_core::Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
        let languages: &[&str] = if snapshots.is_some() { &["en", "ru"] } else { &["en"] };
        for language in languages {
            let dir = tempfile::tempdir().unwrap();
            let (left, right) = (dir.path().join("left"), dir.path().join("right"));
            put(&left, "README.md", "# Zen\n");
            put(&right, "README.md", "# Zen\n");
            put(&left, "go.mod", "module zen\n");
            put(&right, "go.mod", "module zen\n\ngo 1.24\n");
            put(&left, "cmd/main.go", "package main\n");
            put(&left, "internal/old.go", "package internal\n");
            put(&right, "internal/new.go", "package internal\n");
            put(&right, "docs/guide.md", "# Guide\n");
            put(&left, ".git/HEAD", "ref: refs/heads/main\n");
            let mut session = zen_core::Session::new();
            registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
            zen_i18n::with_language(language, || {
                let mut view =
                    super::super::IdeView::new(std::sync::Arc::new(test_product()), crate::plugins::Plugins::new(None));
                let mut screen = crate::offscreen::Offscreen::new(vec2(1300.0, 560.0), 1.5);
                let mut shots = Vec::new();
                let ctx = screen.context().clone();
                let compare = json!({ "left": left, "right": right });
                view.perform_action(&ctx, &registry, &mut session, "view.folderDiff.compare", compare, false).unwrap();
                for _ in 0..300 {
                    screen.step(Vec::new(), |ui| view.show(ui, &registry, &mut session));
                    view.requests.clear();
                    if !view.folder_diff.as_ref().unwrap().comparing() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                let frame = |screen: &mut crate::offscreen::Offscreen,
                             view: &mut super::super::IdeView,
                             session: &mut zen_core::Session,
                             events: Vec<Vec<egui::Event>>| {
                    for events in events {
                        screen.step(events, |ui| view.show(ui, &registry, session));
                        view.requests.clear();
                    }
                    for _ in 0..2 {
                        screen.step(Vec::new(), |ui| view.show(ui, &registry, session));
                        view.requests.clear();
                    }
                    let image =
                        snapshots.is_some().then(|| screen.frame(Vec::new(), |ui| view.show(ui, &registry, session)));
                    view.requests.clear();
                    image
                };
                shots.push(("folders", frame(&mut screen, &mut view, &mut session, Vec::new())));
                assert!(view.folder_in_front(&session));
                let state = view.folder_diff.as_ref().unwrap().state();
                assert_eq!(
                    state["counts"],
                    json!({ "onlyLeft": 2, "onlyRight": 2, "different": 1, "equal": 1 }),
                    "{state}"
                );
                assert_eq!(state["rows"], 5);

                let hits = view.folder_diff.as_ref().unwrap().hits;
                frame(&mut screen, &mut view, &mut session, click(hits.toggles[3].center()));
                assert_eq!(view.folder_diff.as_ref().unwrap().state()["rows"], 6, "the Equal toggle shows equal files");
                frame(&mut screen, &mut view, &mut session, click(hits.toggles[1].center()));
                assert_eq!(view.folder_diff.as_ref().unwrap().state()["rows"], 4, "Right only hides them");
                frame(&mut screen, &mut view, &mut session, click(hits.toggles[1].center()));

                let mut typing = click(hits.filter.center());
                typing.push(vec![egui::Event::Text("*.go & !new* | *.mod".to_owned())]);
                shots.push(("filtered", frame(&mut screen, &mut view, &mut session, typing)));
                let filtered = view.folder_diff.as_ref().unwrap().state();
                let shown: Vec<&str> = filtered["entries"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|entry| entry["relative"].as_str())
                    .collect();
                assert_eq!(shown, ["go.mod", "internal/old.go"], "a folder on one side is one row: {filtered}");
                view.folder_diff.as_mut().unwrap().set_filter("");
                view.tree_focused = false;
                ctx.memory_mut(|memory| memory.stop_text_input());
                frame(&mut screen, &mut view, &mut session, Vec::new());

                let row_y = |view: &super::super::IdeView, relative: &str| {
                    let tab = view.folder_diff.as_ref().unwrap();
                    let state = tab.state();
                    let row = state["entries"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .position(|entry| entry["relative"] == relative)
                        .unwrap();
                    tab.hits.body.top() + row as f32 * 22.0 + 11.0
                };
                let body = view.folder_diff.as_ref().unwrap().hits.body;
                let cell = egui::pos2(body.center().x, row_y(&view, "internal/old.go"));
                frame(&mut screen, &mut view, &mut session, click(cell));
                assert_eq!(
                    entry_state(&view, "internal/old.go").unwrap()["action"],
                    "delete",
                    "a click on → makes it ✕"
                );
                shots.push(("actions", frame(&mut screen, &mut view, &mut session, Vec::new())));

                let sync_all = view.folder_diff.as_ref().unwrap().hits.sync[0].center();
                let mut opening = click(sync_all);
                opening.extend(vec![Vec::new(); 20]);
                shots.push(("confirm", frame(&mut screen, &mut view, &mut session, opening)));
                let confirming = view.folder_diff.as_ref().unwrap().state()["confirming"].clone();
                assert_eq!(
                    confirming,
                    json!([{ "relative": "go.mod", "action": "toRight" }, { "relative": "internal/old.go", "action": "delete" }]),
                    "Synchronize All asks first, naming what it overwrites or deletes"
                );
                assert!(
                    left.join("internal/old.go").exists() && !left.join("docs").exists(),
                    "nothing done before Enter"
                );
                frame(&mut screen, &mut view, &mut session, vec![vec![key(egui::Key::Enter)]]);
                for _ in 0..300 {
                    frame(&mut screen, &mut view, &mut session, Vec::new());
                    if !view.folder_diff.as_ref().unwrap().comparing() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                assert!(!left.join("internal/old.go").exists(), "the ✕ row is deleted");
                assert_eq!(
                    std::fs::read_to_string(right.join("go.mod")).unwrap(),
                    "module zen\n",
                    "the different file is copied over"
                );
                assert!(left.join("docs/guide.md").exists() && right.join("cmd/main.go").exists());
                assert!(!right.join(".git").exists(), "ignored folders are not copied");
                let after = view.folder_diff.as_ref().unwrap().state();
                assert_eq!(
                    after["counts"],
                    json!({ "onlyLeft": 0, "onlyRight": 0, "different": 0, "equal": 5 }),
                    "compared again: {after}"
                );
                assert_eq!(after["confirming"], serde_json::Value::Null);

                put(&right, "go.mod", "module zen // changed\n");
                view.folder_diff.as_mut().unwrap().compare_now();
                frame(&mut screen, &mut view, &mut session, Vec::new());
                let row = egui::pos2(body.left() + 60.0, row_y(&view, "go.mod"));
                let mut double = vec![Vec::new(); 30];
                double.extend(click(row));
                double.extend(click(row));
                frame(&mut screen, &mut view, &mut session, double);
                assert!(
                    view.diff_in_front(&session),
                    "a double-click opens the file diff: {} {:?} {}",
                    view.folder_diff.as_ref().unwrap().state(),
                    row,
                    view.diff.is_some()
                );
                assert_eq!(
                    view.diff.as_ref().unwrap().state()["titles"][1],
                    json!(right.join("go.mod").display().to_string())
                );
                shots.push(("file-diff", frame(&mut screen, &mut view, &mut session, Vec::new())));

                put(&left, "added.txt", "new\n");
                view.diff = None;
                for _ in 0..300 {
                    frame(&mut screen, &mut view, &mut session, Vec::new());
                    if !view.folder_diff.as_ref().unwrap().comparing() {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                assert!(view.folder_in_front(&session), "closing the file diff shows the folders again");
                assert_eq!(
                    entry_state(&view, "added.txt").map(|entry| entry["state"].clone()),
                    Some(json!("onlyLeft")),
                    "coming back compares the folders again"
                );
                ctx.memory_mut(|memory| memory.stop_text_input());
                view.folder_diff.as_mut().unwrap().select(&["go.mod".to_owned()]);
                frame(
                    &mut screen,
                    &mut view,
                    &mut session,
                    vec![vec![key(egui::Key::ArrowDown)], vec![key(egui::Key::Enter)]],
                );
                assert!(view.diff_in_front(&session), "↓ and Enter open the next file's diff");
                assert_eq!(
                    view.diff.as_ref().unwrap().state()["titles"][0],
                    json!(left.join("internal/new.go").display().to_string())
                );
                for (name, image) in &shots {
                    if let (Some(folder), Some(image)) = (&snapshots, image) {
                        crate::offscreen::write_png(&folder.join(format!("folder-diff-{name}-{language}.png")), image)
                            .unwrap();
                    }
                }
            });
        }
    }

    #[test]
    fn a_large_tree_is_compared_in_the_background_and_the_list_stays_responsive() {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        let mut registry = zen_core::Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (left, right) = (dir.path().join("left"), dir.path().join("right"));
        for folder in 0..60 {
            for file in 0..90 {
                let relative = format!("pkg{folder:02}/file{file:03}.go");
                let text = format!("package pkg{folder}\n// {file}\n{}\n", "x".repeat(400));
                put(&left, &relative, &text);
                let other = if file % 10 == 0 { text.replace("// ", "//!") } else { text.clone() };
                if file % 30 != 1 {
                    put(&right, &relative, &other);
                }
            }
            put(&right, &format!("pkg{folder:02}/extra.go"), "package extra\n");
        }
        let mut session = zen_core::Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let mut view =
            super::super::IdeView::new(std::sync::Arc::new(test_product()), crate::plugins::Plugins::new(None));
        let mut screen = crate::offscreen::Offscreen::new(vec2(1300.0, 700.0), 1.0);
        let ctx = screen.context().clone();
        let started = std::time::Instant::now();
        let params = json!({ "left": left, "right": right });
        view.perform_action(&ctx, &registry, &mut session, "view.folderDiff.compare", params, false).unwrap();
        let opened = started.elapsed();
        let mut frames_while_comparing = 0;
        let mut slowest = std::time::Duration::ZERO;
        while view.folder_diff.as_ref().unwrap().comparing() && started.elapsed() < std::time::Duration::from_secs(120)
        {
            let frame = std::time::Instant::now();
            screen.step(Vec::new(), |ui| view.show(ui, &registry, &mut session));
            view.requests.clear();
            slowest = slowest.max(frame.elapsed());
            frames_while_comparing += 1;
        }
        let compared = started.elapsed();
        let timing = std::env::var_os("ZEN_TIMING_CHECKS").is_some();
        if timing {
            assert!(opened < std::time::Duration::from_millis(250), "opening the tab does not wait: {opened:?}");
            assert!(frames_while_comparing > 0, "the window drew while the comparison ran ({compared:?})");
            assert!(slowest < std::time::Duration::from_millis(500), "a frame while comparing took {slowest:?}");
        }
        assert!(!view.folder_diff.as_ref().unwrap().comparing(), "the comparison ended ({compared:?})");
        let state = view.folder_diff.as_ref().unwrap().state();
        assert_eq!(state["counts"]["onlyLeft"], 60 * 3, "{}", state["counts"]);
        assert_eq!(state["counts"]["onlyRight"], 60);
        assert_eq!(state["counts"]["different"], 60 * 9);
        assert_eq!(state["counts"]["equal"], 60 * 78);
        let tab = view.folder_diff.as_mut().unwrap();
        for each in zen_workbench::compare_folders::EntryState::ALL {
            tab.set_shown(each, true);
        }
        assert_eq!(tab.state()["rows"], 60 * 91);
        assert_eq!(tab.state()["truncated"], true);
        let mut drawn = std::time::Duration::ZERO;
        for step in 0..10 {
            let events = if step % 2 == 0 {
                vec![egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: vec2(0.0, -4000.0),
                    modifiers: egui::Modifiers::NONE,
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Unknown,
                }]
            } else {
                Vec::new()
            };
            let frame = std::time::Instant::now();
            screen.step(events, |ui| view.show(ui, &registry, &mut session));
            view.requests.clear();
            drawn = drawn.max(frame.elapsed());
        }
        if timing {
            assert!(drawn < std::time::Duration::from_millis(500), "a frame of 5,460 rows took {drawn:?}");
        }
    }
}
