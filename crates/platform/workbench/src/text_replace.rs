use std::ops::Range;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::Workbench;
use crate::language::char_offset;

pub const NOT_FOUND: &str = "not_found";
pub const NOT_UNIQUE: &str = "not_unique";
pub const OVERLAP: &str = "overlap";
pub const INVALID_REGEX: &str = "invalid_regex";

/// One replacement in a file.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextEdit {
    /// The exact text to find, unsaved changes included; not empty.
    pub old_text: String,
    /// The text that takes its place.
    pub new_text: String,
    /// How many times `oldText` must occur: every occurrence is replaced. 1 when absent.
    #[serde(default)]
    pub expected_count: Option<usize>,
    /// Whether `oldText` is a regex (`^`/`$` at each line) and `newText` may use its groups
    /// (`${1}`, `${name}`; `$$` is `$`).
    #[serde(default)]
    pub regex: bool,
}

/// What `editor.text.replace` takes: one edit (`oldText`, `newText`, `expectedCount`, `regex`) or `edits`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplaceParams {
    /// The file: absolute, or relative to the project.
    pub path: PathBuf,
    /// The exact text to find; with `newText`, the one edit of the call.
    #[serde(default)]
    pub old_text: Option<String>,
    /// The text that takes the place of `oldText`.
    #[serde(default)]
    pub new_text: Option<String>,
    /// How many times `oldText` must occur; 1 when absent.
    #[serde(default)]
    pub expected_count: Option<usize>,
    /// Whether `oldText` is a regular expression, as in an edit.
    #[serde(default)]
    pub regex: bool,
    /// Several edits, each found in the text before the call: all made, as one undo step, or none.
    #[serde(default)]
    pub edits: Vec<TextEdit>,
    /// Save, with the on-save actions; an agent's edit by default, without them.
    #[serde(default)]
    pub save: Option<bool>,
    /// Adds newProblems: the errors the edit brought.
    #[serde(default)]
    pub check_problems: bool,
    /// checkProblems' longest wait, in ms (3000).
    #[serde(default)]
    pub check_ms: Option<u64>,
}

/// What `editor.text.replace` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceResult {
    /// Whether the text changed (false when every new text equals its old text).
    pub changed: bool,
    /// The line of the first replacement, one-based.
    pub line: usize,
    /// How many places were replaced, when more than one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaced: Option<usize>,
    /// Whether the file was saved.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub saved: bool,
    /// With `checkProblems`: the errors and warnings the edit brought, as rows `line:column
    /// severity message [source code]`; empty when it brought none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_problems: Option<Vec<String>>,
    /// With `checkProblems`, when the file was not checked in time or cannot be: why
    /// (`checking`, `notChecked`, `noServer`, `serverMissing`...); `newProblems` then lists what was known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unchecked: Option<crate::language::Reason>,
}

/// What `editor.text.write` takes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteParams {
    /// The file: absolute or relative to the project; a missing project file is created with its folders.
    pub path: PathBuf,
    /// The whole text the file gets.
    pub text: String,
    /// Save afterwards (an agent's edit by default); a created file is on disk at once.
    #[serde(default)]
    pub save: Option<bool>,
    /// Adds newProblems: the errors the edit brought.
    #[serde(default)]
    pub check_problems: bool,
    /// checkProblems' longest wait, in ms (3000).
    #[serde(default)]
    pub check_ms: Option<u64>,
}

/// What `editor.text.write` answers, and `editor.text.batch` for each file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FileResult {
    /// The file, relative to the project when it is in it.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
    /// Whether the text changed.
    pub changed: bool,
    /// The line of the first change, one-based.
    pub line: usize,
    /// How many places were replaced, when more than one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaced: Option<usize>,
    /// Whether the file was created.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub created: bool,
    /// Whether the file was saved.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub saved: bool,
    /// With `checkProblems`: the errors and warnings the edit brought, as rows `line:column
    /// severity message [source code]`; empty when it brought none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_problems: Option<Vec<String>>,
    /// With `checkProblems`, when the file was not checked in time or cannot be: why
    /// (`checking`, `notChecked`, `noServer`, `serverMissing`...); `newProblems` then lists what was known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unchecked: Option<crate::language::Reason>,
}

/// One file of `editor.text.batch`: replaces, as `editor.text.replace` takes them, or the whole
/// `text`, as `editor.text.write` takes it.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FileEdit {
    /// The file: absolute, or relative to the project.
    pub path: PathBuf,
    /// The exact text to find; with `newText`, one edit of the file.
    #[serde(default)]
    pub old_text: Option<String>,
    /// The text that takes the place of `oldText`.
    #[serde(default)]
    pub new_text: Option<String>,
    /// How many times `oldText` must occur; 1 when absent.
    #[serde(default)]
    pub expected_count: Option<usize>,
    /// Whether `oldText` is a regular expression.
    #[serde(default)]
    pub regex: bool,
    /// Several edits of the file.
    #[serde(default)]
    pub edits: Vec<TextEdit>,
    /// The whole text instead of edits; a missing project file is created.
    #[serde(default)]
    pub text: Option<String>,
}

