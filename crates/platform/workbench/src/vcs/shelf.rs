use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;
use zen_vcs::Repository;

use super::changelists::{self, LocalChanges};
use super::commit::{DiffView, diff_view, relative, save_documents};
use super::history::{self, Done, Operation};
use super::{StatusList, absolute, changed, message, repository};
use crate::Workbench;

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListShelves)?;
    registry.register(ShelveChanges)?;
    registry.register(ShelfDiff)?;
    registry.register(Unshelve)?;
    registry.register(DeleteShelf)?;
    registry.register(RenameShelf)?;
    registry.register(ImportShelf)?;
    Ok(())
}

pub(super) fn patch_of(session: &mut Session, root: &Path, id: u64) -> Result<(PathBuf, String), CommandError> {
    let shelf = Shelf::open(session, root);
    let kept = shelf.find(id)?;
    Ok((shelf.patch(id), kept.name.clone()))
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct KeptFile {
    path: String,
    old_path: Option<String>,
    status: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Kept {
    id: u64,
    name: String,
    time: u64,
    files: Vec<KeptFile>,
    applied: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Index {
    root: PathBuf,
    next: u64,
    shelves: Vec<Kept>,
}

struct Shelf {
    folder: PathBuf,
    index: Index,
}

impl Shelf {
    fn open(session: &mut Session, root: &Path) -> Shelf {
        let folder = session.state_mut::<LocalChanges>().shelf_folder(root);
        let index = std::fs::read(folder.join("shelf.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Index>(&bytes).ok())
            .filter(|index| index.root == root)
            .unwrap_or_else(|| Index { root: root.to_owned(), ..Index::default() });
        Shelf { folder, index }
    }

    fn save(&self) -> Result<(), CommandError> {
        let failed = |error: std::io::Error| CommandError::new(error.to_string());
        std::fs::create_dir_all(&self.folder).map_err(failed)?;
        let bytes = serde_json::to_vec_pretty(&self.index).map_err(|error| CommandError::new(error.to_string()))?;
        let partial = self.folder.join("shelf.json.partial");
        std::fs::write(&partial, bytes).map_err(failed)?;
        std::fs::rename(&partial, self.folder.join("shelf.json")).map_err(failed)
    }

    fn patch(&self, id: u64) -> PathBuf {
        self.folder.join(format!("{id}.patch"))
    }

    fn contents(&self, id: u64) -> PathBuf {
        self.folder.join(id.to_string())
    }

    fn find(&self, id: u64) -> Result<&Kept, CommandError> {
        self.index
            .shelves
            .iter()
            .find(|kept| kept.id == id)
            .ok_or_else(|| CommandError::new(t!("vcs-error-no-shelf", id = id)))
    }

    fn info(&self, kept: &Kept) -> ShelfInfo {
        let root = &self.index.root;
        ShelfInfo {
            id: kept.id,
            name: kept.name.clone(),
            time: kept.time,
            applied: kept.applied,
            files: kept
                .files
                .iter()
                .map(|file| ShelvedFile {
                    path: absolute(root, &file.path),
                    relative: file.path.clone(),
                    status: file.status.clone(),
                    old_path: file.old_path.clone(),
                })
                .collect(),
        }
    }

    fn list(&self) -> ShelfList {
        ShelfList { shelves: self.index.shelves.iter().rev().map(|kept| self.info(kept)).collect() }
    }
}

/// A shelved file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ShelvedFile {
    /// The file.
    pub path: PathBuf,
    /// The file, relative to the repository, with `/`.
    pub relative: String,
    /// `added`, `modified`, `deleted`, `renamed` or `unversioned`, as when it was shelved.
    pub status: String,
    /// Where a renamed file was, relative.
    pub old_path: Option<String>,
}

/// A shelved changelist.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ShelfInfo {
    /// Its number in the repository's shelf, which names it in the other commands.
    pub id: u64,
    /// Its name (the changelist's, or the one given).
    pub name: String,
    /// When it was shelved, in seconds since 1970.
    pub time: u64,
    /// Its files.
    pub files: Vec<ShelvedFile>,
    /// Whether it was unshelved (it is kept, under "Already unshelved", until deleted).
    pub applied: bool,
}

/// The shelf.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ShelfList {
    /// Newest first.
    pub shelves: Vec<ShelfInfo>,
}

pub struct ListShelves;

impl Command for ListShelves {
    type Params = NoParams;
    type Output = ShelfList;
    const ID: &'static str = "vcs.shelf.list";
    const TITLE: &'static str = "Shelf";
    const DESCRIPTION: &'static str = "Lists the shelved changelists, newest first: id, name, when, their files (with their \
         status when shelved) and whether they were unshelved already.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<ShelfList, CommandError> {
        let repository = repository(session)?;
        Ok(Shelf::open(session, repository.root()).list())
    }
}

/// What to shelve.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ShelveParams {
    /// The changed files (absolute or relative to the repository), unversioned ones too; the files
    /// of `changelist`, or of the active changelist, when empty.
    pub paths: Vec<PathBuf>,
    /// The changelist to shelve when no `paths` are given.
    pub changelist: Option<String>,
    /// The shelf's name: the changelist's when absent.
    pub name: Option<String>,
}

