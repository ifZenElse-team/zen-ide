use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use super::commit::relative;
use super::{ChangeInfo, StatusList, repository, vcs};

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListChangelists)?;
    registry.register(CreateChangelist)?;
    registry.register(EditChangelist)?;
    registry.register(DeleteChangelist)?;
    registry.register(SetActiveChangelist)?;
    registry.register(MoveToChangelist)?;
    Ok(())
}

#[derive(Default)]
pub struct LocalChanges {
    folder: Option<PathBuf>,
    memory: HashMap<PathBuf, Lists>,
}

impl LocalChanges {
    pub fn set_folder(&mut self, folder: PathBuf) {
        self.folder = Some(folder);
    }

    pub(super) fn shelf_folder(&self, root: &Path) -> PathBuf {
        let base = self.folder.clone().unwrap_or_else(std::env::temp_dir);
        base.join("vcs-shelf").join(super::history::key(root))
    }

    fn file(&self, root: &Path) -> Option<PathBuf> {
        let folder = self.folder.as_ref()?;
        Some(folder.join("vcs-changelists").join(format!("{}.json", super::history::key(root))))
    }

    fn load(&self, root: &Path) -> Lists {
        let lists = match self.file(root) {
            Some(file) => std::fs::read(file)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Lists>(&bytes).ok())
                .filter(|lists| lists.root == root)
                .unwrap_or_default(),
            None => self.memory.get(root).cloned().unwrap_or_default(),
        };
        lists.normalized(root)
    }

    fn store(&mut self, lists: Lists) {
        match self.file(&lists.root) {
            Some(file) => {
                if let Some(folder) = file.parent() {
                    let _ = std::fs::create_dir_all(folder);
                }
                if let Ok(bytes) = serde_json::to_vec_pretty(&lists) {
                    let partial = file.with_extension("json.partial");
                    if std::fs::write(&partial, bytes).is_ok() {
                        let _ = std::fs::rename(&partial, &file);
                    }
                }
            }
            None => {
                self.memory.insert(lists.root.clone(), lists);
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct List {
    name: String,
    comment: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Lists {
    root: PathBuf,
    active: String,
    lists: Vec<List>,
    files: BTreeMap<String, String>,
}

impl Lists {
    fn normalized(mut self, root: &Path) -> Self {
        self.root = root.to_owned();
        if self.lists.is_empty() {
            self.lists.push(List { name: t!("vcs-changelist-default"), comment: String::new() });
        }
        if !self.lists.iter().any(|list| list.name == self.active) {
            self.active = self.lists[0].name.clone();
        }
        let names: BTreeSet<String> = self.lists.iter().map(|list| list.name.clone()).collect();
        let active = self.active.clone();
        for list in self.files.values_mut() {
            if !names.contains(list) {
                list.clone_from(&active);
            }
        }
        self
    }

    pub(super) fn has(&self, name: &str) -> bool {
        self.lists.iter().any(|list| list.name == name)
    }

    pub(super) fn active(&self) -> &str {
        &self.active
    }

    pub(super) fn sync(&mut self, changed: &BTreeSet<String>) -> bool {
        let before = self.files.len();
        self.files.retain(|path, _| changed.contains(path));
        let mut moved = self.files.len() != before;
        for path in changed {
            if !self.files.contains_key(path) {
                self.files.insert(path.clone(), self.active.clone());
                moved = true;
            }
        }
        moved
    }

    pub(super) fn list_of(&self, path: &str) -> &str {
        self.files.get(path).map_or(self.active.as_str(), String::as_str)
    }

    pub(super) fn assign(&mut self, paths: &[String], name: &str) {
        for path in paths {
            self.files.insert(path.clone(), name.to_owned());
        }
    }
}

fn name_of(name: &str, lists: &Lists, except: Option<&str>) -> Result<String, CommandError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(CommandError::new(t!("vcs-error-changelist-name")));
    }
    if lists.has(name) && except != Some(name) {
        return Err(CommandError::new(t!("vcs-error-changelist-exists", name = name)));
    }
    Ok(name.to_owned())
}

fn missing(name: &str) -> CommandError {
    CommandError::new(t!("vcs-error-no-changelist", name = name))
}

/// A changelist with its files.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ChangelistInfo {
    /// Its name, which names it in the other commands.
    pub name: String,
    /// Its comment (Edit Changelist), the message to commit it with.
    pub comment: String,
    /// Whether new changes go into it.
    pub active: bool,
    /// Its changed files, sorted by path.
    pub files: Vec<ChangeInfo>,
}

/// The changelists and the files in no list.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ChangelistReport {
    /// The lists, in order; there is always one.
    pub lists: Vec<ChangelistInfo>,
    /// The unversioned files, which are in no list.
    pub unversioned: Vec<ChangeInfo>,
    /// The status revision it was read at (vcs.status.list).
    pub revision: u64,
}

