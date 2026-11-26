use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_i18n::t;

use super::MAX_WAIT;
use super::places::Place;
use super::refactor::{RenameFile, RenameOperation};
use super::safe_delete::{self, SafeDeleteParams};
use crate::Workbench;

pub type Values = Map<String, Value>;

pub trait RefactoringProvider: Send + Sync {
    fn id(&self) -> &str;

    fn title(&self) -> String;

    fn form(&self, session: &mut Session, context: &Value, wait: Duration) -> Result<RefactoringForm, CommandError>;

    fn plan(
        &self,
        session: &mut Session,
        context: &Value,
        values: &Values,
        wait: Duration,
    ) -> Result<RefactoringPlan, CommandError>;

    fn apply(
        &self,
        session: &mut Session,
        context: &Value,
        values: &Values,
        wait: Duration,
    ) -> Result<RefactoringApplied, CommandError>;

    fn cancel(&self, session: &mut Session);

    fn action(&self) -> Option<&str> {
        None
    }

    fn accepts(&self, _session: &mut Session, _context: &Value) -> bool {
        false
    }
}

#[derive(Default, Clone)]
pub struct RefactoringProviders {
    added: Vec<Arc<dyn RefactoringProvider>>,
}

impl RefactoringProviders {
    pub fn add(&mut self, provider: Arc<dyn RefactoringProvider>) {
        self.added.retain(|known| known.id() != provider.id());
        self.added.push(provider);
    }

    pub fn all(&self) -> Vec<Arc<dyn RefactoringProvider>> {
        let mut all: Vec<Arc<dyn RefactoringProvider>> = vec![Arc::new(SafeDelete), Arc::new(Rename)];
        all.extend(crate::java::refactoring::providers());
        for provider in &self.added {
            all.retain(|known| known.id() != provider.id());
            all.push(provider.clone());
        }
        all
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn RefactoringProvider>> {
        self.all().into_iter().find(|provider| provider.id() == id)
    }

    pub fn for_action(
        &self,
        session: &mut Session,
        action: &str,
        context: &Value,
    ) -> Option<Arc<dyn RefactoringProvider>> {
        self.all()
            .into_iter()
            .rev()
            .find(|provider| provider.action() == Some(action) && provider.accepts(session, context))
    }
}

/// What a field of the refactoring dialog holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum FieldKind {
    /// A line of text.
    Text,
    /// A check box; its value is true or false.
    Checkbox,
    /// One of `options`; its value is the option's `value`.
    Choice,
    /// Rows of `columns`; its value is a list of objects with a value by column id (a string, or a boolean in a
    /// `checkbox` column), and rows can be added, removed and moved unless the field is `fixed`.
    Table,
}

/// How a column of a `table` field shows its cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ColumnKind {
    /// A line of text that can be edited.
    #[default]
    Text,
    /// A check box; the cell's value is true or false, and a row without a boolean there shows none.
    Checkbox,
    /// Text that cannot be edited.
    Label,
}

impl ColumnKind {
    fn is_text(&self) -> bool {
        *self == Self::Text
    }
}

/// A column of a `table` field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TableColumn {
    /// The key of the column's value in a row.
    pub id: String,
    /// The column's header.
    pub label: String,
    /// How its cells are shown: `text` by default.
    #[serde(default, skip_serializing_if = "ColumnKind::is_text")]
    pub kind: ColumnKind,
}

impl TableColumn {
    pub fn new(id: &str, label: String, kind: ColumnKind) -> Self {
        Self { id: id.to_owned(), label, kind }
    }
}

/// A choice of a `choice` field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FieldOption {
    /// The value the field takes.
    pub value: String,
    /// What the dialog shows.
    pub label: String,
}