pub struct ShelveChanges;

impl Command for ShelveChanges {
    type Params = ShelveParams;
    type Output = ShelfInfo;
    const ID: &'static str = "vcs.shelf.create";
    const TITLE: &'static str = "Shelve Changes…";
    const DESCRIPTION: &'static str = "Puts changes aside on the shelf and rolls them back: the files `paths` (staged and \
         unstaged changes alike, with their unsaved edits; unversioned files are shelved and \
         deleted), or the files of `changelist` (the active one when neither is given). The shelf, \
         under `name` or the changelist's name, is a patch in the data folder; vcs.shelf.unshelve \
         brings it back. Undo (vcs.history.undo) takes a shelve back. Answers with the shelf.";

    fn run(&self, session: &mut Session, params: ShelveParams) -> Result<ShelfInfo, CommandError> {
        let repository = repository(session)?;
        let (list, paths) = if params.paths.is_empty() {
            let list = match params.changelist.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
                Some(name) => name.to_owned(),
                None => changelists::active(session)?,
            };
            let files = changelists::files_of(session, &list)?;
            let (paths, _) = crate::hidden_files::without_hidden(session, repository.root(), files);
            (Some(list), paths)
        } else {
            let paths = params
                .paths
                .iter()
                .map(|path| relative(&repository, path))
                .collect::<Result<Vec<_>, CommandError>>()?;
            crate::hidden_files::guard_change(session, repository.root(), &paths)?;
            (None, paths)
        };
        if paths.is_empty() {
            return Err(CommandError::new(t!("vcs-error-nothing-to-shelve")));
        }
        super::vcs(session).settle();
        let status = StatusList.run(session, NoParams {})?;
        let mut files = Vec::with_capacity(paths.len());
        for path in &paths {
            let change = status
                .changes
                .iter()
                .find(|change| &change.relative == path)
                .ok_or_else(|| CommandError::new(t!("vcs-error-not-changed", path = path.as_str())))?;
            if change.status == "conflicted" {
                return Err(CommandError::new(t!("vcs-error-shelve-conflicted", path = path.as_str())));
            }
            files.push(KeptFile {
                path: path.clone(),
                old_path: change.old_path.clone(),
                status: change.status.clone(),
            });
        }
        let name = match params.name.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
            Some(name) => name.to_owned(),
            None => match list {
                Some(list) => list,
                None => changelists::current(session)
                    .ok()
                    .and_then(|(_, report)| {
                        report.lists.into_iter().find(|list| list.files.iter().any(|file| file.relative == paths[0]))
                    })
                    .map_or_else(|| t!("vcs-changelist-default"), |list| list.name),
            },
        };
        save_documents(session, &repository, &paths)?;
        let touched = touched(&files);
        let mut shelf = Shelf::open(session, repository.root());
        let id = shelf.index.next + 1;
        let failed = |error: std::io::Error| CommandError::new(error.to_string());
        std::fs::create_dir_all(shelf.contents(id)).map_err(failed)?;
        let patch = shelf.patch(id);
        let kept = keep(&repository, &shelf, id, &files, &touched, &patch);
        if let Err(error) = kept {
            let _ = std::fs::remove_file(&patch);
            let _ = std::fs::remove_dir_all(shelf.contents(id));
            return Err(error);
        }
        let recording = history::begin_with_files(session, &repository, touched.clone());
        let rolled_back = repository.rollback(&touched, true);
        if let Err(error) = &rolled_back
            && recording.as_ref().is_some_and(|recording| recording.nothing_changed(&repository))
        {
            let _ = std::fs::remove_file(&patch);
            let _ = std::fs::remove_dir_all(shelf.contents(id));
            changed(session);
            return Err(CommandError::new(message(error)));
        }
        history::finish(session, &repository, recording, Done::new(Operation::Shelve, name.clone()).count(files.len()));
        reload(session, &repository, &touched);
        changed(session);
        shelf.index.next = id;
        let entry = Kept { id, name, time: crate::recent::now(), files, applied: false };
        shelf.index.shelves.push(entry.clone());
        shelf.save()?;
        match rolled_back {
            Ok(()) => Ok(shelf.info(&entry)),
            Err(error) => {
                let reason = message(&error);
                Err(CommandError::new(t!(
                    "vcs-error-shelve-rollback",
                    name = entry.name.as_str(),
                    reason = reason.as_str()
                )))
            }
        }
    }
}