pub(super) fn current(session: &mut Session) -> Result<(Lists, ChangelistReport), CommandError> {
    current_with(session, true)
}

fn current_with(session: &mut Session, wait: bool) -> Result<(Lists, ChangelistReport), CommandError> {
    let repository = repository(session)?;
    if wait {
        vcs(session).settle();
    }
    let status = StatusList.run(session, NoParams {})?;
    let root = repository.root().to_owned();
    let local = session.state_mut::<LocalChanges>();
    let mut lists = local.load(&root);
    let (unversioned, tracked): (Vec<ChangeInfo>, Vec<ChangeInfo>) =
        status.changes.into_iter().partition(|change| change.status == "unversioned");
    let changed: BTreeSet<String> = tracked.iter().map(|change| change.relative.clone()).collect();
    if lists.sync(&changed) && !status.computing {
        local.store(lists.clone());
    }
    let mut report = ChangelistReport { unversioned, revision: status.revision, ..ChangelistReport::default() };
    for list in &lists.lists {
        let files =
            tracked.iter().filter(|change| lists.list_of(&change.relative) == list.name).cloned().collect::<Vec<_>>();
        report.lists.push(ChangelistInfo {
            name: list.name.clone(),
            comment: list.comment.clone(),
            active: list.name == lists.active,
            files,
        });
    }
    Ok((lists, report))
}

fn change(
    session: &mut Session,
    edit: impl FnOnce(&mut Lists) -> Result<(), CommandError>,
) -> Result<ChangelistReport, CommandError> {
    let (mut lists, _) = current(session)?;
    edit(&mut lists)?;
    session.state_mut::<LocalChanges>().store(lists);
    current(session).map(|(_, report)| report)
}

pub(super) fn assign(session: &mut Session, root: &Path, paths: &[String], name: &str) {
    let local = session.state_mut::<LocalChanges>();
    let mut lists = local.load(root);
    if lists.has(name) {
        lists.assign(paths, name);
        local.store(lists);
    }
}

pub(crate) fn files_of(session: &mut Session, name: &str) -> Result<Vec<String>, CommandError> {
    let (_, report) = current(session)?;
    let list = report.lists.into_iter().find(|list| list.name == name).ok_or_else(|| missing(name))?;
    Ok(list.files.into_iter().map(|file| file.relative).collect())
}

pub(crate) fn active(session: &mut Session) -> Result<String, CommandError> {
    let (lists, _) = current(session)?;
    Ok(lists.active)
}

/// How to list the changelists.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// Wait for the status being computed (the default); the window does not.
    pub wait: bool,
}

impl Default for ListParams {
    fn default() -> Self {
        ListParams { wait: true }
    }
}

pub struct ListChangelists;

impl Command for ListChangelists {
    type Params = ListParams;
    type Output = ChangelistReport;
    const ID: &'static str = "vcs.changelist.list";
    const TITLE: &'static str = "Changelists";
    const DESCRIPTION: &'static str = "Lists the changelists of the Commit tool window, in order: each with its name, comment, \
         whether it is active (new changes go into it) and its changed files; and the unversioned \
         files, which are in no list. There is always a list (\"Changes\" at first). It waits for the \
         status being computed unless `wait` is false.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<ChangelistReport, CommandError> {
        current_with(session, params.wait).map(|(_, report)| report)
    }
}

/// A new changelist.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CreateParams {
    /// Its name; names are unique.
    pub name: String,
    /// Its comment.
    pub comment: String,
    /// Make it the active list.
    pub active: bool,
    /// Changed files to move into it (absolute or relative to the repository).
    pub paths: Vec<PathBuf>,
}