/// A field the refactoring asks for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefactoringField {
    /// The key of its value in `values`.
    pub id: String,
    /// The label shown with it.
    pub label: String,
    /// What it holds.
    pub kind: FieldKind,
    /// Its value to start from: a string, or a boolean for a check box.
    pub value: Value,
    /// The choices of a `choice` field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<FieldOption>,
    /// The columns of a `table` field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<TableColumn>,
    /// The row a `table` field adds: its values to start from.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub new_row: Value,
    /// Whether a `table` field's rows are fixed: none is added, removed or moved.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fixed: bool,
}

/// What `refactor.dialog.form` gives: the dialog of a refactoring.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RefactoringForm {
    /// Whether the refactoring is still finding what to ask; call again to wait more.
    pub pending: bool,
    /// The dialog's title.
    pub title: String,
    /// A sentence above the fields.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// The fields, in order.
    pub fields: Vec<RefactoringField>,
    /// Whether the refactoring is made at once, without the dialog, when it finds no conflicts.
    pub immediate: bool,
    /// The title of the list of conflicts.
    pub conflicts_title: String,
    /// The button that refactors in spite of the conflicts.
    pub anyway_label: String,
    /// The button that shows the conflicts in the Find tool window.
    pub view_label: String,
    /// Why the refactoring cannot be made here, in words for the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Something that makes the refactoring unsafe, found before it is made.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefactoringConflict {
    /// What is wrong, in words for the user.
    pub message: String,
    /// Where, when it is a place in a file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<Place>,
    /// The place's file relative to the project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relative: Option<String>,
}

/// What `refactor.dialog.preview` gives: what the refactoring would change and what stands in its way.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefactoringPlan {
    /// Whether the refactoring has not finished looking; call again to wait more.
    pub pending: bool,
    /// The text changes, by file.
    pub files: Vec<RenameFile>,
    /// The files created, renamed or deleted, in order.
    pub operations: Vec<RenameOperation>,
    /// How many text changes there are in all.
    pub usage_count: usize,
    /// What makes the refactoring unsafe; refactor.dialog.apply needs `force` while there are some.
    pub conflicts: Vec<RefactoringConflict>,
    /// A sentence above the conflicts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflicts_message: Option<String>,
    /// Why the refactoring cannot be made with these values, in words for the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// What `refactor.dialog.apply` changed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefactoringApplied {
    /// The files changed, created, renamed or deleted.
    pub files: Vec<PathBuf>,
    /// How many text changes were made.
    pub edits: usize,
}

/// A refactoring the dialog can run.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RefactoringInfo {
    /// Its id, the `provider` of the refactor.dialog commands.
    pub id: String,
    /// Its name.
    pub title: String,
    /// The window action it answers when it accepts the place (`move`, `changeSignature`, `pullMembersUp`...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

/// The parameters of `refactor.dialog.form`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FormParams {
    /// The refactoring, an id refactor.dialog.list gives (`safeDelete`, `rename`, `javaMove`, `javaPullUp`...).
    pub provider: String,
    /// Where it applies, as the refactoring takes it: `{path, offset}` or `{path, line, column}`
    /// for a declaration (the caret when absent), `{paths}` for files with `safeDelete` and for classes and
    /// packages with `javaMove`.
    #[serde(default)]
    pub context: Value,
    /// How long to wait in milliseconds, at most 10 000; none by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// The parameters of `refactor.dialog.preview` and `refactor.dialog.apply`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PlanParams {
    /// The refactoring, an id refactor.dialog.list gives (`safeDelete`, `rename`, `javaMove`, `javaPullUp`...).
    pub provider: String,
    /// Where it applies, as refactor.dialog.form took it.
    #[serde(default)]
    pub context: Value,
    /// The fields' values by field id; a field left out keeps the value the form gave.
    #[serde(default)]
    pub values: Values,
    /// Whether to refactor in spite of conflicts (apply only); refused with conflicts otherwise.
    #[serde(default)]
    pub force: bool,
    /// How long to wait in milliseconds, at most 10 000; none for preview and 10 000 for apply by default.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// The parameters of `refactor.dialog.cancel`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CancelParams {
    /// The refactoring whose work to forget.
    pub provider: String,
}