fn touched(files: &[KeptFile]) -> Vec<String> {
    let mut touched: Vec<String> = Vec::new();
    for file in files {
        for path in std::iter::once(&file.path).chain(&file.old_path) {
            if !touched.contains(path) {
                touched.push(path.clone());
            }
        }
    }
    touched
}

fn keep(
    repository: &Repository,
    shelf: &Shelf,
    id: u64,
    files: &[KeptFile],
    touched: &[String],
    patch: &Path,
) -> Result<(), CommandError> {
    repository.write_patch(touched, patch).map_err(|error| CommandError::new(message(&error)))?;
    if std::fs::metadata(patch).map_or(0, |metadata| metadata.len()) == 0 {
        return Err(CommandError::new(t!("vcs-error-nothing-to-shelve")));
    }
    let failed = |error: std::io::Error| CommandError::new(error.to_string());
    let contents = shelf.contents(id);
    for (n, file) in files.iter().enumerate() {
        let base = repository
            .file_at_head(file.old_path.as_deref().unwrap_or(&file.path))
            .map_err(|error| CommandError::new(message(&error)))?;
        if let Some(base) = base {
            std::fs::write(contents.join(format!("{n}.base")), base).map_err(failed)?;
        }
        if let Ok(shelved) = std::fs::read(absolute(repository.root(), &file.path)) {
            std::fs::write(contents.join(format!("{n}.shelved")), shelved).map_err(failed)?;
        }
    }
    Ok(())
}

fn reload(session: &mut Session, repository: &Repository, paths: &[String]) {
    let workbench = session.state_mut::<Workbench>();
    for document in &mut workbench.documents {
        if relative(repository, document.path()).is_ok_and(|path| paths.contains(&path)) {
            document.reload_from_disk();
        }
    }
}

/// A file of a shelf.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ShelfFileParams {
    /// The shelf (vcs.shelf.list).
    pub id: u64,
    /// One of its files (absolute or relative to the repository).
    pub path: PathBuf,
}

pub struct ShelfDiff;

impl Command for ShelfDiff {
    type Params = ShelfFileParams;
    type Output = DiffView;
    const ID: &'static str = "vcs.shelf.diff";
    const TITLE: &'static str = "Show Diff";
    const DESCRIPTION: &'static str = "Compares a shelved file with the version it was shelved against (HEAD at the time): \
         both texts and the changed lines and words, as vcs.diff.file does.";

