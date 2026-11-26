use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use zen_lsp::FileOperation;

use crate::language::file_operations::Touched as FileTouch;
use crate::{Workbench, display, inside_project, project_folder};

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(CopyFiles)?;
    registry.register(CutFiles)?;
    registry.register(CopyPaths)?;
    registry.register(PasteFiles)?;
    Ok(())
}

pub trait SystemClipboard: Send + Sync {
    fn files(&self) -> Vec<PathBuf>;
    fn set_files(&self, files: &[PathBuf]) -> bool;
    fn set_text(&self, text: &str) -> bool;
    fn text(&self) -> String;
}

#[derive(Debug, Default)]
pub struct MemoryClipboard {
    contents: Mutex<(Vec<PathBuf>, String)>,
}

impl SystemClipboard for MemoryClipboard {
    fn files(&self) -> Vec<PathBuf> {
        self.contents.lock().unwrap_or_else(PoisonError::into_inner).0.clone()
    }

    fn set_files(&self, files: &[PathBuf]) -> bool {
        let mut contents = self.contents.lock().unwrap_or_else(PoisonError::into_inner);
        contents.0 = files.to_vec();
        contents.1 = files.iter().map(|path| path.display().to_string()).collect::<Vec<_>>().join("\n");
        true
    }

    fn set_text(&self, text: &str) -> bool {
        let mut contents = self.contents.lock().unwrap_or_else(PoisonError::into_inner);
        *contents = (Vec::new(), text.to_owned());
        true
    }

    fn text(&self) -> String {
        self.contents.lock().unwrap_or_else(PoisonError::into_inner).1.clone()
    }
}

pub struct FileClipboard {
    system: Arc<dyn SystemClipboard>,
    copied: Vec<PathBuf>,
    cut: bool,
    on_system: bool,
}

impl Default for FileClipboard {
    fn default() -> Self {
        Self { system: Arc::new(MemoryClipboard::default()), copied: Vec::new(), cut: false, on_system: false }
    }
}

impl FileClipboard {
    pub fn copied(&self) -> (&[PathBuf], bool) {
        (&self.copied, self.cut)
    }

    pub fn can_paste(&self) -> bool {
        let system = existing(self.system.files());
        !system.is_empty() || (!self.on_system && self.copied.iter().any(|path| path.exists()))
    }

    fn forget(&mut self) {
        self.copied.clear();
        self.cut = false;
        self.on_system = false;
    }
}

pub fn clipboard_or_copied_text(session: &mut Session) -> Option<String> {
    let system = Some(session.state_mut::<FileClipboard>().system.text()).filter(|text| !text.is_empty());
    system.or_else(|| {
        session.state_mut::<crate::clipboard_history::ClipboardHistory>().get(0).map(|text| text.to_string())
    })
}

pub(crate) fn copy_text(session: &mut Session, text: &str) -> bool {
    session.state_mut::<FileClipboard>().system.set_text(text)
}

pub fn install(session: &mut Session, system: Arc<dyn SystemClipboard>) {
    session.state_mut::<FileClipboard>().system = system;
}

pub fn clipboard_text(session: &mut Session) -> String {
    session.state_mut::<FileClipboard>().system.text()
}

pub fn set_clipboard_text(session: &mut Session, text: &str) -> bool {
    session.state_mut::<FileClipboard>().system.set_text(text)
}

fn existing(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.into_iter().filter(|path| std::fs::symlink_metadata(path).is_ok()).collect()
}

fn yes() -> bool {
    true
}

/// Files or folders to copy or cut.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilesParams {
    /// Absolute paths of existing files and folders.
    pub paths: Vec<PathBuf>,
    /// Put them on the system clipboard (the default); `false` only remembers them for the IDE's
    /// own Paste, leaving the system clipboard alone.
    #[serde(default = "yes")]
    pub clipboard: bool,
}

/// What was copied or cut.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct FilesCopied {
    /// The files and folders.
    pub paths: Vec<PathBuf>,
    /// Whether they were put on the system clipboard.
    pub clipboard: bool,
}