pub struct CreateChangelist;

impl Command for CreateChangelist {
    type Params = CreateParams;
    type Output = ChangelistReport;
    const ID: &'static str = "vcs.changelist.create";
    const TITLE: &'static str = "New Changelist…";
    const DESCRIPTION: &'static str = "Adds a changelist with a unique `name` and an optional `comment`; with `active`, new \
         changes go into it; `paths` are changed files moved into it. Answers with the lists.";

    fn run(&self, session: &mut Session, params: CreateParams) -> Result<ChangelistReport, CommandError> {
        let repository = repository(session)?;
        let paths =
            params.paths.iter().map(|path| relative(&repository, path)).collect::<Result<Vec<_>, CommandError>>()?;
        change(session, |lists| {
            let name = name_of(&params.name, lists, None)?;
            lists.lists.push(List { name: name.clone(), comment: params.comment.trim().to_owned() });
            lists.assign(&paths, &name);
            if params.active {
                lists.active = name;
            }
            Ok(())
        })
    }
}

/// A list's new name or comment.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct EditParams {
    /// The list.
    pub name: String,
    /// Its new name; it keeps its name when absent.
    pub new_name: Option<String>,
    /// Its new comment; it keeps its comment when absent.
    pub comment: Option<String>,
    /// Make it the active list.
    pub active: bool,
}

pub struct EditChangelist;

impl Command for EditChangelist {
    type Params = EditParams;
    type Output = ChangelistReport;
    const ID: &'static str = "vcs.changelist.edit";
    const TITLE: &'static str = "Edit Changelist…";
    const DESCRIPTION: &'static str = "Renames the changelist `name` to `newName` (unique; its files stay in it) and/or sets its \
         `comment`; with `active`, makes it the active list. Answers with the lists.";

    fn run(&self, session: &mut Session, params: EditParams) -> Result<ChangelistReport, CommandError> {
        change(session, |lists| {
            let old = params.name.trim().to_owned();
            if !lists.has(&old) {
                return Err(missing(&old));
            }
            let new = match &params.new_name {
                Some(new) => name_of(new, lists, Some(&old))?,
                None => old.clone(),
            };
            for list in &mut lists.lists {
                if list.name == old {
                    list.name.clone_from(&new);
                    if let Some(comment) = &params.comment {
                        list.comment = comment.trim().to_owned();
                    }
                }
            }
            for list in lists.files.values_mut() {
                if *list == old {
                    list.clone_from(&new);
                }
            }
            if lists.active == old || params.active {
                lists.active = new;
            }
            Ok(())
        })
    }
}

/// A list.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListRef {
    /// The list's name.
    pub name: String,
}

pub struct DeleteChangelist;

impl Command for DeleteChangelist {
    type Params = ListRef;
    type Output = ChangelistReport;
    const ID: &'static str = "vcs.changelist.delete";
    const TITLE: &'static str = "Delete Changelist";
    const DESCRIPTION: &'static str = "Deletes the changelist `name`; its files move to the active list (their changes stay). \
         The active list cannot be deleted: make another one active first. Answers with the lists.";

    fn run(&self, session: &mut Session, params: ListRef) -> Result<ChangelistReport, CommandError> {
        change(session, |lists| {
            let name = params.name.trim();
            if !lists.has(name) {
                return Err(missing(name));
            }
            if lists.active == name {
                return Err(CommandError::new(t!("vcs-error-changelist-active", name = name)));
            }
            lists.lists.retain(|list| list.name != name);
            let active = lists.active.clone();
            for list in lists.files.values_mut() {
                if list == name {
                    list.clone_from(&active);
                }
            }
            Ok(())
        })
    }
}

pub struct SetActiveChangelist;

impl Command for SetActiveChangelist {
    type Params = ListRef;
    type Output = ChangelistReport;
    const ID: &'static str = "vcs.changelist.setActive";
    const TITLE: &'static str = "Set Active Changelist";
    const DESCRIPTION: &'static str =
        "Makes the changelist `name` the active one: new changes go into it. Answers with the lists.";

