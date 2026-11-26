use std::collections::HashSet;
use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Serialize;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};

use crate::{Document, Workbench, closing};

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(Autosave)
}

/// Why autosave left a modified file unsaved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Skip {
    /// The file cannot be written: the IDE opened it read-only (an encoding it does not edit), or
    /// the file is read-only on disk.
    ReadOnly,
    /// The file changed on disk since the IDE last read or wrote it: the user settles the conflict
    /// (`file.document.reloadFromDisk` or `file.document.keepMine`) before it is written.
    Conflict,
    /// The file is not on disk: another program deleted it, or it was never written.
    Missing,
}

/// A modified file autosave left alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Skipped {
    /// The file.
    pub path: PathBuf,
    /// Why it was not saved.
    pub reason: Skip,
}

/// A file autosave tried to write and could not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Failed {
    /// The file.
    pub path: PathBuf,
    /// Why, as the notification says it.
    pub error: String,
}

/// What an autosave did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
pub struct Autosaved {
    /// The files saved.
    pub saved: Vec<PathBuf>,
    /// The modified files left unsaved, and why.
    pub skipped: Vec<Skipped>,
    /// The files that could not be written; each is notified the first time it fails.
    pub failed: Vec<Failed>,
}

pub struct Autosave;

impl Command for Autosave {
    type Params = NoParams;
    type Output = Autosaved;
    const ID: &'static str = "file.document.autosave";
    const TITLE: &'static str = "Autosave";
    const DESCRIPTION: &'static str = "Saves every modified file as the IDE's autosave does (after the idle time, when \
         the window is left, when the Terminal gets the keyboard): through Save All's path, with the actions on save \
         and a Local History version, but skipping files that are read-only, changed on disk (a conflict, settled \
         with file.document.reloadFromDisk or file.document.keepMine) or not on disk. Returns the files saved, \
         skipped (and why) and failed; a file's failure is notified once, not at every autosave.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Autosaved, CommandError> {
        Ok(autosave(session))
    }
}

pub fn autosave(session: &mut Session) -> Autosaved {
    if session.state::<Workbench>().is_none() {
        return Autosaved::default();
    }
    let workbench = session.state_mut::<Workbench>();
    let mut writable = Vec::new();
    let mut skipped = Vec::new();
    let mut conflicts = false;
    for document in workbench.documents.iter_mut().filter(|document| document.dirty && document.image.is_none()) {
        let had_conflict = document.conflict;
        match check(document) {
            Ok(()) => writable.push(document.path.clone()),
            Err(reason) => {
                conflicts |= reason == Skip::Conflict && !had_conflict;
                skipped.push(Skipped { path: document.path.clone(), reason });
            }
        }
    }
    if conflicts {
        workbench.revision += 1;
    }
    let (saved, failed, notes) = closing::save_documents_noting(session, &writable);
    let troubled: HashSet<PathBuf> = notes.iter().chain(&failed).map(|(path, _)| path.clone()).collect();
    let reported = std::mem::take(session.state_mut::<Reported>());
    let messages: Vec<String> = notes
        .into_iter()
        .chain(failed.iter().cloned())
        .filter(|(path, _)| !reported.0.contains(path))
        .map(|(_, message)| message)
        .collect();
    let workbench = session.state_mut::<Workbench>();
    let waiting = |path: &PathBuf| !writable.contains(path) && closing::is_dirty(workbench, path);
    let still: Vec<PathBuf> = reported.0.into_iter().filter(waiting).collect();
    workbench.messages.extend(messages);
    session.set_state(Reported(still.into_iter().chain(troubled).collect()));
    let failed = failed.into_iter().map(|(path, error)| Failed { path, error }).collect();
    Autosaved { saved, skipped, failed }
}

#[derive(Default)]
struct Reported(HashSet<PathBuf>);

fn check(document: &mut Document) -> Result<(), Skip> {
    if document.read_only.is_some() {
        return Err(Skip::ReadOnly);
    }
    if document.conflict {
        return Err(Skip::Conflict);
    }
    let Ok(metadata) = std::fs::metadata(&document.path) else { return Err(Skip::Missing) };
    if !metadata.is_file() {
        return Err(Skip::Missing);
    }
    if metadata.permissions().readonly() {
        return Err(Skip::ReadOnly);
    }
    match document.changed_on_disk() {
        Ok(false) => Ok(()),
        Ok(true) => {
            document.conflict = true;
            Err(Skip::Conflict)
        }
        Err(_) => Err(Skip::ReadOnly),
    }
}

#[cfg(test)]
mod tests;