pub struct ListRefactorings;

impl Command for ListRefactorings {
    type Params = NoParams;
    type Output = Vec<RefactoringInfo>;
    const ID: &'static str = "refactor.dialog.list";
    const TITLE: &'static str = "Refactorings";
    const DESCRIPTION: &'static str = "Lists the refactorings the shared refactoring dialog runs, from the platform and the \
         language plug-ins, with the ids the other refactor.dialog commands take.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<RefactoringInfo>, CommandError> {
        let providers = session.state_mut::<RefactoringProviders>().all();
        Ok(providers
            .iter()
            .map(|provider| RefactoringInfo {
                id: provider.id().to_owned(),
                title: provider.title(),
                action: provider.action().map(str::to_owned),
            })
            .collect())
    }
}

fn guard_context(session: &Session, context: &Value) -> Result<(), CommandError> {
    let Some(workbench) = session.state::<Workbench>() else { return Ok(()) };
    let named = context
        .get("paths")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(context.get("path"))
        .filter_map(Value::as_str)
        .map(|path| workbench.resolve(Path::new(path)));
    let mut paths: Vec<PathBuf> = named.collect();
    if paths.is_empty() {
        paths.extend(workbench.active_document().map(|document| document.path().to_owned()));
    }
    crate::hidden_files::guard_read(session, &paths)
}

pub struct RefactoringFormCommand;

impl Command for RefactoringFormCommand {
    type Params = FormParams;
    type Output = RefactoringForm;
    const ID: &'static str = "refactor.dialog.form";
    const TITLE: &'static str = "Refactoring Dialog";
    const DESCRIPTION: &'static str = "Gives the dialog of a refactoring at a place: its title, the fields it asks for with their \
         values to start from, and the labels of its conflict list. Call again with waitMs while pending; \
         refactor.dialog.preview shows what it would change.";

    fn run(&self, session: &mut Session, params: FormParams) -> Result<RefactoringForm, CommandError> {
        guard_context(session, &params.context)?;
        let provider = provider(session, &params.provider)?;
        provider.form(session, &params.context, wait(params.wait_ms, Duration::ZERO))
    }
}

pub struct PreviewRefactoring;

impl Command for PreviewRefactoring {
    type Params = PlanParams;
    type Output = RefactoringPlan;
    const ID: &'static str = "refactor.dialog.preview";
    const TITLE: &'static str = "Preview Refactoring";
    const DESCRIPTION: &'static str = "Computes what a refactoring would change with the fields' values (text changes by file \
         and files created, renamed or deleted) and the conflicts found before it is made, changing nothing. Call \
         again with waitMs while pending; refactor.dialog.apply makes it.";

    fn run(&self, session: &mut Session, params: PlanParams) -> Result<RefactoringPlan, CommandError> {
        guard_context(session, &params.context)?;
        let provider = provider(session, &params.provider)?;
        provider.plan(session, &params.context, &params.values, wait(params.wait_ms, Duration::ZERO))
    }
}

pub struct ApplyRefactoring;

impl Command for ApplyRefactoring {
    type Params = PlanParams;
    type Output = RefactoringApplied;
    const ID: &'static str = "refactor.dialog.apply";
    const TITLE: &'static str = "Refactor";
    const DESCRIPTION: &'static str = "Makes a refactoring with the fields' values once its preview is ready (waiting up to \
         waitMs), refusing while it has conflicts unless `force`. Every file changes or none does, and one Undo \
         reverts it.";

