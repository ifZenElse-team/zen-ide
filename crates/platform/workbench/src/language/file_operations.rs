use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use zen_core::Session;
use zen_i18n::t;
use zen_lsp::lsp_types::FileChangeType;
use zen_lsp::{Client, FileOperation, LspError};

use super::edits::{self, Applied};
use super::refactor::Refactoring;
use super::{Languages, with_languages};
use crate::Workbench;

pub(crate) const PARTICIPANTS_WAIT: Duration = Duration::from_secs(3);

const ASK_AGAIN_AFTER: Duration = Duration::from_millis(250);

const SCANNED_FILES: usize = 5000;

const SCANNED_SIZE: u64 = 2 << 20;

const SHOWN_FILES: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Touched {
    pub(crate) path: PathBuf,
    pub(crate) new_path: Option<PathBuf>,
    pub(crate) folder: bool,
}

impl Touched {
    pub(crate) fn existing(path: &Path, new_path: Option<PathBuf>) -> Self {
        Self { path: path.to_owned(), new_path, folder: path.is_dir() }
    }

    pub(crate) fn new_entry(path: &Path, folder: bool) -> Self {
        Self { path: path.to_owned(), new_path: None, folder }
    }
}

fn wanted<'a>(
    client: &Client,
    operation: FileOperation,
    will: bool,
    files: &'a [Touched],
) -> Vec<(&'a Path, Option<&'a Path>)> {
    files
        .iter()
        .filter(|file| client.wants_file_operation(operation, will, &file.path, file.folder))
        .map(|file| (file.path.as_path(), file.new_path.as_deref()))
        .collect()
}

pub(crate) fn will(session: &mut Session, operation: FileOperation, files: &[Touched]) -> Vec<Applied> {
    if files.is_empty() {
        return Vec::new();
    }
    let allowed =
        session.state::<crate::scratch::Scratches>().map(crate::scratch::Scratches::folders).unwrap_or_default();
    session.state_mut::<Workbench>().agent_hidden = crate::hidden_files::for_caller(session);
    with_languages(session, |languages, workbench| ask(languages, workbench, operation, files, &allowed))
}

fn ask(
    languages: &mut Languages,
    workbench: &mut Workbench,
    operation: FileOperation,
    files: &[Touched],
    allowed: &[PathBuf],
) -> Vec<Applied> {
    let deadline = Instant::now() + PARTICIPANTS_WAIT;
    let mut asked = Vec::new();
    for id in languages.running_servers() {
        let Some(client) = languages.client_of(&id) else { continue };
        let wanted = wanted(client, operation, true, files);
        if wanted.is_empty() {
            continue;
        }
        if let Ok(pending) = client.request(operation.will_method(), operation.params(&wanted)) {
            asked.push((id, pending));
        }
    }
    let mut applied = Vec::new();
    for (id, mut pending) in asked {
        let mut steps = None;
        let mut missed = Vec::new();
        loop {
            let answer = match pending.wait(deadline.saturating_duration_since(Instant::now())) {
                Ok(answer) => answer,
                Err(error) => {
                    if error == LspError::Timeout {
                        if let Some(client) = languages.client_of(&id) {
                            client.cancel(&pending);
                        }
                        if steps.is_none() {
                            workbench.messages.push(t!("workbench-file-operation-timeout", server = id.as_str()));
                        }
                    }
                    break;
                }
            };
            if answer.is_null() {
                break;
            }
            let parsed = match edits::parse(&answer, languages) {
                Ok(parsed) => parsed,
                Err(error) => {
                    workbench.messages.push(t!(
                        "workbench-file-operation-edit-failed",
                        server = id.as_str(),
                        error = error.message()
                    ));
                    break;
                }
            };
            missed = left_naming(&parsed, files, workbench);
            steps = Some(parsed);
            if missed.is_empty() || Instant::now() + ASK_AGAIN_AFTER >= deadline {
                break;
            }
            if let Some(client) = languages.client_of(&id) {
                let changed: Vec<(String, FileChangeType)> =
                    missed.iter().map(|path| (zen_lsp::position::file_uri(path), FileChangeType::CHANGED)).collect();
                let _ = client.did_change_watched_files(&changed);
            }
            std::thread::sleep(ASK_AGAIN_AFTER);
            let wanted = languages.client_of(&id).map(|client| wanted(client, operation, true, files));
            let again = languages
                .client_of(&id)
                .zip(wanted)
                .and_then(|(client, wanted)| client.request(operation.will_method(), operation.params(&wanted)).ok());
            match again {
                Some(next) => pending = next,
                None => break,
            }
        }
        let Some(steps) = steps else { continue };
        if !missed.is_empty() {
            let shown: Vec<String> = missed
                .iter()
                .take(SHOWN_FILES)
                .map(|path| path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default())
                .collect();
            let name = files.iter().find_map(renamed_stems).map(|(old, _)| old).unwrap_or_default();
            workbench.messages.push(t!(
                "workbench-file-operation-missed",
                server = id.as_str(),
                name = name,
                count = missed.len(),
                files = shown.join(", ")
            ));
        }
        let made = edits::confine(&steps, workbench, allowed).and_then(|()| edits::apply(&steps, languages, workbench));
        match made {
            Ok(log) => applied.extend(log),
            Err(error) => workbench.messages.push(t!(
                "workbench-file-operation-edit-failed",
                server = id.as_str(),
                error = error.message()
            )),
        }
    }
    applied
}