/// What `editor.text.batch` takes.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BatchParams {
    /// The files and what to do to each: every file changes, or none does.
    pub files: Vec<FileEdit>,
    /// Whether to save the changed files afterwards.
    #[serde(default)]
    pub save: Option<bool>,
    /// Adds newProblems: the errors the edit brought.
    #[serde(default)]
    pub check_problems: bool,
    /// checkProblems' longest wait, in ms (3000).
    #[serde(default)]
    pub check_ms: Option<u64>,
}

/// What `editor.text.batch` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BatchResult {
    /// Each file, in the order given.
    pub files: Vec<FileResult>,
}

pub struct ReplaceText;

impl Command for ReplaceText {
    type Params = ReplaceParams;
    type Output = ReplaceResult;
    const ID: &'static str = "editor.text.replace";
    const TITLE: &'static str = "Replace Text";
    const DESCRIPTION: &'static str = "Replaces exact text in a file in one call, for agents: finds `oldText` in the \
         file as the editor has it (unsaved changes included; opens it when needed), checks it occurs `expectedCount` \
         times (1 by default) and replaces every occurrence with `newText` as one undo step; `regex` makes `oldText` a \
         regular expression (`^`/`$` at line ends, `${1}` in `newText`); `edits` makes several replacements of the file \
         at once, all or none; `save` saves afterwards with the on-save actions. In a CRLF or CR file, a line feed in \
         the texts stands for the file's line separator. Answers `{changed, line}`; errors \
         `not_found`, `not_unique` (with `count`), `overlap` and `invalid_regex`. `checkProblems` adds `newProblems`, the errors it brought. editor.text.write gives a file its \
         whole text, editor.text.batch edits several files.";

    fn run(&self, session: &mut Session, params: ReplaceParams) -> Result<ReplaceResult, CommandError> {
        let edit = FileEdit {
            path: params.path,
            old_text: params.old_text,
            new_text: params.new_text,
            expected_count: params.expected_count,
            regex: params.regex,
            edits: params.edits,
            text: None,
        };
        let planned = plan(session, edit)?;
        let check = Check::before(session, params.check_problems, params.check_ms, std::slice::from_ref(&planned));
        let save = saving(session, params.save);
        let mut result = apply(session, &planned, save)?;
        check.after(session, std::slice::from_mut(&mut result));
        Ok(ReplaceResult {
            changed: result.changed,
            line: result.line,
            replaced: result.replaced,
            saved: result.saved,
            new_problems: result.new_problems,
            unchecked: result.unchecked,
        })
    }
}

pub struct WriteText;

impl Command for WriteText {
    type Params = WriteParams;
    type Output = FileResult;
    const ID: &'static str = "editor.text.write";
    const TITLE: &'static str = "Write File";
    const DESCRIPTION: &'static str = "Gives a file its whole text in one call, for agents: a file of the project that \
         does not exist is created (with the folders on the way) and opened; an existing one gets the text in the \
         editor as one undo step. `save` saves afterwards with the on-save actions. Answers `{changed, line, \
         created?, saved?}`.";

    fn run(&self, session: &mut Session, params: WriteParams) -> Result<FileResult, CommandError> {
        let edit = FileEdit { path: params.path, text: Some(params.text), ..FileEdit::default() };
        let planned = plan(session, edit)?;
        let check = Check::before(session, params.check_problems, params.check_ms, std::slice::from_ref(&planned));
        let save = saving(session, params.save);
        let mut result = apply(session, &planned, save)?;
        check.after(session, std::slice::from_mut(&mut result));
        result.path.clear();
        Ok(result)
    }
}

pub struct BatchEdit;

impl Command for BatchEdit {
    type Params = BatchParams;
    type Output = BatchResult;
    const ID: &'static str = "editor.text.batch";
    const TITLE: &'static str = "Edit Files";
    const DESCRIPTION: &'static str = "Edits several files in one call, for agents: each of `files` takes the edits of \
         editor.text.replace (`oldText`/`newText`, `regex`, `edits`) or a whole `text` as editor.text.write does \
         (creating a missing file). Every file changes, or none does: an error names the file by its index \
         (`file`). `save` saves the changed files. Answers `{files: [{path, changed, line, replaced?, created?}]}`.";

    fn run(&self, session: &mut Session, params: BatchParams) -> Result<BatchResult, CommandError> {
        if params.files.is_empty() {
            return Err(CommandError::new(t!("workbench-replace-nothing")));
        }
        let mut planned = Vec::with_capacity(params.files.len());
        for (index, edit) in params.files.into_iter().enumerate() {
            let file = plan(session, edit).map_err(|error| in_file(error, index))?;
            if planned.iter().any(|other: &Planned| other.path == file.path) {
                let name = file.name.clone();
                return Err(in_file(
                    CommandError::with_code(OVERLAP, t!("workbench-replace-overlap", file = name)),
                    index,
                ));
            }
            planned.push(file);
        }
        let saving = saving(session, params.save);
        let save = saving != Saving::No;
        if save {
            for (index, file) in planned.iter().enumerate() {
                refuse_saving(session, file).map_err(|error| in_file(error, index))?;
            }
        }
        let before = if save { texts_of(session, &planned) } else { Vec::new() };
        let check = Check::before(session, params.check_problems, params.check_ms, &planned);
        let mut files = Vec::with_capacity(planned.len());
        for (index, file) in planned.iter().enumerate() {
            match apply(session, file, Saving::No) {
                Ok(result) => files.push(result),
                Err(error) => {
                    undo(session, &planned[..index]);
                    return Err(in_file(error, index));
                }
            }
        }
        if save {
            for (index, (file, result)) in planned.iter().zip(files.iter_mut()).enumerate() {
                match save_changed(session, file, result, saving) {
                    Ok(saved) => result.saved = saved,
                    Err(error) => {
                        undo(session, &planned);
                        let done: Vec<bool> = files.iter().map(|file| file.saved).collect();
                        return Err(in_file(take_back(session, &planned, before, &done).unwrap_or(error), index));
                    }
                }
            }
        }
        check.after(session, &mut files);
        Ok(BatchResult { files })
    }
}