    fn run(&self, session: &mut Session, params: PlanParams) -> Result<RefactoringApplied, CommandError> {
        guard_context(session, &params.context)?;
        let provider = provider(session, &params.provider)?;
        let wait = wait(params.wait_ms, MAX_WAIT);
        let plan = provider.plan(session, &params.context, &params.values, wait)?;
        if plan.pending {
            return Err(CommandError::new(zen_lsp::LspError::Timeout.to_string()));
        }
        if let Some(error) = plan.error {
            return Err(CommandError::new(error));
        }
        if !plan.conflicts.is_empty() && !params.force {
            let message = plan
                .conflicts_message
                .unwrap_or_else(|| t!("workbench-refactoring-conflicts", count = plan.conflicts.len()));
            return Err(CommandError::new(t!("workbench-refactoring-refused", conflicts = message)));
        }
        provider.apply(session, &params.context, &params.values, wait)
    }
}

pub struct CancelRefactoring;

impl Command for CancelRefactoring {
    type Params = CancelParams;
    type Output = ();
    const ID: &'static str = "refactor.dialog.cancel";
    const TITLE: &'static str = "Cancel Refactoring";
    const DESCRIPTION: &'static str = "Forgets the work of a refactoring the dialog started, cancelling the language servers' requests that still run.";

    fn run(&self, session: &mut Session, params: CancelParams) -> Result<(), CommandError> {
        provider(session, &params.provider)?.cancel(session);
        Ok(())
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListRefactorings)?;
    registry.register(RefactoringFormCommand)?;
    registry.register(PreviewRefactoring)?;
    registry.register(ApplyRefactoring)?;
    registry.register(CancelRefactoring)?;
    Ok(())
}

fn provider(session: &mut Session, id: &str) -> Result<Arc<dyn RefactoringProvider>, CommandError> {
    session
        .state_mut::<RefactoringProviders>()
        .get(id)
        .ok_or_else(|| CommandError::new(t!("workbench-refactoring-unknown", id = id)))
}

fn wait(wait_ms: Option<u64>, default: Duration) -> Duration {
    wait_ms.map_or(default, |wait_ms| Duration::from_millis(wait_ms).min(MAX_WAIT))
}

fn wait_ms(wait: Duration) -> u64 {
    wait.as_millis() as u64
}

pub(crate) fn context<T: serde::de::DeserializeOwned + Default>(context: &Value) -> Result<T, CommandError> {
    if context.is_null() {
        return Ok(T::default());
    }
    serde_json::from_value(context.clone()).map_err(|error| CommandError::new(error.to_string()))
}

struct SafeDelete;

impl RefactoringProvider for SafeDelete {
    fn id(&self) -> &str {
        "safeDelete"
    }

    fn title(&self) -> String {
        t!("workbench-safe-delete-title")
    }

    fn form(&self, _: &mut Session, _: &Value, _: Duration) -> Result<RefactoringForm, CommandError> {
        Ok(RefactoringForm {
            title: self.title(),
            immediate: true,
            conflicts_title: t!("workbench-safe-delete-usages-detected"),
            anyway_label: t!("workbench-safe-delete-anyway"),
            view_label: t!("workbench-safe-delete-view-usages"),
            ..RefactoringForm::default()
        })
    }

    fn plan(
        &self,
        session: &mut Session,
        context: &Value,
        _: &Values,
        wait: Duration,
    ) -> Result<RefactoringPlan, CommandError> {
        let params: SafeDeleteParams = self::context(context)?;
        let found = safe_delete::find(session, &params, wait)?;
        if found.pending || found.error.is_some() {
            return Ok(RefactoringPlan { pending: found.pending, error: found.error, ..RefactoringPlan::default() });
        }
        let conflicts = found
            .usages
            .iter()
            .flat_map(|file| {
                file.usages.iter().map(|place| RefactoringConflict {
                    message: t!("workbench-safe-delete-usage", file = file.relative.as_str(), line = place.line),
                    place: Some(place.clone()),
                    relative: Some(file.relative.clone()),
                })
            })
            .collect();
        let files = params.paths.as_ref().is_some_and(|paths| !paths.is_empty());
        let message = (found.total > 0).then(|| {
            if files {
                t!("workbench-safe-delete-file-conflicts", name = found.name.as_str(), count = found.total)
            } else {
                t!("workbench-safe-delete-conflicts", name = found.name.as_str(), count = found.total)
            }
        });
        Ok(RefactoringPlan {
            pending: false,
            usage_count: found.deletes.iter().map(|file| file.changes.len()).sum(),
            files: found.deletes,
            operations: found.operations,
            conflicts,
            conflicts_message: message,
            error: None,
        })
    }

