use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::{DiskSync, Document, Workbench};

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(SyncWithDisk)?;
    registry.register(Settle::<true>)?;
    registry.register(Settle::<false>)?;
    Ok(())
}

pub fn edit_file(
    session: &mut Session,
    path: &Path,
    edit: impl FnOnce(&str) -> Option<String>,
) -> Result<bool, CommandError> {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let refused = |error: &zen_vfs::VfsError| CommandError::new(crate::closing::save_failed(&name, error));
    let workbench = session.state_mut::<Workbench>();
    let path = &workbench.resolve(path);
    if let Some(document) = workbench.documents.iter_mut().find(|document| document.path() == path) {
        document.refuse_read_only().and_then(|()| document.check_disk()).map_err(|error| refused(&error))?;
        let Some(after) = edit(document.text()).filter(|after| after != document.text()) else { return Ok(false) };
        let (range, inserted) = changed_part(document.text(), &after);
        if !document.replace_ranges(vec![(range, inserted.to_owned())]) {
            return Ok(false);
        }
        document.save().map_err(|error| refused(&error))?;
        crate::local_history::record_saves(session);
        return Ok(true);
    }
    let before = match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(CommandError::new(crate::closing::read_failed(path, &error))),
    };
    let text = std::str::from_utf8(before.as_deref().unwrap_or_default()).map_err(|_| {
        let error = std::io::Error::new(std::io::ErrorKind::InvalidData, "the file is not UTF-8 text");
        CommandError::new(crate::closing::read_failed(path, &error))
    })?;
    let Some(after) = edit(text).filter(|after| before.is_none() || after != text) else { return Ok(false) };
    zen_vfs::replace_unchanged(path, before.as_deref(), after.as_bytes()).map_err(|error| refused(&error))?;
    let written = vec![(path.to_owned(), before, Some(after.into_bytes()))];
    crate::local_history::changed_by(session, crate::local_history::FILE_EDIT, written);
    Ok(true)
}

fn changed_part<'a>(before: &str, after: &'a str) -> (std::ops::Range<usize>, &'a str) {
    let start = before.chars().zip(after.chars()).take_while(|(a, b)| a == b).count();
    let tail = |text: &'a str, chars: usize| text.char_indices().nth(chars).map_or(text.len(), |(at, _)| at);
    let old = &before[before.char_indices().nth(start).map_or(before.len(), |(at, _)| at)..];
    let new = &after[tail(after, start)..];
    let same = old.chars().rev().zip(new.chars().rev()).take_while(|(a, b)| a == b).count();
    (start..start + old.chars().count() - same, &new[..tail(new, new.chars().count() - same)])
}

/// Files that changed on disk.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct SyncParams {
    /// The changed files or folders; every open document is compared when empty.
    pub paths: Vec<PathBuf>,
}

/// What happened to one open document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Synced {
    /// The document's file.
    pub path: PathBuf,
    /// What comparing it with the disk found.
    pub outcome: DiskSync,
}

pub struct SyncWithDisk;

impl Command for SyncWithDisk {
    type Params = SyncParams;
    type Output = Vec<Synced>;
    const ID: &'static str = "file.document.syncWithDisk";
    const TITLE: &'static str = "Synchronize";
    const DESCRIPTION: &'static str = "Compares open files with the disk after other programs changed them: files without \
         unsaved changes are reloaded, files with unsaved changes are marked as conflicts. Returns what changed.";

    fn run(&self, session: &mut Session, params: SyncParams) -> Result<Vec<Synced>, CommandError> {
        let workbench = session.state_mut::<Workbench>();
        let mut synced = Vec::new();
        let mut unlocked = Vec::new();
        let changed: std::collections::HashSet<&Path> = params.paths.iter().map(PathBuf::as_path).collect();
        for document in &mut workbench.documents {
            if !changed.is_empty() && !document.path().ancestors().any(|path| changed.contains(path)) {
                continue;
            }
            let was_locked = document.is_locked();
            let outcome = document.sync_with_disk();
            if was_locked && !document.is_locked() {
                unlocked.push(document.path().to_owned());
            }
            if outcome != DiskSync::Unchanged {
                synced.push(Synced { path: document.path().to_owned(), outcome });
            }
        }
        if !synced.is_empty() {
            workbench.revision += 1;
        }
        for path in &unlocked {
            crate::read_only::settle_offers(session, path);
        }
        Ok(synced)
    }
}