    fn run(&self, session: &mut Session, params: ShelfFileParams) -> Result<DiffView, CommandError> {
        let repository = repository(session)?;
        let path = relative(&repository, &params.path)?;
        let shelf = Shelf::open(session, repository.root());
        let kept = shelf.find(params.id)?;
        let n = kept
            .files
            .iter()
            .position(|file| file.path == path)
            .ok_or_else(|| CommandError::new(t!("vcs-error-not-shelved", path = path.as_str())))?;
        let contents = shelf.contents(kept.id);
        let read = |name: String| std::fs::read(contents.join(name)).unwrap_or_default();
        let titles = (t!("vcs-shelf-diff-base"), t!("vcs-shelf-diff-shelved"));
        let (base, shelved) = (read(format!("{n}.base")), read(format!("{n}.shelved")));
        Ok(diff_view(absolute(repository.root(), &path), path, titles, &base, &shelved))
    }
}

/// What to unshelve.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct UnshelveParams {
    /// The shelf (vcs.shelf.list).
    pub id: u64,
    /// Only these of its files (absolute or relative); all of them when empty.
    pub paths: Vec<PathBuf>,
    /// The changelist the files go into: when absent, the one named as the shelf (the list it was
    /// shelved from) if there is one, else the active one.
    pub changelist: Option<String>,
    /// Mark the shelf as unshelved once all of it applied cleanly (it moves under "Already
    /// unshelved" and can still be unshelved again or deleted). On by default.
    pub remove: bool,
}

impl Default for UnshelveParams {
    fn default() -> Self {
        UnshelveParams { id: 0, paths: Vec::new(), changelist: None, remove: true }
    }
}

/// What an unshelve brought back.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Unshelved {
    /// The shelf, as it is now.
    pub shelf: ShelfInfo,
    /// The files brought back, relative.
    pub files: Vec<String>,
    /// The changelist they went into.
    pub changelist: String,
}

pub struct Unshelve;

impl Command for Unshelve {
    type Params = UnshelveParams;
    type Output = Unshelved;
    const ID: &'static str = "vcs.shelf.unshelve";
    const TITLE: &'static str = "Unshelve…";
    const DESCRIPTION: &'static str = "Brings a shelf's changes back to the working tree, all its files or only `paths`, into \
         the changelist `changelist` (by default the list it was shelved from, if it is still there, \
         else the active one). Files changed since are merged \
         three-way; on conflicts the files are left conflicted (resolve them, the shelf stays) and \
         the command fails with the code `conflicts`. With `remove` (the default), a shelf applied \
         whole is marked as unshelved. Undo (vcs.history.undo) takes an unshelve back.";