fn take_files(session: &mut Session, mut params: FilesParams, cut: bool) -> Result<FilesCopied, CommandError> {
    if params.paths.is_empty() {
        return Err(CommandError::new(t!("workbench-error-nothing-selected")));
    }
    if let Some(workbench) = session.state::<Workbench>() {
        params.paths = params.paths.iter().map(|path| workbench.resolve(path)).collect();
    }
    if let Some(root) = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned) {
        crate::hidden_files::guard_change(session, &root, &params.paths)?;
    }
    if let Some(missing) =
        params.paths.iter().find(|path| !path.is_absolute() || std::fs::symlink_metadata(path).is_err())
    {
        return Err(CommandError::new(t!("workbench-error-no-such-file", path = display(missing))));
    }
    if cut {
        let project = session.state::<Workbench>().and_then(|workbench| workbench.project.clone());
        let project = project.ok_or_else(|| CommandError::new(t!("workbench-error-no-project")))?;
        if let Some(outside) = params.paths.iter().find(|path| **path == project || !inside_project(&project, path)) {
            return Err(CommandError::new(t!("workbench-error-outside-project", path = display(outside))));
        }
    }
    let state = session.state_mut::<FileClipboard>();
    let clipboard = params.clipboard && state.system.set_files(&params.paths);
    state.copied = params.paths.clone();
    state.cut = cut;
    state.on_system = clipboard && !existing(state.system.files()).is_empty();
    Ok(FilesCopied { paths: params.paths, clipboard })
}

pub struct CopyFiles;

impl Command for CopyFiles {
    type Params = FilesParams;
    type Output = FilesCopied;
    const ID: &'static str = "project.file.copy";
    const TITLE: &'static str = "Copy";
    const DESCRIPTION: &'static str = "Copies files and folders to the clipboard as files, as the Finder does (the Finder can \
         paste them; `project.file.paste` pastes copies). With `clipboard: false` they are only \
         remembered for `project.file.paste`, and the system clipboard is left alone.";

    fn run(&self, session: &mut Session, params: FilesParams) -> Result<FilesCopied, CommandError> {
        take_files(session, params, false)
    }
}

pub struct CutFiles;

impl Command for CutFiles {
    type Params = FilesParams;
    type Output = FilesCopied;
    const ID: &'static str = "project.file.cut";
    const TITLE: &'static str = "Cut";
    const DESCRIPTION: &'static str = "Cuts files and folders of the project: they go on the clipboard as files, and the next \
         `project.file.paste` moves them instead of copying. With `clipboard: false` the system \
         clipboard is left alone.";

    fn run(&self, session: &mut Session, params: FilesParams) -> Result<FilesCopied, CommandError> {
        take_files(session, params, true)
    }
}

/// The paths copied as text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct PathsCopied {
    /// The paths in the asked form, one per line.
    pub text: String,
    /// Whether it was put on the system clipboard.
    pub clipboard: bool,
}

/// Which form of a path Copy Path/Reference… copies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PathForm {
    /// The absolute path.
    #[default]
    Absolute,
    /// The file's name.
    FileName,
    /// The path from the project's folder.
    FromProject,
    /// The path from the Git repository's root.
    FromRepository,
}

/// What to copy as text.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CopyPathParams {
    /// The files and folders.
    pub paths: Vec<PathBuf>,
    /// Put the text on the system clipboard (the default); `false` only answers it.
    #[serde(default = "yes")]
    pub clipboard: bool,
    /// The form of each path; the absolute path when absent.
    #[serde(default)]
    pub form: PathForm,
}

fn repository_root(path: &Path) -> Option<&Path> {
    path.ancestors().find(|folder| folder.join(".git").exists())
}

fn in_form(session: &Session, path: &Path, form: PathForm) -> String {
    let from = |root: Option<PathBuf>| {
        root.and_then(|root| path.strip_prefix(root).ok().map(Path::to_owned)).unwrap_or_else(|| path.to_owned())
    };
    let shown = match form {
        PathForm::Absolute => path.to_owned(),
        PathForm::FileName => path.file_name().map_or_else(|| path.to_owned(), PathBuf::from),
        PathForm::FromProject => {
            from(session.state::<Workbench>().and_then(|workbench| workbench.project().map(Path::to_owned)))
        }
        PathForm::FromRepository => from(repository_root(path).map(Path::to_owned)),
    };
    shown.display().to_string()
}

pub struct CopyPaths;

impl Command for CopyPaths {
    type Params = CopyPathParams;
    type Output = PathsCopied;
    const ID: &'static str = "project.file.copyPath";
    const TITLE: &'static str = "Copy Path";
    const DESCRIPTION: &'static str = "Copies the paths of files and folders to the clipboard as text, one per line, and \
         answers with the text: absolute by default, or with `form` the file name (`fileName`), the path from the \
         project (`fromProject`) or from the Git repository's root (`fromRepository`); with `clipboard: false` it \
         only answers.";