/// Which document; the active one when absent.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct DocumentParams {
    /// The document's file.
    pub path: Option<PathBuf>,
}

fn target<'a>(workbench: &'a mut Workbench, path: Option<&PathBuf>) -> Result<&'a mut Document, CommandError> {
    let found = match path {
        Some(path) => workbench.documents.iter_mut().find(|document| document.path() == path),
        None => workbench.active_document_mut(),
    };
    found.ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))
}

pub struct Settle<const RELOAD: bool>;

impl<const RELOAD: bool> Command for Settle<RELOAD> {
    type Params = DocumentParams;
    type Output = bool;
    const ID: &'static str = if RELOAD { "file.document.reloadFromDisk" } else { "file.document.keepMine" };
    const TITLE: &'static str = if RELOAD { "Load File from Disk" } else { "Keep My Edits" };
    const DESCRIPTION: &'static str = if RELOAD {
        "Reloads a file from disk, dropping its unsaved changes (Undo brings them back)."
    } else {
        "Keeps a file's unsaved changes after it changed on disk; the next save writes them over the file."
    };

    fn run(&self, session: &mut Session, params: DocumentParams) -> Result<bool, CommandError> {
        let document = target(session.state_mut::<Workbench>(), params.path.as_ref())?;
        if RELOAD {
            Ok(document.reload_from_disk())
        } else {
            document.keep_mine();
            Ok(true)
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn setup() -> (Registry, Session, tempfile::TempDir) {
        zen_i18n::register(crate::LOCALES);
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        (registry, Session::new(), tempfile::tempdir().unwrap())
    }

    fn text(session: &Session) -> String {
        session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
    }

    #[test]
    fn reloads_clean_files_and_flags_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("main.go");
        std::fs::write(&file, "package main\n").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        let text =
            |session: &Session| session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
        let sync = |session: &mut Session| {
            registry.dispatch(session, "file.document.syncWithDisk", json!({ "paths": [file] })).unwrap()
        };

        assert_eq!(sync(&mut session), json!([]));
        std::fs::write(&file, "package other\n").unwrap();
        assert_eq!(sync(&mut session)[0]["outcome"], "reloaded");
        assert_eq!(text(&session), "package other\n");
        assert!(!session.state::<Workbench>().unwrap().active_document().unwrap().is_dirty());
        registry.dispatch(&mut session, "editor.edit.undo", json!({})).unwrap();
        assert_eq!(text(&session), "package main\n");
        registry.dispatch(&mut session, "editor.edit.redo", json!({})).unwrap();

        registry.dispatch(&mut session, "editor.edit.insert", json!({ "text": "// mine\n" })).unwrap();
        std::fs::write(&file, "package theirs\n").unwrap();
        assert_eq!(sync(&mut session)[0]["outcome"], "conflict");
        assert!(session.state::<Workbench>().unwrap().active_document().unwrap().has_conflict());
        registry.dispatch(&mut session, "file.document.keepMine", json!({})).unwrap();
        assert!(text(&session).contains("// mine"), "the unsaved change is kept");
        registry.dispatch(&mut session, "file.document.reloadFromDisk", json!({})).unwrap();
        assert_eq!(text(&session), "package theirs\n");

        registry.dispatch(&mut session, "editor.edit.insert", json!({ "text": "x" })).unwrap();
        registry.dispatch(&mut session, "file.document.save", json!({})).unwrap();
        assert_eq!(sync(&mut session), json!([]));

        std::fs::remove_file(&file).unwrap();
        assert_eq!(sync(&mut session)[0]["outcome"], "removed");
    }

    fn added(text: &str) -> Option<String> {
        Some(format!("{text}<added/>\n"))
    }

    #[test]
    fn editing_a_file_that_is_not_open_writes_it_or_creates_it() {
        let (registry, mut session, dir) = setup();
        let file = dir.path().join("pom.xml");
        std::fs::write(&file, "<project/>\n").unwrap();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        assert!(edit_file(&mut session, &file, added).unwrap());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "<project/>\n<added/>\n");
        assert!(!edit_file(&mut session, &file, |_| None).unwrap(), "nothing to change");

        let changed_meanwhile = edit_file(&mut session, &file, |text| {
            std::fs::write(&file, "<theirs/>\n").unwrap();
            added(text)
        });
        assert!(changed_meanwhile.is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "<theirs/>\n");

        let new = dir.path().join("build.gradle");
        assert!(edit_file(&mut session, &new, added).unwrap());
        assert_eq!(std::fs::read_to_string(&new).unwrap(), "<added/>\n");

        let binary = dir.path().join("binary");
        std::fs::write(&binary, [0xff, 0xfe]).unwrap();
        assert!(edit_file(&mut session, &binary, added).is_err());
        assert_eq!(std::fs::read(&binary).unwrap(), [0xff, 0xfe], "never rewritten as text");
    }

    #[test]
    fn editing_an_open_file_goes_through_its_document() {
        let (registry, mut session, dir) = setup();
        let file = dir.path().join("pom.xml");
        std::fs::write(&file, "<project/>\n").unwrap();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        registry.dispatch(&mut session, "editor.edit.insert", json!({ "text": "<!-- mine -->\n" })).unwrap();

        assert!(edit_file(&mut session, &file, added).unwrap());
        let mine = "<!-- mine -->\n<project/>\n<added/>\n";
        assert_eq!(text(&session), mine, "the unsaved change is kept");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), mine);
        assert!(!session.state::<Workbench>().unwrap().active_document().unwrap().is_dirty());
        registry.dispatch(&mut session, "editor.edit.undo", json!({})).unwrap();
        assert_eq!(text(&session), "<!-- mine -->\n<project/>\n", "one undo step");
        registry.dispatch(&mut session, "editor.edit.redo", json!({})).unwrap();

        std::fs::write(&file, "<theirs/>\n").unwrap();
        assert!(edit_file(&mut session, &file, added).is_err());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "<theirs/>\n", "a change made outside is not written over");
        assert_eq!(text(&session), mine, "the document keeps its text");
    }

    #[test]
    fn the_changed_part_is_what_differs_in_characters() {
        assert_eq!(changed_part("abcd", "abXd"), (2..3, "X"));
        assert_eq!(changed_part("привет", "привет!"), (6..6, "!"));
        assert_eq!(changed_part("aXa", "aa"), (1..2, ""));
        assert_eq!(changed_part("aa", "aaa"), (2..2, "a"));
        assert_eq!(changed_part("", ""), (0..0, ""));
        assert_eq!(changed_part("ж", "я"), (0..1, "я"));
    }

    #[test]
    fn only_the_files_that_changed_are_compared() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("pkg");
        std::fs::create_dir(&folder).unwrap();
        let (first, second, third) = (dir.path().join("a.go"), folder.join("b.go"), dir.path().join("c.go"));
        for file in [&first, &second, &third] {
            std::fs::write(file, "package main\n").unwrap();
        }
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        for file in [&first, &second, &third] {
            registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        }
        for file in [&first, &second, &third] {
            std::fs::write(file, "package other\n").unwrap();
        }
        let synced = registry.dispatch(&mut session, "file.document.syncWithDisk", json!({ "paths": [first, folder] }));
        let paths: Vec<String> =
            synced.unwrap().as_array().unwrap().iter().map(|one| one["path"].as_str().unwrap().to_owned()).collect();
        assert_eq!(paths, [first.display().to_string(), second.display().to_string()], "a folder covers its files");
    }
}