    fn run(&self, session: &mut Session, params: UnshelveParams) -> Result<Unshelved, CommandError> {
        let repository = repository(session)?;
        let mut shelf = Shelf::open(session, repository.root());
        let kept = shelf.find(params.id)?.clone();
        let wanted =
            params.paths.iter().map(|path| relative(&repository, path)).collect::<Result<Vec<_>, CommandError>>()?;
        if let Some(path) = wanted.iter().find(|path| !kept.files.iter().any(|file| &file.path == *path)) {
            return Err(CommandError::new(t!("vcs-error-not-shelved", path = path.as_str())));
        }
        let files: Vec<KeptFile> =
            kept.files.iter().filter(|file| wanted.is_empty() || wanted.contains(&file.path)).cloned().collect();
        let whole = files.len() == kept.files.len();
        let touched = touched(&files);
        let include = if whole { Vec::new() } else { touched.clone() };
        let (lists, _) = changelists::current(session)?;
        let target = match params.changelist.as_deref().map(str::trim).filter(|name| !name.is_empty()) {
            Some(name) if lists.has(name) => name.to_owned(),
            Some(name) => return Err(CommandError::new(t!("vcs-error-no-changelist", name = name))),
            None if lists.has(&kept.name) => kept.name.clone(),
            None => lists.active().to_owned(),
        };
        save_documents(session, &repository, &touched)?;
        let paths: Vec<String> = files.iter().map(|file| file.path.clone()).collect();
        crate::hidden_files::guard_change(session, repository.root(), &touched)?;
        changelists::assign(session, repository.root(), &paths, &target);
        let recording = history::begin_with_files(session, &repository, touched.clone());
        let result = repository.apply_patch(&shelf.patch(kept.id), &include);
        if result.is_ok() {
            let done = Done::new(Operation::Unshelve, kept.name.clone()).count(files.len());
            history::finish(session, &repository, recording, done);
        }
        reload(session, &repository, &touched);
        changed(session);
        if let Err(error) = result {
            return Err(super::reported(session, &error));
        }
        if whole && params.remove {
            if let Some(entry) = shelf.index.shelves.iter_mut().find(|entry| entry.id == kept.id) {
                entry.applied = true;
            }
            shelf.save()?;
        }
        let info = shelf.info(shelf.find(kept.id)?);
        Ok(Unshelved { shelf: info, files: paths, changelist: target })
    }
}

/// A shelf.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ShelfRef {
    /// The shelf (vcs.shelf.list).
    pub id: u64,
}

pub struct DeleteShelf;

impl Command for DeleteShelf {
    type Params = ShelfRef;
    type Output = ShelfList;
    const ID: &'static str = "vcs.shelf.delete";
    const TITLE: &'static str = "Delete Shelved Changes";
    const DESCRIPTION: &'static str = "Deletes a shelf and its changes for good (the window asks first; this command does \
         not). Answers with the shelves left.";

    fn run(&self, session: &mut Session, params: ShelfRef) -> Result<ShelfList, CommandError> {
        let repository = repository(session)?;
        let mut shelf = Shelf::open(session, repository.root());
        shelf.find(params.id)?;
        shelf.index.shelves.retain(|kept| kept.id != params.id);
        shelf.save()?;
        let _ = std::fs::remove_file(shelf.patch(params.id));
        let _ = std::fs::remove_dir_all(shelf.contents(params.id));
        Ok(shelf.list())
    }
}

/// A shelf's new name.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RenameParams {
    /// The shelf (vcs.shelf.list).
    pub id: u64,
    /// Its new name.
    pub name: String,
}

pub struct RenameShelf;

impl Command for RenameShelf {
    type Params = RenameParams;
    type Output = ShelfInfo;
    const ID: &'static str = "vcs.shelf.rename";
    const TITLE: &'static str = "Rename Shelved Changes";
    const DESCRIPTION: &'static str = "Gives a shelf a new name. Answers with the shelf.";

    fn run(&self, session: &mut Session, params: RenameParams) -> Result<ShelfInfo, CommandError> {
        let name = params.name.trim();
        if name.is_empty() {
            return Err(CommandError::new(t!("vcs-error-shelf-name")));
        }
        let repository = repository(session)?;
        let mut shelf = Shelf::open(session, repository.root());
        shelf.find(params.id)?;
        if let Some(kept) = shelf.index.shelves.iter_mut().find(|kept| kept.id == params.id) {
            kept.name = name.to_owned();
        }
        shelf.save()?;
        Ok(shelf.info(shelf.find(params.id)?))
    }
}

/// A patch to put on the shelf.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ImportParams {
    /// The patch, mapped onto the repository as vcs.patch.read maps it.
    #[serde(flatten)]
    pub input: super::patch::PatchInput,
    /// Only these of its files (as vcs.patch.read names them); all of them when empty.
    pub paths: Vec<PathBuf>,
    /// The shelf's name: the patch's subject or file name when absent.
    pub name: Option<String>,
}

pub struct ImportShelf;