    fn apply(
        &self,
        session: &mut Session,
        context: &Value,
        _: &Values,
        wait: Duration,
    ) -> Result<RefactoringApplied, CommandError> {
        let mut params: SafeDeleteParams = self::context(context)?;
        params.wait_ms = Some(wait_ms(wait));
        let deleted = safe_delete::apply(session, &params, true)?;
        let edits = usize::from(params.paths.as_ref().is_none_or(Vec::is_empty));
        Ok(RefactoringApplied { files: deleted.files, edits })
    }

    fn cancel(&self, session: &mut Session) {
        safe_delete::cancel(session);
    }
}

struct Rename;

impl Rename {
    fn name(values: &Values) -> String {
        values.get("newName").and_then(Value::as_str).unwrap_or_default().trim().to_owned()
    }
}

impl RefactoringProvider for Rename {
    fn id(&self) -> &str {
        "rename"
    }

    fn title(&self) -> String {
        t!("workbench-rename-title")
    }

    fn form(&self, session: &mut Session, context: &Value, wait: Duration) -> Result<RefactoringForm, CommandError> {
        let mut params: super::LookupParams = self::context(context)?;
        params.wait_ms = Some(wait_ms(wait));
        let target = super::refactor::prepare_at(session, &params)?;
        if target.pending || target.error.is_some() {
            return Ok(RefactoringForm { pending: target.pending, error: target.error, ..RefactoringForm::default() });
        }
        Ok(RefactoringForm {
            title: self.title(),
            fields: vec![RefactoringField {
                id: "newName".to_owned(),
                label: t!("workbench-rename-to", name = target.placeholder.as_str()),
                kind: FieldKind::Text,
                value: json!(target.placeholder),
                options: Vec::new(),
                columns: Vec::new(),
                new_row: Value::Null,
                fixed: false,
            }],
            conflicts_title: t!("workbench-refactoring-problems"),
            anyway_label: t!("workbench-refactoring-continue"),
            view_label: t!("workbench-refactoring-view-conflicts"),
            ..RefactoringForm::default()
        })
    }

    fn plan(
        &self,
        session: &mut Session,
        context: &Value,
        values: &Values,
        wait: Duration,
    ) -> Result<RefactoringPlan, CommandError> {
        let at: super::LookupParams = self::context(context)?;
        let params = super::refactor::RenamePreviewParams {
            path: at.path,
            offset: at.offset,
            line: at.line,
            column: at.column,
            new_name: Self::name(values),
            wait_ms: Some(wait_ms(wait)),
        };
        let preview = super::refactor::preview_at(session, &params)?;
        Ok(RefactoringPlan {
            pending: preview.pending,
            files: preview.files,
            operations: preview.operations,
            usage_count: preview.usage_count,
            conflicts: Vec::new(),
            conflicts_message: None,
            error: preview.error,
        })
    }

    fn apply(
        &self,
        session: &mut Session,
        context: &Value,
        values: &Values,
        wait: Duration,
    ) -> Result<RefactoringApplied, CommandError> {
        let at: super::LookupParams = self::context(context)?;
        let params = super::refactor::RenameApplyParams {
            new_name: Some(Self::name(values)),
            path: at.path,
            offset: at.offset,
            line: at.line,
            column: at.column,
            save: Some(false),
            wait_ms: Some(wait_ms(wait)),
        };
        let applied = super::refactor::apply_rename(session, &params)?;
        Ok(RefactoringApplied { files: applied.files, edits: applied.edits })
    }

    fn cancel(&self, session: &mut Session) {
        super::refactor::cancel_rename(session);
    }
}

#[cfg(test)]
mod tests;