    fn run(&self, session: &mut Session, params: CopyPathParams) -> Result<PathsCopied, CommandError> {
        if params.paths.is_empty() {
            return Err(CommandError::new(t!("workbench-error-nothing-selected")));
        }
        let text = params.paths.iter().map(|path| in_form(session, path, params.form)).collect::<Vec<_>>().join("\n");
        let clipboard = params.clipboard && session.state_mut::<FileClipboard>().system.set_text(&text);
        Ok(PathsCopied { text, clipboard })
    }
}

/// Where to paste, and optionally what.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PasteParams {
    /// The project folder to paste into.
    pub directory: PathBuf,
    /// The files and folders to paste; when left out, those on the clipboard.
    #[serde(default)]
    pub sources: Option<Vec<PathBuf>>,
    /// Read the system clipboard (the default); `false` pastes only what was copied or cut in the
    /// IDE, leaving the system clipboard alone.
    #[serde(default = "yes")]
    pub clipboard: bool,
}

/// What was pasted.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Pasted {
    /// The new paths, in the order of the sources.
    pub paths: Vec<PathBuf>,
    /// Whether the files were moved (after Cut) rather than copied.
    pub moved: bool,
}

pub struct PasteFiles;

impl Command for PasteFiles {
    type Params = PasteParams;
    type Output = Pasted;
    const ID: &'static str = "project.file.paste";
    const TITLE: &'static str = "Paste";
    const DESCRIPTION: &'static str = "Pastes files and folders into a project folder: those given in `sources`, or else those on \
         the clipboard (copied in the IDE or in the Finder; files copied in the IDE are forgotten once \
         something else is copied over them). Copies whose name is taken are named as \
         the Finder names them (`main copy.go`, `main copy 2.go`); nothing is overwritten. After \
         `project.file.cut` the files are moved instead, once, and language servers that ask to hear of \
         moves update what refers to them first (one Undo in a changed file reverts that and the move). \
         With `clipboard: false` only what was copied or cut in the IDE is pasted.";

    fn run(&self, session: &mut Session, params: PasteParams) -> Result<Pasted, CommandError> {
        let directory = project_folder(session.state_mut::<Workbench>(), &params.directory)?;
        let state = session.state_mut::<FileClipboard>();
        let sources = match params.sources {
            Some(sources) => sources,
            None => {
                let system = if params.clipboard { existing(state.system.files()) } else { Vec::new() };
                if !system.is_empty() {
                    system
                } else if params.clipboard && state.on_system {
                    state.forget();
                    Vec::new()
                } else {
                    existing(state.copied.clone())
                }
            }
        };
        if sources.is_empty() {
            return Err(CommandError::new(t!("workbench-paste-nothing")));
        }
        let sources: Vec<PathBuf> = match session.state::<Workbench>() {
            Some(workbench) => sources.iter().map(|source| workbench.resolve(source)).collect(),
            None => sources,
        };
        let state = session.state_mut::<FileClipboard>();
        let moved = state.cut && same_files(&sources, &state.copied);
        if let Some(root) = session.state::<Workbench>().and_then(Workbench::project).map(Path::to_owned) {
            crate::hidden_files::guard_copy(session, &root, &sources, &directory)?;
        }
        let state = session.state_mut::<FileClipboard>();
        if moved {
            state.forget();
        }
        let touched: Vec<FileTouch> = if moved {
            sources
                .iter()
                .filter(|source| source.parent() != Some(directory.as_path()))
                .filter_map(|source| {
                    let name = source.file_name()?;
                    Some(FileTouch::existing(source, Some(directory.join(name))))
                })
                .collect()
        } else {
            Vec::new()
        };
        let asked = crate::language::file_operations::will(session, FileOperation::Rename, &touched);
        let workbench = session.state_mut::<Workbench>();
        let mut pasted = Vec::new();
        let mut relocated = Vec::new();
        let mut failure = None;
        for source in &sources {
            let result =
                if moved { zen_vfs::move_into(source, &directory) } else { zen_vfs::copy_into(source, &directory) };
            match result {
                Ok(path) => {
                    if moved && path != *source {
                        workbench.relocated(source, &path);
                        relocated.push((source.clone(), path.clone()));
                    }
                    pasted.push(path);
                }
                Err(error) => {
                    let message = if moved {
                        t!("workbench-error-move", path = display(&error.path), error = error.source.to_string())
                    } else {
                        t!("workbench-error-copy", path = display(&error.path), error = error.source.to_string())
                    };
                    failure = Some(message);
                }
            }
        }
        for (from, to) in &relocated {
            session.state_mut::<crate::history::History>().relocate(from, to);
        }
        let workbench = session.state_mut::<Workbench>();
        if !pasted.is_empty() {
            workbench.revision += 1;
            let count = pasted.len();
            workbench.messages.push(if moved {
                t!("workbench-moved", count = count)
            } else {
                t!("workbench-copied", count = count)
            });
        }
        if let Some(error) = failure.as_ref().filter(|_| !pasted.is_empty()) {
            workbench.messages.push(error.clone());
        }
        for (from, to) in &relocated {
            crate::bookmarks::relocate(session, from, to);
            crate::file_types::relocate_overrides(session, from, Some(to));
        }
        if moved {
            let told: Vec<FileTouch> = touched
                .into_iter()
                .filter_map(|file| {
                    let (_, to) = relocated.iter().find(|(from, _)| *from == file.path)?;
                    Some(FileTouch { new_path: Some(to.clone()), ..file })
                })
                .collect();
            crate::language::file_operations::did(session, FileOperation::Rename, &told);
            crate::language::file_operations::remember(session, asked, &relocated);
        } else {
            let made: Vec<FileTouch> = pasted.iter().map(|path| FileTouch::existing(path, None)).collect();
            crate::language::file_operations::did(session, FileOperation::Create, &made);
        }
        match failure {
            Some(error) if pasted.is_empty() => Err(CommandError::new(error)),
            _ => Ok(Pasted { paths: pasted, moved }),
        }
    }
}