const CHECK_WAIT: u64 = 3_000;
const MAX_CHECK_WAIT: u64 = 10_000;

struct Check {
    wait: Option<std::time::Duration>,
    before: Vec<(PathBuf, Vec<crate::language::Problem>)>,
}

impl Check {
    fn before(session: &mut Session, asked: bool, check_ms: Option<u64>, planned: &[Planned]) -> Self {
        if !asked {
            return Self { wait: None, before: Vec::new() };
        }
        let wait = std::time::Duration::from_millis(check_ms.unwrap_or(CHECK_WAIT).min(MAX_CHECK_WAIT));
        let before = planned.iter().map(|file| (file.path.clone(), reported(session, &file.path))).collect();
        Self { wait: Some(wait), before }
    }

    fn after(self, session: &mut Session, results: &mut [FileResult]) {
        let Some(wait) = self.wait else { return };
        let deadline = std::time::Instant::now() + wait;
        for ((path, before), result) in self.before.into_iter().zip(results.iter_mut()) {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let unchecked = crate::language::wait_checked(session, &path, left);
            let mut known = before;
            let brought = reported(session, &path)
                .into_iter()
                .filter(|problem| {
                    let same = known.iter().position(|old| {
                        (old.severity, &old.message, &old.source, &old.code)
                            == (problem.severity, &problem.message, &problem.source, &problem.code)
                    });
                    same.map(|index| known.remove(index)).is_none()
                })
                .map(|problem| crate::language::problem_row(&problem))
                .collect();
            result.new_problems = Some(brought);
            result.unchecked = unchecked;
        }
    }
}

fn reported(session: &mut Session, path: &Path) -> Vec<crate::language::Problem> {
    let mut problems = crate::inspections::all_problems(session, path);
    problems.retain(|problem| problem.severity <= crate::language::Severity::Warning);
    problems
}

fn in_file(error: CommandError, index: usize) -> CommandError {
    let mut data = error.data.clone().unwrap_or_default();
    data.insert("file".to_owned(), json!(index));
    error.with_data(Value::Object(data))
}

struct Planned {
    path: PathBuf,
    relative: String,
    name: String,
    change: Change,
    line: usize,
    changed: bool,
    replaced: usize,
}

enum Change {
    Create(String),
    Replace(Vec<(Range<usize>, String)>),
}

fn plan(session: &mut Session, edit: FileEdit) -> Result<Planned, CommandError> {
    let FileEdit { path, old_text, new_text, expected_count, regex, mut edits, text } = edit;
    match (old_text, new_text) {
        (Some(old_text), Some(new_text)) => edits.insert(0, TextEdit { old_text, new_text, expected_count, regex }),
        (None, None) => {}
        _ => return Err(CommandError::new(t!("workbench-replace-pair"))),
    }
    if text.is_some() == !edits.is_empty() {
        return Err(CommandError::new(t!("workbench-replace-nothing")));
    }
    let workbench = session.state_mut::<Workbench>();
    let path = workbench.resolve(&path);
    crate::trust::guard_agent_data_folder(session, std::slice::from_ref(&path))?;
    let workbench = session.state_mut::<Workbench>();
    let relative = crate::language::project_relative(&path, workbench.project());
    let open = workbench.documents.iter().any(|document| document.path == path);
    if let Some(text) = &text
        && !open
        && !path.exists()
    {
        let project = workbench.project.as_deref();
        if !project.is_some_and(|project| crate::inside_project(project, &path)) {
            return Err(CommandError::new(t!("workbench-error-outside-project", path = path.display().to_string())));
        }
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        let change = Change::Create(text.clone());
        return Ok(Planned { path, relative, name, change, line: 1, changed: true, replaced: 1 });
    }
    if !open {
        crate::open_document(session, &path)?;
    }
    let workbench = session.state_mut::<Workbench>();
    let document = workbench
        .documents
        .iter()
        .find(|document| document.path == path)
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let name = document.name();
    if let Some(reason) = document.read_only() {
        return Err(CommandError::new(t!("workbench-replace-read-only", file = name.as_str(), reason = reason)));
    }
    let current = document.text();
    let separator = document.line_separator();
    for edit in &mut edits {
        if separator != zen_text::LineSeparator::Lf
            && !edit.regex
            && edit.old_text.contains('\n')
            && !edit.old_text.contains('\r')
            && !current.contains(edit.old_text.as_str())
        {
            edit.old_text = zen_text::line_separator::convert(&edit.old_text, separator).into_owned();
        }
        if !edit.regex {
            edit.new_text = document.with_line_separator(std::mem::take(&mut edit.new_text));
        }
    }
    let text = text.map(|text| document.with_line_separator(text));
    let places = match text {
        Some(text) => vec![(0..current.len(), text)],
        None => places(current, &edits, &name)?,
    };
    let first = places.first().map_or(0, |(range, _)| range.start);
    let changed = places.iter().any(|(range, new_text)| current[range.clone()] != **new_text);
    let first = if edits.is_empty() {
        let new_text = places.first().map_or("", |(_, text)| text.as_str());
        current.bytes().zip(new_text.bytes()).take_while(|(a, b)| a == b).count()
    } else {
        first
    };
    let line = current[..first.min(current.len())].matches('\n').count() + 1;
    let replaced = places.len();
    let change = Change::Replace(
        places
            .into_iter()
            .map(|(range, new_text)| (char_offset(current, range.start)..char_offset(current, range.end), new_text))
            .collect(),
    );
    Ok(Planned { path, relative, name, change, line, changed, replaced })
}

