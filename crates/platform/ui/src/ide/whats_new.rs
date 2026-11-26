use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_i18n::t;
use zen_toolbox::{ReleaseNote, StartNotes, notes_on_start};

use super::IdeView;

const NOTES: &str = "release-notes.json";
const SHOWN: &str = "whats-new-shown";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Kept {
    version: String,
    date: String,
    notes: String,
}

pub(super) fn keep_notes(folder: &Path, notes: &[ReleaseNote]) {
    let kept: Vec<Kept> = notes
        .iter()
        .map(|note| Kept { version: note.version.clone(), date: note.date.clone(), notes: note.notes.clone() })
        .collect();
    let Ok(text) = serde_json::to_string(&kept) else { return };
    let file = folder.join(NOTES);
    if std::fs::read_to_string(&file).is_ok_and(|saved| saved == text) {
        return;
    }
    let _ = std::fs::create_dir_all(folder);
    let _ = zen_vfs::write_atomic(&file, text.as_bytes());
}

fn notes_of(folder: &Path, version: &str) -> Option<Kept> {
    let text = std::fs::read_to_string(folder.join(NOTES)).ok()?;
    let kept: Vec<Kept> = serde_json::from_str(&text).ok()?;
    kept.into_iter().find(|note| note.version == version)
}

fn page(product: &str, note: &Kept) -> String {
    let title = t!("ide-whats-new-title", product = product, version = note.version.as_str());
    format!("# {title}\n\n{}\n\n{}\n", note.date, note.notes)
}

fn write_page(folder: &Path, product: &str, version: &str) -> Result<PathBuf, String> {
    let note = notes_of(folder, version).ok_or_else(|| t!("ide-whats-new-no-notes", version = version))?;
    let pages = folder.join("release-notes");
    std::fs::create_dir_all(&pages).map_err(|error| error.to_string())?;
    let file = pages.join(format!("{product}-{version}.md"));
    zen_vfs::write_atomic(&file, page(product, &note).as_bytes()).map_err(|error| error.to_string())?;
    Ok(file)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AfterStart {
    Nothing,
    Show,
}

pub(super) fn after_start(folder: &Path, version: &str, wanted: bool) -> AfterStart {
    let file = folder.join(SHOWN);
    let last = std::fs::read_to_string(&file).ok().map(|text| text.trim().to_owned());
    let remember = || {
        let _ = std::fs::create_dir_all(folder);
        let _ = zen_vfs::write_atomic(&file, version.as_bytes());
    };
    match notes_on_start(last.as_deref(), version, false) {
        StartNotes::Nothing => AfterStart::Nothing,
        StartNotes::Show { .. } if wanted => {
            if notes_of(folder, version).is_none() {
                return AfterStart::Nothing;
            }
            remember();
            AfterStart::Show
        }
        StartNotes::Show { .. } | StartNotes::Remember => {
            remember();
            AfterStart::Nothing
        }
    }
}

impl IdeView {
    pub(super) fn open_whats_new(&mut self, registry: &Registry, session: &mut Session) -> Result<Value, String> {
        let folder = self.help.folder.clone().ok_or_else(|| t!("ide-shortcuts-reference-no-folder"))?;
        let file = write_page(&folder, self.product.name, self.product.version)?;
        registry.dispatch(session, "file.document.open", json!({ "path": file })).map_err(|error| error.to_string())?;
        Ok(json!({ "path": file }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(version: &str) -> ReleaseNote {
        ReleaseNote {
            version: version.to_owned(),
            date: "2026-11-05".to_owned(),
            notes: "- Fixed".to_owned(),
            installed: false,
        }
    }

    #[test]
    fn the_notes_of_the_running_version_need_no_network_once_kept() {
        let dir = tempfile::tempdir().unwrap();
        keep_notes(dir.path(), &[note("26.11.1"), note("26.11.2")]);
        let file = write_page(dir.path(), "GoGarden", "26.11.2").unwrap();
        let text = std::fs::read_to_string(file).unwrap();
        assert!(text.contains("26.11.2") && text.contains("- Fixed"), "{text}");
        assert!(write_page(dir.path(), "GoGarden", "26.12.1").unwrap_err().contains("26.12.1"));
    }

    #[test]
    fn it_opens_once_after_an_update_and_never_on_a_first_start() {
        let dir = tempfile::tempdir().unwrap();
        keep_notes(dir.path(), &[note("26.11.1"), note("26.11.2")]);
        assert_eq!(after_start(dir.path(), "26.11.1", true), AfterStart::Nothing, "a first start");
        assert_eq!(after_start(dir.path(), "26.11.1", true), AfterStart::Nothing, "the same version");
        assert_eq!(after_start(dir.path(), "26.11.2", true), AfterStart::Show, "after the update");
        assert_eq!(after_start(dir.path(), "26.11.2", true), AfterStart::Nothing, "only once");
    }

    #[test]
    fn turned_off_it_stays_closed_and_notes_not_there_yet_wait_for_the_next_start() {
        let dir = tempfile::tempdir().unwrap();
        keep_notes(dir.path(), &[note("26.11.2")]);
        assert_eq!(after_start(dir.path(), "26.11.1", true), AfterStart::Nothing);
        assert_eq!(after_start(dir.path(), "26.12.1", true), AfterStart::Nothing, "no notes for it yet");
        assert_eq!(after_start(dir.path(), "26.11.2", false), AfterStart::Nothing, "the option is off");
        keep_notes(dir.path(), &[note("26.12.1")]);
        assert_eq!(
            after_start(dir.path(), "26.12.1", true),
            AfterStart::Show,
            "the update was remembered as not shown"
        );
    }
}
