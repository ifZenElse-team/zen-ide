use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::vcs::commit::{DiffView, diff_view, diff_view_with};
use crate::{PathParams, Workbench, display};

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(CompareFiles)?;
    registry.register(CompareWithEditor)?;
    registry.register(Reveal)?;
    Ok(())
}

/// Two files to compare.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct CompareParams {
    /// The file shown on the left.
    pub left: PathBuf,
    /// The file shown on the right.
    pub right: PathBuf,
    /// How whitespace counts; every character by default.
    #[serde(default)]
    pub whitespace: crate::compare::Whitespace,
}

fn read(path: &Path) -> Result<Vec<u8>, CommandError> {
    if path.is_dir() {
        return Err(CommandError::new(t!("workbench-error-compare-folder", path = display(path))));
    }
    std::fs::read(path)
        .map_err(|error| CommandError::new(t!("workbench-error-open", path = display(path), error = error.to_string())))
}

fn title(path: &Path) -> String {
    path.file_name().map_or_else(|| display(path), |name| name.to_string_lossy().into_owned())
}

pub struct CompareFiles;

impl Command for CompareFiles {
    type Params = CompareParams;
    type Output = DiffView;
    const ID: &'static str = "file.compare.files";
    const TITLE: &'static str = "Compare With…";
    const DESCRIPTION: &'static str = "Compares two files, as the diff viewer shows them: both texts and the changed hunks from \
         `left` to `right` (`binary` when one is not text); `whitespace` (`none`, `trim`, `ignore`) says how \
         whitespace counts. Three files: file.compare.three.";

    fn run(&self, _session: &mut Session, params: CompareParams) -> Result<DiffView, CommandError> {
        let (before, after) = (read(&params.left)?, read(&params.right)?);
        let titles = (display(&params.left), display(&params.right));
        Ok(diff_view_with(params.right.clone(), title(&params.right), titles, &before, &after, params.whitespace))
    }
}

pub struct CompareWithEditor;

impl Command for CompareWithEditor {
    type Params = PathParams;
    type Output = DiffView;
    const ID: &'static str = "file.compare.editor";
    const TITLE: &'static str = "Compare File with Editor";
    const DESCRIPTION: &'static str = "Compares a file on disk (left) with the text of the file in the active editor tab (right), \
         unsaved changes included.";

    fn run(&self, session: &mut Session, params: PathParams) -> Result<DiffView, CommandError> {
        let before = read(&params.path)?;
        let workbench = session.state::<Workbench>();
        let document = workbench
            .and_then(Workbench::active_document)
            .filter(|document| document.image.is_none())
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let titles = (display(&params.path), t!("workbench-compare-editor", file = title(document.path())));
        Ok(diff_view(document.path().to_owned(), title(document.path()), titles, &before, document.text().as_bytes()))
    }

    fn is_available(&self, session: &Session) -> bool {
        session.state::<Workbench>().and_then(Workbench::active_document).is_some()
    }
}

/// A file or folder to show in the file manager.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RevealParams {
    /// An absolute path.
    pub path: PathBuf,
    /// Only answer with what would run, without opening the file manager.
    #[serde(default)]
    pub dry_run: bool,
}

/// The file manager's command line.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Revealed {
    /// The program.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// Whether it was started.
    pub ran: bool,
}

fn reveal_command(path: &Path) -> (String, Vec<String>) {
    let path = display(path);
    if cfg!(target_os = "macos") {
        ("open".to_owned(), vec!["-R".to_owned(), path])
    } else if cfg!(target_os = "windows") {
        ("explorer".to_owned(), vec![format!("/select,{path}")])
    } else {
        let folder = Path::new(&path).parent().map_or(path.clone(), display);
        ("xdg-open".to_owned(), vec![folder])
    }
}

pub struct Reveal;

impl Command for Reveal {
    type Params = RevealParams;
    type Output = Revealed;
    const ID: &'static str = "file.fs.reveal";
    const TITLE: &'static str = "Open In Finder";
    const DESCRIPTION: &'static str = "Shows a file or folder in the system's file manager (the Finder on macOS), selected in its \
         folder. With `dryRun: true` only answers with the command line.";

    fn run(&self, _session: &mut Session, params: RevealParams) -> Result<Revealed, CommandError> {
        reveal(&params.path, params.dry_run)
    }
}

pub(crate) fn reveal(path: &Path, dry_run: bool) -> Result<Revealed, CommandError> {
    if std::fs::symlink_metadata(path).is_err() {
        return Err(CommandError::new(t!("workbench-error-no-such-file", path = display(path))));
    }
    let (program, args) = reveal_command(path);
    let ran = !dry_run;
    if ran {
        #[cfg(not(target_arch = "wasm32"))]
        std::process::Command::new(&program)
            .args(&args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| CommandError::new(t!("workbench-error-reveal", error = error.to_string())))?;
    }
    Ok(Revealed { program, args, ran })
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use zen_core::Registry;

    use super::*;

    fn session(project: &Path) -> (Registry, Session) {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        (registry, session)
    }

    #[test]
    fn two_files_are_compared_left_to_right() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
        std::fs::write(&a, "one\ntwo\n").unwrap();
        std::fs::write(&b, "one\nthree\n").unwrap();
        let (registry, mut session) = session(dir.path());
        let view = registry.dispatch(&mut session, "file.compare.files", json!({ "left": a, "right": b })).unwrap();
        assert_eq!(view["before"], "one\ntwo\n");
        assert_eq!(view["after"], "one\nthree\n");
        assert_eq!(view["hunks"].as_array().unwrap().len(), 1);
        assert_eq!(view["beforeTitle"], json!(a.display().to_string()));
        assert!(
            registry.dispatch(&mut session, "file.compare.files", json!({ "left": a, "right": dir.path() })).is_err()
        );
    }

    #[test]
    fn a_file_is_compared_with_the_active_editor_and_its_unsaved_text() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
        std::fs::write(&a, "one\n").unwrap();
        std::fs::write(&b, "two\n").unwrap();
        let (registry, mut session) = session(dir.path());
        assert!(registry.dispatch(&mut session, "file.compare.editor", json!({ "path": a })).is_err(), "no editor");
        registry.dispatch(&mut session, "file.document.open", json!({ "path": b })).unwrap();
        registry.dispatch(&mut session, "editor.edit.insert", json!({ "text": "new " })).unwrap();
        let view = registry.dispatch(&mut session, "file.compare.editor", json!({ "path": a })).unwrap();
        assert_eq!(view["before"], "one\n");
        assert_eq!(view["after"], "new two\n");
    }

    #[test]
    fn reveal_names_the_file_manager_and_a_dry_run_starts_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "a").unwrap();
        let (registry, mut session) = session(dir.path());
        let revealed =
            registry.dispatch(&mut session, "file.fs.reveal", json!({ "path": file, "dryRun": true })).unwrap();
        assert_eq!(revealed["ran"], json!(false));
        if cfg!(target_os = "macos") {
            assert_eq!(revealed["program"], "open");
            assert_eq!(revealed["args"], json!(["-R", file.display().to_string()]));
        }
        assert!(
            registry
                .dispatch(&mut session, "file.fs.reveal", json!({ "path": dir.path().join("gone"), "dryRun": true }))
                .is_err()
        );
    }
}