fn renamed_stems(file: &Touched) -> Option<(String, String)> {
    let new_path = file.new_path.as_deref()?;
    if file.folder || file.path.extension() != new_path.extension() {
        return None;
    }
    let old = file.path.file_stem()?.to_str()?;
    let new = new_path.file_stem()?.to_str()?;
    let identifier = |name: &str| {
        name.chars().next().is_some_and(|first| first.is_alphabetic() || first == '_')
            && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
    };
    (old != new && identifier(old) && identifier(new)).then(|| (old.to_owned(), new.to_owned()))
}

fn names(text: &str, name: &str) -> bool {
    let part = |c: char| c.is_alphanumeric() || c == '_' || c == '$';
    text.match_indices(name).any(|(at, _)| {
        !text[..at].chars().next_back().is_some_and(part) && !text[at + name.len()..].chars().next().is_some_and(part)
    })
}

fn left_naming(steps: &[edits::Step], files: &[Touched], workbench: &Workbench) -> Vec<PathBuf> {
    let Some(root) = workbench.project() else { return Vec::new() };
    let edited: Vec<&edits::FileEdit> = edits::text_files(steps).collect();
    let mut left = Vec::new();
    for file in files {
        let Some((old, new)) = renamed_stems(file) else { continue };
        let renames = edited.iter().any(|edit| edit.changes.iter().any(|change| change.new_text == new));
        let Some(extension) = file.path.extension() else { continue };
        if !renames {
            continue;
        }
        for candidate in same_kind(root, extension) {
            if candidate == file.path || edited.iter().any(|edit| edit.path == candidate) {
                continue;
            }
            let open = workbench.documents().iter().find(|document| document.path() == candidate);
            let text = match open {
                Some(document) => Some(document.text().to_owned()),
                None => std::fs::metadata(&candidate)
                    .ok()
                    .filter(|meta| meta.len() <= SCANNED_SIZE)
                    .and_then(|_| std::fs::read_to_string(&candidate).ok()),
            };
            if text.is_some_and(|text| names(&text, &old)) && !left.contains(&candidate) {
                left.push(candidate);
            }
        }
    }
    left
}

fn same_kind(root: &Path, extension: &std::ffi::OsStr) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![root.to_owned()];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let skipped = entry.file_name().to_str().is_some_and(|name| {
                name.starts_with('.') || matches!(name, "target" | "build" | "out" | "node_modules")
            });
            match entry.file_type() {
                Ok(kind) if kind.is_dir() && !skipped => folders.push(path),
                Ok(kind) if kind.is_file() && path.extension() == Some(extension) => found.push(path),
                _ => {}
            }
            if found.len() >= SCANNED_FILES {
                return found;
            }
        }
    }
    found
}

pub(crate) fn did(session: &mut Session, operation: FileOperation, files: &[Touched]) {
    if files.is_empty() {
        return;
    }
    with_languages(session, |languages, _| {
        for id in languages.running_servers() {
            let Some(client) = languages.client_of(&id) else { continue };
            let wanted = wanted(client, operation, false, files);
            if !wanted.is_empty() {
                let _ = client.notify(operation.did_method(), operation.params(&wanted));
            }
        }
    });
}

pub(crate) fn remember(session: &mut Session, mut applied: Vec<Applied>, renamed: &[(PathBuf, PathBuf)]) {
    if applied.is_empty() {
        return;
    }
    let written = applied.iter().flat_map(Applied::written).collect();
    let workbench = session.state_mut::<Workbench>();
    for (from, to) in renamed {
        let version = workbench.documents.iter().find(|document| document.path() == to).map(crate::Document::version);
        applied.push(Applied::Renamed {
            from: from.clone(),
            to: to.clone(),
            replaced: None,
            folders: Vec::new(),
            version,
        });
    }
    let entry = edits::journal(applied, workbench);
    session.state_mut::<Refactoring>().record(entry);
    crate::local_history::changed_by(session, crate::local_history::RENAME, written);
}

pub(crate) fn forget(session: &mut Session, applied: &[Applied]) {
    if !applied.is_empty() {
        with_languages(session, |languages, workbench| edits::revert(applied, languages, workbench, false));
    }
}

#[cfg(test)]
mod tests;