    fn run(&self, session: &mut Session, params: ListRef) -> Result<ChangelistReport, CommandError> {
        change(session, |lists| {
            let name = params.name.trim();
            if !lists.has(name) {
                return Err(missing(name));
            }
            lists.active = name.to_owned();
            Ok(())
        })
    }
}

/// Files to move to another list.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct MoveParams {
    /// The changed files (absolute or relative to the repository).
    pub paths: Vec<PathBuf>,
    /// The list to move them to.
    pub to: String,
}

pub struct MoveToChangelist;

impl Command for MoveToChangelist {
    type Params = MoveParams;
    type Output = ChangelistReport;
    const ID: &'static str = "vcs.changelist.move";
    const TITLE: &'static str = "Move to Another Changelist…";
    const DESCRIPTION: &'static str = "Moves changed files (`paths`) to the changelist `to`; unversioned files are in no list \
         and are refused. Answers with the lists.";

    fn run(&self, session: &mut Session, params: MoveParams) -> Result<ChangelistReport, CommandError> {
        let repository = repository(session)?;
        let paths =
            params.paths.iter().map(|path| relative(&repository, path)).collect::<Result<Vec<_>, CommandError>>()?;
        if paths.is_empty() {
            return Err(CommandError::new(t!("vcs-error-no-files")));
        }
        change(session, |lists| {
            let to = params.to.trim();
            if !lists.has(to) {
                return Err(missing(to));
            }
            if let Some(path) = paths.iter().find(|path| !lists.files.contains_key(*path)) {
                return Err(CommandError::new(t!("vcs-error-not-in-changelist", path = path.as_str())));
            }
            lists.assign(&paths, to);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::testing::*;
    use super::*;

    fn set(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

    #[test]
    fn new_changes_go_to_the_active_list_and_leave_when_unchanged() {
        isolate();
        zen_i18n::set_language(Some("en"));
        let root = Path::new("/r");
        let mut lists = Lists::default().normalized(root);
        assert_eq!(lists.active, "Changes");
        assert!(lists.sync(&set(&["a.go", "b.go"])));
        lists.lists.push(List { name: "Feature".into(), comment: String::new() });
        lists.assign(&["b.go".to_owned()], "Feature");
        lists.active = "Feature".into();
        assert!(lists.sync(&set(&["a.go", "b.go", "c.go"])));
        assert_eq!(
            (lists.list_of("a.go"), lists.list_of("b.go"), lists.list_of("c.go")),
            ("Changes", "Feature", "Feature")
        );
        assert!(lists.sync(&set(&["a.go", "c.go"])));
        lists.active = "Changes".into();
        lists.sync(&set(&["a.go", "b.go", "c.go"]));
        assert_eq!(lists.list_of("b.go"), "Changes");
        assert!(!lists.sync(&set(&["a.go", "b.go", "c.go"])), "nothing to follow");
        lists.lists.retain(|list| list.name != "Feature");
        let lists = lists.normalized(root);
        assert_eq!(lists.list_of("c.go"), "Changes");
    }

    #[test]
    fn lists_are_created_renamed_moved_between_activated_deleted_and_kept() {
        let dir = test_repository();
        zen_i18n::set_language(Some("en"));
        let root = dir.path().canonicalize().unwrap();
        let data = tempfile::tempdir().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        session.state_mut::<LocalChanges>().set_folder(data.path().to_owned());
        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        std::fs::write(root.join("b.txt"), "new\n").unwrap();
        git(&root, &["add", "b.txt"]);
        std::fs::write(root.join("new.txt"), "untracked\n").unwrap();
        registry.dispatch(&mut session, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        let s = &mut session;
        let run = |s: &mut Session, id: &str, params: serde_json::Value| registry.dispatch(s, id, params);
        let names = |report: &serde_json::Value| -> Vec<(String, bool, Vec<String>)> {
            report["lists"]
                .as_array()
                .unwrap()
                .iter()
                .map(|list| {
                    let files = list["files"].as_array().unwrap().iter();
                    (
                        list["name"].as_str().unwrap().to_owned(),
                        list["active"].as_bool().unwrap(),
                        files.map(|file| file["relative"].as_str().unwrap().to_owned()).collect(),
                    )
                })
                .collect()
        };
        let report = run(s, "vcs.changelist.list", json!({})).unwrap();
        assert_eq!(names(&report), [("Changes".to_owned(), true, vec!["a.txt".to_owned(), "b.txt".to_owned()])]);
        assert_eq!(report["unversioned"][0]["relative"], "new.txt", "in no list");

        let report = run(s, "vcs.changelist.create", json!({ "name": "Feature", "paths": ["b.txt"] })).unwrap();
        assert_eq!(names(&report)[1], ("Feature".to_owned(), false, vec!["b.txt".to_owned()]));
        assert!(run(s, "vcs.changelist.create", json!({ "name": " Feature " })).is_err(), "names are unique");
        assert!(run(s, "vcs.changelist.create", json!({ "name": " " })).is_err());
        assert!(run(s, "vcs.changelist.move", json!({ "paths": ["new.txt"], "to": "Feature" })).is_err());
        assert!(run(s, "vcs.changelist.move", json!({ "paths": ["a.txt"], "to": "Nowhere" })).is_err());
        let report = run(s, "vcs.changelist.move", json!({ "paths": [root.join("a.txt")], "to": "Feature" })).unwrap();
        assert_eq!(names(&report)[1].2, ["a.txt", "b.txt"]);
        let report =
            run(s, "vcs.changelist.edit", json!({ "name": "Feature", "newName": "Login", "comment": "Fix login" }))
                .unwrap();
        assert_eq!(report["lists"][1]["comment"], "Fix login");
        assert_eq!(names(&report)[1].0, "Login");
        run(s, "vcs.changelist.setActive", json!({ "name": "Login" })).unwrap();
        assert!(run(s, "vcs.changelist.delete", json!({ "name": "Login" })).is_err(), "the active list stays");

        std::fs::write(root.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        std::fs::write(root.join("c.txt"), "c\n").unwrap();
        git(&root, &["add", "c.txt"]);
        registry.dispatch(s, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        let report = run(s, "vcs.changelist.list", json!({})).unwrap();
        assert_eq!(names(&report)[1], ("Login".to_owned(), true, vec!["b.txt".to_owned(), "c.txt".to_owned()]));
        let (registry, mut again) = super::super::tests::session(&root);
        again.state_mut::<LocalChanges>().set_folder(data.path().to_owned());
        let report = registry.dispatch(&mut again, "vcs.changelist.list", json!({})).unwrap();
        assert_eq!(names(&report)[1].0, "Login");
        assert!(names(&report)[1].1);

        registry.dispatch(&mut again, "vcs.changelist.setActive", json!({ "name": "Changes" })).unwrap();
        let report = registry.dispatch(&mut again, "vcs.changelist.delete", json!({ "name": "Login" })).unwrap();
        assert_eq!(names(&report), [("Changes".to_owned(), true, vec!["b.txt".to_owned(), "c.txt".to_owned()])]);
    }

    #[test]
    fn one_changelist_is_committed_and_the_others_stay() {
        zen_i18n::set_language(Some("en"));
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let (registry, mut session) = super::super::tests::session(&root);
        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        std::fs::write(root.join("b.txt"), "b\n").unwrap();
        git(&root, &["add", "b.txt"]);
        registry.dispatch(&mut session, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        registry.dispatch(&mut session, "vcs.changelist.create", json!({ "name": "B", "paths": ["b.txt"] })).unwrap();
        let params = json!({ "message": "Add b", "changelist": "B" });
        registry.dispatch(&mut session, "vcs.commit.create", params).unwrap();
        assert_eq!(git(&root, &["show", "--name-only", "--format=%s", "HEAD"]).trim(), "Add b\n\nb.txt");
        assert_eq!(git(&root, &["status", "--porcelain"]).trim(), "M a.txt");
        let params = json!({ "message": "Nothing", "changelist": "Nowhere" });
        assert!(registry.dispatch(&mut session, "vcs.commit.create", params).is_err());
        let report = registry.dispatch(&mut session, "vcs.changelist.list", json!({})).unwrap();
        assert_eq!(report["lists"][1]["files"], json!([]), "B is empty now");
        assert_eq!(report["lists"][0]["files"][0]["relative"], "a.txt");
    }
}