fn apply(session: &mut Session, planned: &Planned, saving: Saving) -> Result<FileResult, CommandError> {
    let mut created = false;
    match &planned.change {
        Change::Create(text) => {
            let create_error = |path: &Path, error: std::io::Error| {
                CommandError::new(t!(
                    "workbench-error-create",
                    path = path.display().to_string(),
                    error = error.to_string()
                ))
            };
            if let Some(parent) = planned.path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| create_error(parent, error))?;
            }
            let bytes = crate::file_encodings::new_file_bytes(session, &planned.path, text);
            zen_vfs::replace_unchanged(&planned.path, None, &bytes)
                .map_err(|error| create_error(&planned.path, error.source))?;
            session.state_mut::<Workbench>().revision += 1;
            crate::open_document(session, &planned.path)?;
            created = true;
        }
        Change::Replace(replacements) if planned.changed => {
            let workbench = session.state_mut::<Workbench>();
            let document = workbench
                .documents
                .iter_mut()
                .find(|document| document.path == planned.path)
                .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
            if !document.replace_ranges(replacements.clone()) {
                return Err(CommandError::new(t!("workbench-replace-refused", file = planned.name.as_str())));
            }
        }
        Change::Replace(_) => {}
    }
    let saved = saving != Saving::No && !created && save_with(session, &planned.path, &planned.name, saving)?;
    Ok(FileResult {
        path: planned.relative.clone(),
        changed: planned.changed,
        line: planned.line,
        replaced: (planned.replaced > 1).then_some(planned.replaced),
        created,
        saved,
        new_problems: None,
        unchecked: None,
    })
}

fn save_changed(
    session: &mut Session,
    file: &Planned,
    result: &FileResult,
    saving: Saving,
) -> Result<bool, CommandError> {
    Ok((result.changed || result.created) && save_with(session, &file.path, &file.name, saving)?)
}

fn refuse_saving(session: &mut Session, file: &Planned) -> Result<(), CommandError> {
    let workbench = session.state_mut::<Workbench>();
    let Some(document) = workbench.documents.iter_mut().find(|document| document.path == file.path) else {
        return Ok(());
    };
    if let Some(reason) = document.read_only() {
        let reason = reason.to_owned();
        return Err(CommandError::new(t!("workbench-replace-read-only", file = file.name.as_str(), reason = reason)));
    }
    document
        .check_disk()
        .map(|_| ())
        .map_err(|error| CommandError::new(crate::closing::save_failed(&file.name, &error)))
}

struct Before {
    text: String,
    saved: String,
}

fn texts_of(session: &Session, planned: &[Planned]) -> Vec<Option<Before>> {
    let Some(workbench) = session.state::<Workbench>() else { return Vec::new() };
    let before = |file: &Planned| {
        let document = workbench.documents.iter().find(|document| document.path == file.path)?;
        Some(Before { text: document.text().to_owned(), saved: document.saved.to_string() })
    };
    planned.iter().map(before).collect()
}

fn take_back(
    session: &mut Session,
    planned: &[Planned],
    before: Vec<Option<Before>>,
    done: &[bool],
) -> Option<CommandError> {
    let mut left = None;
    let workbench = session.state_mut::<Workbench>();
    for ((file, before), done) in planned.iter().zip(before).zip(done.iter().copied().chain(std::iter::repeat(false))) {
        let Some(before) = before else { continue };
        let Some(document) = workbench.documents.iter_mut().find(|document| document.path == file.path) else {
            continue;
        };
        if done {
            set_text(document, &before.saved);
            if let Err(error) = document.save() {
                left = Some(CommandError::new(crate::closing::save_failed(&file.name, &error)));
            }
        }
        set_text(document, &before.text);
    }
    left
}

fn set_text(document: &mut crate::Document, text: &str) {
    if document.text() != text {
        let length = document.text().chars().count();
        document.replace_ranges(vec![(0..length, text.to_owned())]);
    }
}