impl Command for ImportShelf {
    type Params = ImportParams;
    type Output = ShelfInfo;
    const ID: &'static str = "vcs.shelf.import";
    const TITLE: &'static str = "Import to Shelf";
    const DESCRIPTION: &'static str = "Puts a patch (`path`, `text` or `clipboard`, mapped as vcs.patch.read maps it) on \
         the shelf instead of applying it, all its files or only `paths`, named `name` (the patch's \
         subject or file name by default); the working tree is left alone. vcs.shelf.unshelve applies \
         it later. Answers with the shelf.";

    fn run(&self, session: &mut Session, params: ImportParams) -> Result<ShelfInfo, CommandError> {
        let repository = repository(session)?;
        let loaded = super::patch::load(session, &repository, &params.input)?;
        let wanted =
            params.paths.iter().map(|path| relative(&repository, path)).collect::<Result<Vec<_>, CommandError>>()?;
        let planned: Vec<super::patch::Planned> = super::patch::plan(&repository, &loaded)
            .into_iter()
            .filter(|file| wanted.is_empty() || wanted.iter().any(|path| path == file.target()))
            .collect();
        if planned.is_empty() {
            return Err(CommandError::new(t!("vcs-patch-error-nothing")));
        }
        let mut chosen = loaded.patch.clone();
        chosen.mail = None;
        chosen.files.retain(|file| planned.iter().any(|kept| kept.target() == file.path()));
        let name = params
            .name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                loaded.patch.mail.as_ref().map(|mail| mail.subject.clone()).filter(|subject| !subject.is_empty())
            })
            .unwrap_or_else(|| t!("vcs-patch-name"));
        let mut shelf = Shelf::open(session, repository.root());
        let id = shelf.index.next + 1;
        let failed = |error: std::io::Error| CommandError::new(error.to_string());
        let contents = shelf.contents(id);
        std::fs::create_dir_all(&contents).map_err(failed)?;
        let mut files = Vec::with_capacity(planned.len());
        for (n, file) in planned.iter().enumerate() {
            if let Some(before) = file.before() {
                std::fs::write(contents.join(format!("{n}.base")), before).map_err(failed)?;
            }
            if let Some(after) = file.after() {
                std::fs::write(contents.join(format!("{n}.shelved")), after).map_err(failed)?;
            }
            files.push(KeptFile {
                path: file.target().to_owned(),
                old_path: file.source().map(str::to_owned),
                status: file.status().to_owned(),
            });
        }
        std::fs::write(shelf.patch(id), chosen.to_bytes()).map_err(failed)?;
        shelf.index.next = id;
        let entry = Kept { id, name, time: crate::recent::now(), files, applied: false };
        shelf.index.shelves.push(entry.clone());
        shelf.save()?;
        Ok(shelf.info(&entry))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::testing::*;
    use super::*;

    fn session(root: &Path, data: &Path) -> (Registry, Session) {
        zen_i18n::set_language(Some("en"));
        let (registry, mut session) = super::super::tests::session(root);
        session.state_mut::<LocalChanges>().set_folder(data.to_owned());
        session.state_mut::<history::GitHistory>().set_folder(data.join("vcs-history"));
        (registry, session)
    }

    #[test]
    fn changes_are_shelved_previewed_unshelved_into_a_list_and_deleted() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let data = tempfile::tempdir().unwrap();
        let (registry, mut session) = session(&root, data.path());
        let s = &mut session;
        let run = |s: &mut Session, id: &str, params: serde_json::Value| registry.dispatch(s, id, params);
        assert!(run(s, "vcs.shelf.create", json!({})).is_err(), "nothing to shelve");
        std::fs::write(root.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(root.join("new.txt"), "new\n").unwrap();
        run(s, "file.document.open", json!({ "path": root.join("a.txt") })).unwrap();
        let document = s.state_mut::<Workbench>().active_document_mut().unwrap();
        document.replace_ranges(vec![(14..14, "four\n".to_owned())]);
        run(s, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        assert!(run(s, "vcs.shelf.create", json!({ "paths": ["other.txt"] })).is_err(), "not changed");

        let shelf = run(s, "vcs.shelf.create", json!({ "paths": ["a.txt", "new.txt"], "name": "Halfway" })).unwrap();
        assert_eq!((shelf["id"].as_u64(), shelf["name"].as_str()), (Some(1), Some("Halfway")));
        assert_eq!(shelf["files"][1]["status"], "unversioned");
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\ntwo\nthree\n", "rolled back");
        assert!(!root.join("new.txt").exists());
        let text = s.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        assert_eq!(text, "one\ntwo\nthree\n", "the open file follows");
        assert!(data.path().join("vcs-shelf").read_dir().unwrap().next().is_some(), "kept in the data folder");

        let diff = run(s, "vcs.shelf.diff", json!({ "id": 1, "path": "a.txt" })).unwrap();
        assert_eq!(
            (diff["before"].as_str(), diff["after"].as_str()),
            (Some("one\ntwo\nthree\n"), Some("one\nTWO\nthree\nfour\n"))
        );
        assert_eq!(diff["hunks"].as_array().map(Vec::len), Some(2));
        let diff = run(s, "vcs.shelf.diff", json!({ "id": 1, "path": "new.txt" })).unwrap();
        assert_eq!((diff["before"].as_str(), diff["after"].as_str()), (Some(""), Some("new\n")));
        assert!(run(s, "vcs.shelf.diff", json!({ "id": 1, "path": "b.txt" })).is_err());

        let undo = run(s, "vcs.history.list", json!({})).unwrap();
        assert_eq!(undo["undo"]["title"], "Shelve 'Halfway'", "{undo}");
        run(s, "vcs.history.undo", json!({})).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\nTWO\nthree\nfour\n");
        run(s, "vcs.history.redo", json!({})).unwrap();
        assert!(!root.join("new.txt").exists());

        run(s, "vcs.changelist.create", json!({ "name": "Feature" })).unwrap();
        let done = run(s, "vcs.shelf.unshelve", json!({ "id": 1, "changelist": "Feature" })).unwrap();
        assert_eq!(done["shelf"]["applied"], true);
        assert_eq!(std::fs::read_to_string(root.join("new.txt")).unwrap(), "new\n");
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\nTWO\nthree\nfour\n");
        let lists = run(s, "vcs.changelist.list", json!({})).unwrap();
        assert_eq!(lists["lists"][1]["files"][0]["relative"], "a.txt", "{lists}");
        assert_eq!(run(s, "vcs.history.list", json!({})).unwrap()["undo"]["title"], "Unshelve 'Halfway'");

        assert!(run(s, "vcs.shelf.rename", json!({ "id": 1, "name": " " })).is_err());
        run(s, "vcs.shelf.rename", json!({ "id": 1, "name": "Done" })).unwrap();
        let list = run(s, "vcs.shelf.list", json!({})).unwrap();
        assert_eq!(list["shelves"][0]["name"], "Done");
        let left = run(s, "vcs.shelf.delete", json!({ "id": 1 })).unwrap();
        assert_eq!(left["shelves"], json!([]));
        assert!(run(s, "vcs.shelf.unshelve", json!({ "id": 1 })).is_err());
    }

    #[test]
    fn a_changelist_is_shelved_and_a_conflicting_unshelve_is_reported() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        std::fs::write(root.join("b.txt"), "b\n").unwrap();
        git(&root, &["add", "b.txt"]);
        git(&root, &["commit", "-q", "-m", "B"]);
        let data = tempfile::tempdir().unwrap();
        let (registry, mut session) = session(&root, data.path());
        let s = &mut session;
        let run = |s: &mut Session, id: &str, params: serde_json::Value| registry.dispatch(s, id, params);
        std::fs::write(root.join("a.txt"), "one\nmine\nthree\n").unwrap();
        std::fs::write(root.join("b.txt"), "B\n").unwrap();
        run(s, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        run(s, "vcs.changelist.create", json!({ "name": "Login", "paths": ["a.txt"] })).unwrap();
        let shelf = run(s, "vcs.shelf.create", json!({ "changelist": "Login" })).unwrap();
        assert_eq!(shelf["name"], "Login");
        assert_eq!(shelf["files"].as_array().map(Vec::len), Some(1));
        assert_eq!(std::fs::read_to_string(root.join("b.txt")).unwrap(), "B\n", "the other list stays");

        std::fs::write(root.join("a.txt"), "one\ntheirs\nthree\n").unwrap();
        git(&root, &["commit", "-q", "-am", "Theirs"]);
        let failed = run(s, "vcs.shelf.unshelve", json!({ "id": shelf["id"] })).unwrap_err();
        let zen_core::DispatchError::Failed { error, .. } = failed else { panic!("{failed:?}") };
        assert_eq!(error.code.as_deref(), Some(super::super::CONFLICTS));
        assert!(error.message.contains("a.txt"), "{}", error.message);
        let told = s.state_mut::<super::super::workspace::GitWorkspace>().take_conflicts();
        assert_eq!(told.as_deref(), Some(error.message.as_str()));
        let list = run(s, "vcs.shelf.list", json!({})).unwrap();
        assert_eq!(list["shelves"][0]["applied"], false, "kept until the conflicts are resolved");
        let text = std::fs::read_to_string(root.join("a.txt")).unwrap();
        assert!(text.contains("<<<<<<<"), "{text}");
    }

    #[cfg(unix)]
    #[test]
    fn a_rollback_failing_half_way_keeps_the_shelf_and_the_undo_entry() {
        use std::os::unix::fs::PermissionsExt;
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let data = tempfile::tempdir().unwrap();
        let (registry, mut session) = session(&root, data.path());
        let s = &mut session;
        let run = |s: &mut Session, id: &str, params: serde_json::Value| registry.dispatch(s, id, params);
        std::fs::write(root.join("a.txt"), "edited\n").unwrap();
        let locked = root.join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::write(locked.join("new.txt"), "new\n").unwrap();
        run(s, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();

        let failed = run(s, "vcs.shelf.create", json!({ "paths": ["a.txt", "locked/new.txt"], "name": "Work" }));
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        let zen_core::DispatchError::Failed { error, .. } = failed.unwrap_err() else { panic!("not a failure") };
        assert!(error.message.contains("Work"), "{}", error.message);

        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\ntwo\nthree\n", "rolled back");
        let list = run(s, "vcs.shelf.list", json!({})).unwrap();
        assert_eq!(list["shelves"][0]["name"], "Work", "{list}");
        let diff = run(s, "vcs.shelf.diff", json!({ "id": 1, "path": "a.txt" })).unwrap();
        assert_eq!(diff["after"], "edited\n");
        let undo = run(s, "vcs.history.list", json!({})).unwrap();
        assert_eq!(undo["undo"]["title"], "Shelve 'Work'", "{undo}");
        run(s, "vcs.history.undo", json!({})).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "edited\n");
    }

    #[test]
    fn a_rollback_that_changed_nothing_leaves_no_shelf() {
        let dir = test_repository();
        let root = dir.path().canonicalize().unwrap();
        let data = tempfile::tempdir().unwrap();
        let (registry, mut session) = session(&root, data.path());
        let s = &mut session;
        let run = |s: &mut Session, id: &str, params: serde_json::Value| registry.dispatch(s, id, params);
        std::fs::write(root.join("a.txt"), "edited\n").unwrap();
        run(s, "vcs.status.refresh", json!({ "wait": true })).unwrap();
        std::fs::write(root.join(".git/index.lock"), "").unwrap();

        let failed = run(s, "vcs.shelf.create", json!({ "paths": ["a.txt"], "name": "Work" }));
        std::fs::remove_file(root.join(".git/index.lock")).unwrap();
        assert!(failed.is_err());
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "edited\n");
        assert_eq!(run(s, "vcs.shelf.list", json!({})).unwrap()["shelves"], json!([]));
    }
}