fn same_files(a: &[PathBuf], b: &[PathBuf]) -> bool {
    fn sorted(paths: &[PathBuf]) -> Vec<&Path> {
        let mut paths: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
        paths.sort();
        paths
    }
    !a.is_empty() && sorted(a) == sorted(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn setup() -> (tempfile::TempDir, PathBuf, Registry, Session, Arc<MemoryClipboard>) {
        let dir = tempfile::tempdir().unwrap();
        let project = std::fs::canonicalize(dir.path()).unwrap().join("project");
        std::fs::create_dir_all(project.join("pkg")).unwrap();
        std::fs::create_dir_all(project.join("docs")).unwrap();
        std::fs::write(project.join("main.go"), "package main").unwrap();
        std::fs::write(project.join("pkg/util.go"), "package pkg").unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        let fake = Arc::new(MemoryClipboard::default());
        install(&mut session, fake.clone());
        (dir, project, registry, session, fake)
    }

    fn run(registry: &Registry, session: &mut Session, id: &str, params: Value) -> Value {
        registry.dispatch(session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    #[test]
    fn a_session_without_a_window_never_reaches_the_system_clipboard() {
        let state = FileClipboard::default();
        assert!(state.system.set_files(&[PathBuf::from("/x")]));
        assert_eq!(state.system.files(), vec![PathBuf::from("/x")]);
    }

    #[test]
    fn copy_puts_several_files_on_the_clipboard_and_paste_names_copies_as_the_finder() {
        let (_dir, project, registry, mut session, fake) = setup();
        let main = project.join("main.go");
        let pkg = project.join("pkg");
        let copied = run(&registry, &mut session, "project.file.copy", json!({ "paths": [main, pkg] }));
        assert_eq!(copied, json!({ "paths": [main, pkg], "clipboard": true }));
        assert_eq!(fake.files(), vec![main.clone(), pkg.clone()]);

        let pasted = run(&registry, &mut session, "project.file.paste", json!({ "directory": project }));
        assert_eq!(
            pasted,
            json!({ "paths": [project.join("main copy.go"), project.join("pkg copy")], "moved": false })
        );
        assert_eq!(std::fs::read_to_string(project.join("pkg copy/util.go")).unwrap(), "package pkg");
        let again = run(&registry, &mut session, "project.file.paste", json!({ "directory": project }));
        assert_eq!(again["paths"][0], json!(project.join("main copy 2.go")));
        let docs = run(&registry, &mut session, "project.file.paste", json!({ "directory": project.join("docs") }));
        assert_eq!(docs["paths"], json!([project.join("docs/main.go"), project.join("docs/pkg")]));
        assert!(main.exists(), "copying keeps the originals");
    }

    #[test]
    fn files_copied_in_the_finder_are_pasted_from_the_clipboard() {
        let (dir, project, registry, mut session, fake) = setup();
        let outside = dir.path().join("notes.txt");
        std::fs::write(&outside, "notes").unwrap();
        fake.set_files(&[outside.clone(), dir.path().join("gone.txt")]);
        let pasted = run(&registry, &mut session, "project.file.paste", json!({ "directory": project.join("docs") }));
        assert_eq!(pasted, json!({ "paths": [project.join("docs/notes.txt")], "moved": false }));
        assert!(outside.exists());
    }

    #[test]
    fn something_copied_elsewhere_replaces_files_copied_in_the_ide() {
        let (_dir, project, registry, mut session, fake) = setup();
        let main = project.join("main.go");
        run(&registry, &mut session, "project.file.copy", json!({ "paths": [main] }));
        assert!(session.state::<FileClipboard>().unwrap().can_paste());

        fake.set_text("");
        assert!(!session.state::<FileClipboard>().unwrap().can_paste(), "an image can be pasted instead");
        let error = registry
            .dispatch(&mut session, "project.file.paste", json!({ "directory": project }))
            .expect_err("the old file is not pasted");
        assert!(!project.join("main copy.go").exists(), "{error}");
    }

    #[test]
    fn a_cut_is_dropped_once_something_else_was_copied() {
        let (_dir, project, registry, mut session, fake) = setup();
        let main = project.join("main.go");
        run(&registry, &mut session, "project.file.cut", json!({ "paths": [main] }));

        fake.set_text("");
        assert!(
            registry
                .dispatch(&mut session, "project.file.paste", json!({ "directory": project.join("docs") }))
                .is_err()
        );
        assert!(main.exists(), "the file stays where it was");
        assert_eq!(session.state::<FileClipboard>().unwrap().copied(), (&[][..], false));
    }

    #[test]
    fn a_clipboard_that_cannot_give_files_back_still_pastes_what_was_copied() {
        struct WriteOnly;
        impl SystemClipboard for WriteOnly {
            fn files(&self) -> Vec<PathBuf> {
                Vec::new()
            }
            fn set_files(&self, _files: &[PathBuf]) -> bool {
                true
            }
            fn set_text(&self, _text: &str) -> bool {
                true
            }
            fn text(&self) -> String {
                String::new()
            }
        }
        let (_dir, project, registry, mut session, _fake) = setup();
        install(&mut session, Arc::new(WriteOnly));
        let main = project.join("main.go");
        run(&registry, &mut session, "project.file.copy", json!({ "paths": [main] }));

        assert!(session.state::<FileClipboard>().unwrap().can_paste());
        let pasted = run(&registry, &mut session, "project.file.paste", json!({ "directory": project }));
        assert_eq!(pasted["paths"], json!([project.join("main copy.go")]));
    }

    #[test]
    fn files_only_remembered_by_the_ide_stay_until_pasted() {
        let (_dir, project, registry, mut session, fake) = setup();
        let main = project.join("main.go");
        run(&registry, &mut session, "project.file.copy", json!({ "paths": [main], "clipboard": false }));
        fake.set_text("");

        assert!(session.state::<FileClipboard>().unwrap().can_paste());
        let pasted = run(&registry, &mut session, "project.file.paste", json!({ "directory": project }));
        assert_eq!(pasted["paths"], json!([project.join("main copy.go")]));
    }

    #[test]
    fn copy_path_reference_gives_each_form_of_the_path() {
        let (_dir, project, registry, mut session, _fake) = setup();
        let util = project.join("pkg/util.go");
        std::fs::create_dir(project.join(".git")).unwrap();
        let copied = |session: &mut Session, form: &str| {
            let params = json!({ "paths": [util], "clipboard": false, "form": form });
            run(&registry, session, "project.file.copyPath", params)["text"].as_str().unwrap().to_owned()
        };
        assert_eq!(copied(&mut session, "absolute"), util.display().to_string());
        assert_eq!(copied(&mut session, "fileName"), "util.go");
        assert_eq!(copied(&mut session, "fromProject"), Path::new("pkg").join("util.go").display().to_string());
        assert_eq!(copied(&mut session, "fromRepository"), Path::new("pkg").join("util.go").display().to_string());
    }

    #[test]
    fn cut_then_paste_moves_once_and_open_tabs_follow() {
        let (_dir, project, registry, mut session, fake) = setup();
        let util = project.join("pkg/util.go");
        run(&registry, &mut session, "file.document.open", json!({ "path": util }));
        let cut = run(&registry, &mut session, "project.file.cut", json!({ "paths": [util] }));
        assert_eq!(cut["clipboard"], json!(true));
        assert_eq!(fake.files(), vec![util.clone()]);
        let pasted = run(&registry, &mut session, "project.file.paste", json!({ "directory": project.join("docs") }));
        let moved = project.join("docs/util.go");
        assert_eq!(pasted, json!({ "paths": [moved], "moved": true }));
        assert!(!util.exists() && moved.exists());
        let workbench = session.state::<Workbench>().unwrap();
        assert_eq!(workbench.active_document().map(|document| document.path().to_owned()), Some(moved.clone()));
        assert!(registry.dispatch(&mut session, "project.file.paste", json!({ "directory": project })).is_err());
    }

    #[test]
    fn cut_only_takes_project_files() {
        let (dir, _project, registry, mut session, fake) = setup();
        let outside = dir.path().join("x.txt");
        std::fs::write(&outside, "x").unwrap();
        assert!(registry.dispatch(&mut session, "project.file.cut", json!({ "paths": [outside] })).is_err());
        assert!(fake.files().is_empty());
    }

    #[test]
    fn without_the_clipboard_only_the_ides_own_copy_is_used() {
        let (dir, project, registry, mut session, fake) = setup();
        let other = dir.path().join("other.txt");
        std::fs::write(&other, "o").unwrap();
        fake.set_files(&[other]);
        let main = project.join("main.go");
        let copied = run(&registry, &mut session, "project.file.copy", json!({ "paths": [main], "clipboard": false }));
        assert_eq!(copied["clipboard"], json!(false));
        assert_eq!(fake.files().len(), 1, "the clipboard is left alone");
        let pasted = run(
            &registry,
            &mut session,
            "project.file.paste",
            json!({ "directory": project.join("docs"), "clipboard": false }),
        );
        assert_eq!(pasted["paths"], json!([project.join("docs/main.go")]));
        let given = run(
            &registry,
            &mut session,
            "project.file.paste",
            json!({ "directory": project.join("docs"), "sources": [project.join("pkg/util.go")], "clipboard": false }),
        );
        assert_eq!(given["paths"], json!([project.join("docs/util.go")]));
    }

    #[test]
    fn copy_path_gives_absolute_paths_one_per_line() {
        let (_dir, project, registry, mut session, fake) = setup();
        let paths = [project.join("main.go"), project.join("pkg")];
        let copied = run(&registry, &mut session, "project.file.copyPath", json!({ "paths": paths }));
        let text = format!("{}\n{}", paths[0].display(), paths[1].display());
        assert_eq!(copied, json!({ "text": text, "clipboard": true }));
        assert_eq!(fake.text(), text);
        let answered =
            run(&registry, &mut session, "project.file.copyPath", json!({ "paths": [paths[0]], "clipboard": false }));
        assert_eq!(answered["clipboard"], json!(false));
        assert_eq!(fake.text(), text, "left alone");
    }

    #[test]
    fn paste_refuses_folders_outside_the_project_and_nothing_to_paste() {
        let (dir, project, registry, mut session, _fake) = setup();
        assert!(registry.dispatch(&mut session, "project.file.paste", json!({ "directory": project })).is_err());
        run(&registry, &mut session, "project.file.copy", json!({ "paths": [project.join("main.go")] }));
        assert!(registry.dispatch(&mut session, "project.file.paste", json!({ "directory": dir.path() })).is_err());
    }

    #[test]
    fn rename_renames_in_place_and_tabs_follow() {
        let (_dir, project, registry, mut session, _fake) = setup();
        let util = project.join("pkg/util.go");
        run(&registry, &mut session, "file.document.open", json!({ "path": util }));
        let renamed =
            run(&registry, &mut session, "file.fs.rename", json!({ "path": project.join("pkg"), "name": "lib" }));
        assert_eq!(renamed, json!(project.join("lib")));
        let workbench = session.state::<Workbench>().unwrap();
        assert_eq!(
            workbench.active_document().map(|document| document.path().to_owned()),
            Some(project.join("lib/util.go"))
        );
        assert!(
            registry
                .dispatch(&mut session, "file.fs.rename", json!({ "path": project.join("main.go"), "name": "lib" }))
                .is_err()
        );
        assert!(registry.dispatch(&mut session, "file.fs.rename", json!({ "path": project, "name": "x" })).is_err());
    }
}