fn undo(session: &mut Session, made: &[Planned]) {
    for planned in made.iter().rev() {
        match planned.change {
            Change::Create(_) => {
                let workbench = session.state_mut::<Workbench>();
                workbench.documents.retain(|document| document.path != planned.path);
                workbench.groups.retain(|path| path != planned.path);
                workbench.sync_groups();
                let _ = std::fs::remove_file(&planned.path);
            }
            Change::Replace(_) if planned.changed => {
                let workbench = session.state_mut::<Workbench>();
                if let Some(document) = workbench.documents.iter_mut().find(|document| document.path == planned.path) {
                    document.undo();
                }
            }
            Change::Replace(_) => {}
        }
    }
}

fn places(text: &str, edits: &[TextEdit], file: &str) -> Result<Vec<(Range<usize>, String)>, CommandError> {
    let mut places = Vec::new();
    for (index, edit) in edits.iter().enumerate() {
        if edit.old_text.is_empty() {
            return Err(CommandError::new(t!("workbench-replace-empty")));
        }
        let expected = edit.expected_count.unwrap_or(1);
        let data = |count: usize| {
            let mut data = json!({ "count": count, "expected": expected });
            if edits.len() > 1 {
                data["edit"] = json!(index);
            }
            data
        };
        let found: Vec<(Range<usize>, String)> = if edit.regex {
            let query = zen_text::search::Query {
                text: edit.old_text.clone(),
                regex: true,
                match_case: true,
                ..zen_text::search::Query::default()
            };
            let found = zen_text::search::replacements(text, &query, &edit.new_text).map_err(|error| {
                CommandError::with_code(INVALID_REGEX, t!("workbench-replace-bad-regex", error = error))
                    .with_data(data(0))
            })?;
            let bytes: Vec<usize> = text.char_indices().map(|(at, _)| at).chain(std::iter::once(text.len())).collect();
            found.into_iter().map(|(range, new_text)| (bytes[range.start]..bytes[range.end], new_text)).collect()
        } else {
            text.match_indices(edit.old_text.as_str())
                .map(|(at, _)| (at..at + edit.old_text.len(), edit.new_text.clone()))
                .collect()
        };
        if found.is_empty() {
            return Err(
                CommandError::with_code(NOT_FOUND, t!("workbench-replace-not-found", file = file)).with_data(data(0))
            );
        }
        if found.len() != expected {
            let message = t!("workbench-replace-not-unique", file = file, count = found.len(), expected = expected);
            return Err(CommandError::with_code(NOT_UNIQUE, message).with_data(data(found.len())));
        }
        places.extend(found);
    }
    places.sort_by_key(|(range, _)| (range.start, range.end));
    if places.windows(2).any(|pair| pair[0].0.end > pair[1].0.start) {
        return Err(CommandError::with_code(OVERLAP, t!("workbench-replace-overlap", file = file)));
    }
    Ok(places)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Saving {
    No,
    Plain,
    WithActions,
}

pub(crate) fn saving(session: &Session, asked: Option<bool>) -> Saving {
    match asked {
        Some(true) => Saving::WithActions,
        Some(false) => Saving::No,
        None if zen_core::macros::by_agent(session) => Saving::Plain,
        None => Saving::No,
    }
}

pub(crate) fn save(session: &mut Session, path: &Path, name: &str) -> Result<bool, CommandError> {
    save_with(session, path, name, Saving::WithActions)
}

pub(crate) fn save_with(session: &mut Session, path: &Path, name: &str, saving: Saving) -> Result<bool, CommandError> {
    if saving == Saving::WithActions {
        let _ = crate::language::format::before_save(session, path);
    }
    let workbench = session.state_mut::<Workbench>();
    let Some(document) = workbench.documents.iter_mut().find(|document| document.path == path) else {
        return Ok(false);
    };
    if !document.dirty {
        return Ok(false);
    }
    document.save().map_err(|error| CommandError::new(crate::closing::save_failed(name, &error)))?;
    crate::local_history::record_saves(session);
    Ok(true)
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ReplaceText)?;
    registry.register(WriteText)?;
    registry.register(BatchEdit)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use zen_core::Registry;

    use super::*;

    const MAIN: &str = "package main\n\nfunc a() int { return 1 }\n\nfunc b() int { return 1 }\n\nfunc main() {}\n";

    struct Fixture {
        root: PathBuf,
        registry: Registry,
        session: Session,
        _dir: tempfile::TempDir,
    }

    impl Fixture {
        fn new() -> Self {
            zen_i18n::register(crate::LOCALES);
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().canonicalize().unwrap();
            std::fs::write(root.join("main.go"), MAIN).unwrap();
            let mut registry = Registry::new();
            crate::register(&mut registry).unwrap();
            let mut session = Session::new();
            registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
            Fixture { _dir: dir, root, registry, session }
        }

        fn replace(&mut self, params: Value) -> Result<Value, zen_core::DispatchError> {
            self.registry.dispatch(&mut self.session, "editor.text.replace", params)
        }

        fn run(&mut self, command: &str, params: Value) -> Result<Value, zen_core::DispatchError> {
            self.registry.dispatch(&mut self.session, command, params)
        }

        fn text_of(&self, name: &str) -> String {
            let workbench = self.session.state::<Workbench>().unwrap();
            workbench.documents().iter().find(|d| d.path() == self.root.join(name)).unwrap().text().to_owned()
        }

        fn text(&self) -> String {
            self.text_of("main.go")
        }

        fn failure(&mut self, params: Value) -> CommandError {
            failure(self.replace(params))
        }
    }

    fn failure(result: Result<Value, zen_core::DispatchError>) -> CommandError {
        match result.unwrap_err() {
            zen_core::DispatchError::Failed { error, .. } => error,
            other => panic!("{other}"),
        }
    }

    #[test]
    fn an_agents_edit_is_on_disk_when_the_call_answers_unless_it_asks_not() {
        let mut f = Fixture::new();
        let file = f.root.join("main.go");
        let (registry, session) = (&f.registry, &mut f.session);
        let agent = |session: &mut Session, command: &str, params: Value| {
            zen_core::macros::as_agent(session, |session| registry.dispatch(session, command, params)).unwrap()
        };
        let replaced = agent(
            session,
            "editor.text.replace",
            json!({ "path": "main.go", "oldText": "func a()", "newText": "func one()" }),
        );
        assert_eq!(replaced["saved"], true, "{replaced}");
        assert!(std::fs::read_to_string(&file).unwrap().contains("func one()"), "the shell reads it at once");
        let kept = json!({ "path": "main.go", "oldText": "func b()", "newText": "func two()", "save": false });
        assert_ne!(agent(session, "editor.text.replace", kept)["saved"], true);
        assert!(std::fs::read_to_string(&file).unwrap().contains("func b()"), "save: false keeps it in the editor");
        let batch = json!({ "files": [{ "path": "main.go", "oldText": "func main()", "newText": "func main2()" }] });
        agent(session, "editor.text.batch", batch);
        assert!(
            std::fs::read_to_string(&file).unwrap().contains("func two()")
                && std::fs::read_to_string(&file).unwrap().contains("main2")
        );
        agent(session, "editor.text.write", json!({ "path": "new.go", "text": "package main\n" }));
        assert_eq!(std::fs::read_to_string(f.root.join("new.go")).unwrap(), "package main\n");
        let unsaved = json!({ "path": "main.go", "oldText": "func two()", "newText": "func dos()", "save": false });
        agent(session, "editor.text.replace", unsaved);
        std::fs::write(&file, "package main\n\nfunc elsewhere() {}\n").unwrap();
        let clash = json!({ "path": "main.go", "oldText": "func dos()", "newText": "func tres()" });
        let refused =
            zen_core::macros::as_agent(session, |session| registry.dispatch(session, "editor.text.replace", clash));
        assert!(refused.is_err(), "a file changed on disk under unsaved edits is told, not overwritten");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "package main\n\nfunc elsewhere() {}\n");
        let person = json!({ "path": "main.go", "oldText": "func tres()", "newText": "func three()" });
        assert_ne!(f.replace(person).unwrap()["saved"], true, "a person's command still leaves saving to them");
    }

    #[test]
    fn an_agents_default_save_leaves_the_on_save_actions_out_and_save_true_runs_them() {
        let mut f = Fixture::new();
        let file = f.root.join("main.go");
        let (registry, session) = (&f.registry, &mut f.session);
        let agent = |session: &mut Session, params: Value| {
            zen_core::macros::as_agent(session, |session| registry.dispatch(session, "editor.text.replace", params))
                .unwrap()
        };
        agent(
            session,
            json!({ "path": "main.go", "oldText": "func a() int { return 1 }", "newText": "func a() int { return 1 }   " }),
        );
        assert!(
            std::fs::read_to_string(&file).unwrap().contains("func a() int { return 1 }   \n"),
            "the text is on disk exactly as the agent wrote it"
        );
        let edit =
            json!({ "path": "main.go", "oldText": "func main() {}", "newText": "func main() {}   ", "save": true });
        agent(session, edit);
        assert!(
            std::fs::read_to_string(&file).unwrap().contains("func main() {}\n"),
            "save: true runs the on-save actions, trailing spaces removed: {:?}",
            std::fs::read_to_string(&file)
        );
    }

    #[test]
    fn one_call_replaces_saves_and_answers_in_a_few_bytes() {
        let mut fixture = Fixture::new();
        let answer = fixture
            .replace(json!({ "path": "main.go", "oldText": "func a() int { return 1 }", "newText": "func a() int { return 2 }", "save": true }))
            .unwrap();
        assert_eq!(answer, json!({ "changed": true, "line": 3, "saved": true }));
        assert!(answer.to_string().len() < 300);
        let on_disk = std::fs::read_to_string(fixture.root.join("main.go")).unwrap();
        assert_eq!(on_disk, MAIN.replace("func a() int { return 1 }", "func a() int { return 2 }"));
        fixture.registry.dispatch(&mut fixture.session, "editor.edit.undo", json!({})).unwrap();
        assert_eq!(fixture.text(), MAIN);
    }

    #[test]
    fn the_editor_text_with_unsaved_changes_is_what_is_searched() {
        let mut fixture = Fixture::new();
        let path = fixture.root.join("main.go");
        fixture
            .replace(json!({ "path": path, "oldText": "func main() {}", "newText": "func main() { a() }" }))
            .unwrap();
        let answer = fixture.replace(json!({ "path": path, "oldText": "{ a() }", "newText": "{ b() }" })).unwrap();
        assert_eq!(answer, json!({ "changed": true, "line": 7 }));
        assert!(fixture.text().contains("func main() { b() }"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), MAIN, "not saved without `save`");
    }

    #[test]
    fn text_that_is_missing_or_not_unique_is_an_error_with_its_count() {
        let mut fixture = Fixture::new();
        let error = fixture.failure(json!({ "path": "main.go", "oldText": "return 1", "newText": "return 3" }));
        assert_eq!(error.code.as_deref(), Some(NOT_UNIQUE));
        assert_eq!(error.data.as_ref().unwrap()["count"], 2);
        let error = fixture.failure(json!({ "path": "main.go", "oldText": "return 9", "newText": "return 3" }));
        assert_eq!(error.code.as_deref(), Some(NOT_FOUND));
        assert_eq!(fixture.text(), MAIN, "nothing changed");
        let answer = fixture
            .replace(json!({ "path": "main.go", "oldText": "return 1", "newText": "return 3", "expectedCount": 2 }))
            .unwrap();
        assert_eq!(answer, json!({ "changed": true, "line": 3, "replaced": 2 }));
        assert_eq!(fixture.text(), MAIN.replace("return 1", "return 3"));
    }

    #[test]
    fn a_batch_is_atomic_and_one_undo_step() {
        let mut fixture = Fixture::new();
        let edits = json!([
            { "oldText": "func a()", "newText": "func first()" },
            { "oldText": "func b()", "newText": "func second()" },
            { "oldText": "func missing()", "newText": "x" },
        ]);
        let error = fixture.failure(json!({ "path": "main.go", "edits": edits }));
        assert_eq!((error.code.as_deref(), &error.data.as_ref().unwrap()["edit"]), (Some(NOT_FOUND), &json!(2)));
        assert_eq!(fixture.text(), MAIN, "no edit of a failed batch is made");
        let edits = json!([
            { "oldText": "func b()", "newText": "func second()" },
            { "oldText": "func a()", "newText": "func first()" },
        ]);
        let answer = fixture.replace(json!({ "path": "main.go", "edits": edits })).unwrap();
        assert_eq!(answer, json!({ "changed": true, "line": 3, "replaced": 2 }));
        assert!(fixture.text().contains("func first()") && fixture.text().contains("func second()"));
        fixture.registry.dispatch(&mut fixture.session, "editor.edit.undo", json!({})).unwrap();
        assert_eq!(fixture.text(), MAIN, "one undo step");
        let overlapping = json!([{ "oldText": "func a()", "newText": "x" }, { "oldText": "a() int", "newText": "y" }]);
        assert_eq!(fixture.failure(json!({ "path": "main.go", "edits": overlapping })).code.as_deref(), Some(OVERLAP));
    }

    #[test]
    fn a_regular_expression_replaces_by_pattern_with_its_groups() {
        let mut fixture = Fixture::new();
        let answer = fixture
            .replace(json!({
                "path": "main.go", "oldText": r"^func (\w+)\(\) int \{ return (\d) \}$",
                "newText": "func ${1}() int { return ${2}0 }", "regex": true, "expectedCount": 2
            }))
            .unwrap();
        assert_eq!(answer, json!({ "changed": true, "line": 3, "replaced": 2 }));
        assert_eq!(fixture.text(), MAIN.replace("return 1", "return 10"));
        let error = fixture.failure(json!({ "path": "main.go", "oldText": "(", "newText": "x", "regex": true }));
        assert_eq!(error.code.as_deref(), Some(INVALID_REGEX));
        let edits = json!([{ "oldText": r"^package \w+$", "newText": "package app", "regex": true }]);
        fixture.replace(json!({ "path": "main.go", "edits": edits })).unwrap();
        assert!(fixture.text().starts_with("package app\n"), "{}", fixture.text());
    }

    #[test]
    fn a_whole_text_is_written_and_a_new_file_created() {
        let mut fixture = Fixture::new();
        let created =
            fixture.run("editor.text.write", json!({ "path": "pkg/util/util.go", "text": "package util\n" })).unwrap();
        assert_eq!(created, json!({ "changed": true, "line": 1, "created": true }));
        assert_eq!(std::fs::read_to_string(fixture.root.join("pkg/util/util.go")).unwrap(), "package util\n");
        assert_eq!(fixture.text_of("pkg/util/util.go"), "package util\n", "opened");

        let text = MAIN.replace("func main() {}", "func main() { a() }");
        let written =
            fixture.run("editor.text.write", json!({ "path": "main.go", "text": text, "save": true })).unwrap();
        assert_eq!(written, json!({ "changed": true, "line": 7, "saved": true }));
        assert_eq!(std::fs::read_to_string(fixture.root.join("main.go")).unwrap(), text);
        fixture.run("editor.edit.undo", json!({})).unwrap();
        assert_eq!(fixture.text(), MAIN, "one undo step");
        let outside = fixture.run("editor.text.write", json!({ "path": "/nonexistent-zen/x.go", "text": "x" }));
        assert!(outside.is_err(), "no file is created outside the project");
    }

    #[test]
    fn saving_for_an_agent_never_writes_over_a_change_made_outside() {
        let mut fixture = Fixture::new();
        let path = fixture.root.join("main.go");
        fixture.run("file.document.open", json!({ "path": path })).unwrap();
        std::fs::write(&path, "package theirs\n").unwrap();
        let params =
            json!({ "path": path, "oldText": "func main() {}", "newText": "func main() { b() }", "save": true });
        fixture.replace(params).unwrap_err();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "package theirs\n");
    }

    #[test]
    fn a_batch_that_cannot_save_a_file_changed_outside_saves_none() {
        let mut fixture = Fixture::new();
        let (main, other) = (fixture.root.join("main.go"), fixture.root.join("other.go"));
        std::fs::write(&other, "package other\n").unwrap();
        fixture.run("file.document.open", json!({ "path": main })).unwrap();
        fixture.run("file.document.open", json!({ "path": other })).unwrap();
        std::fs::write(&other, "package theirs\n").unwrap();
        let params = json!({ "save": true, "files": [
            { "path": main, "oldText": "func main() {}", "newText": "func main() { a() }" },
            { "path": other, "oldText": "package other", "newText": "package mine" },
        ] });
        let refused = fixture.run("editor.text.batch", params).unwrap_err().to_string();
        assert!(refused.contains("other.go"), "{refused}");
        assert_eq!(std::fs::read_to_string(&main).unwrap(), MAIN, "nothing is written");
        assert_eq!(std::fs::read_to_string(&other).unwrap(), "package theirs\n");
        assert_eq!(fixture.text(), MAIN, "nothing changed in the editor either");
    }

    #[cfg(unix)]
    #[test]
    fn a_batch_that_fails_to_write_a_file_takes_back_the_ones_it_wrote() {
        use std::os::unix::fs::PermissionsExt;
        let mut fixture = Fixture::new();
        let main = fixture.root.join("main.go");
        let folder = fixture.root.join("locked");
        let other = folder.join("other.go");
        std::fs::create_dir(&folder).unwrap();
        std::fs::write(&other, "package other\n").unwrap();
        fixture.run("file.document.open", json!({ "path": main })).unwrap();
        fixture.run("file.document.open", json!({ "path": other })).unwrap();
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
        let params = json!({ "save": true, "files": [
            { "path": main, "oldText": "func main() {}", "newText": "func main() { a() }" },
            { "path": other, "oldText": "package other", "newText": "package mine" },
        ] });
        let failed = fixture.run("editor.text.batch", params);
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(failed.unwrap_err().to_string().contains("other.go"));
        assert_eq!(std::fs::read_to_string(&main).unwrap(), MAIN, "written back");
        assert_eq!(fixture.text(), MAIN);
        let workbench = fixture.session.state::<Workbench>().unwrap();
        assert!(workbench.documents().iter().all(|document| !document.is_dirty()), "nothing is left half done");
    }

    #[test]
    fn a_batch_of_files_changes_every_file_or_none() {
        let mut fixture = Fixture::new();
        std::fs::write(fixture.root.join("lib.go"), "package main\n\nfunc lib() {}\n").unwrap();
        let failing = json!({ "files": [
            { "path": "main.go", "oldText": "func main() {}", "newText": "func main() { lib() }" },
            { "path": "new.go", "text": "package main\n" },
            { "path": "lib.go", "oldText": "func missing()", "newText": "x" },
        ] });
        let error = failure(fixture.run("editor.text.batch", failing));
        assert_eq!((error.code.as_deref(), &error.data.as_ref().unwrap()["file"]), (Some(NOT_FOUND), &json!(2)));
        assert_eq!(fixture.text(), MAIN, "nothing changed");
        assert!(!fixture.root.join("new.go").exists(), "nothing created");

        let batch = json!({ "save": true, "files": [
            { "path": "main.go", "oldText": "func main() {}", "newText": "func main() { lib() }" },
            { "path": "lib.go", "edits": [{ "oldText": r"func (\w+)\(\)", "newText": "func ${1}2()", "regex": true }] },
            { "path": "new.go", "text": "package main\n" },
        ] });
        let answer = fixture.run("editor.text.batch", batch).unwrap();
        assert_eq!(
            answer,
            json!({ "files": [
                { "path": "main.go", "changed": true, "line": 7, "saved": true },
                { "path": "lib.go", "changed": true, "line": 3, "saved": true },
                { "path": "new.go", "changed": true, "line": 1, "created": true },
            ] })
        );
        assert_eq!(std::fs::read_to_string(fixture.root.join("lib.go")).unwrap(), "package main\n\nfunc lib2() {}\n");
        assert!(std::fs::read_to_string(fixture.root.join("main.go")).unwrap().contains("{ lib() }"));
        let twice = json!({ "files": [
            { "path": "lib.go", "oldText": "lib2", "newText": "x" },
            { "path": "lib.go", "oldText": "package", "newText": "x" },
        ] });
        assert_eq!(failure(fixture.run("editor.text.batch", twice)).data.unwrap()["file"], 1, "one entry per file");
    }
}
