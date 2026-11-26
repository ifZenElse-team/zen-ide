use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use zen_core::{Command, CommandError, Session};
use zen_db::assist::{Access, Catalogue, Target};
use zen_db::dialect::SqlDialect;
use zen_db::schema::{ConstraintKind, Object, ObjectKind, Snapshot};
use zen_db::sql::Dialect;
use zen_i18n::t;
use zen_syntax::Highlighting;
use zen_syntax::inject::{Code, Injector};

use super::assist::{dialects, kind_word, target_at};
use super::objects::object_tab;
use super::scripts::{self, ScriptBinding};
use super::sources::find;
use super::structure::{self, Structure};
use crate::brackets::HighlighterSource;
use crate::find_results::FindUsagesOptions;
use crate::find_results::provided::{self, Elsewhere, NodeCommand, UsageFinder, UsageRow};
use crate::language::show_usages::UsageAccess;
use crate::search::SearchParams;

const LISTED: usize = 50;

const MAX_WAIT: u64 = 10_000;

/// What `db.usages.find` looks for.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct UsagesParams {
    /// The connection's id, with `path`.
    pub connection: Option<String>,
    /// The object's path `[db, schema, folder, object]`, or a column's `[db, schema, folder, table, "columns", column]`, as db.navigator.* rows give them.
    pub path: Option<Vec<String>>,
    /// A column of the object at `path`.
    pub column: Option<String>,
    /// An open console, `.sql` file or code file whose name at `offset` is looked for, instead of `connection` and `path`.
    pub file: Option<PathBuf>,
    /// The character offset in `file`; its caret when absent.
    pub offset: Option<usize>,
    /// Whether the usages open on a tab of their own; the Find Usages options' when absent.
    pub new_tab: Option<bool>,
    /// How long to wait for the search to end, at most 10 000 ms; none by default.
    pub wait_ms: Option<u64>,
    /// The most usages to answer; 50 when absent.
    pub limit: Option<usize>,
}

/// What `db.usages.rename` looks for and the name it suggests.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RenameUsagesParams {
    /// What is renamed, as db.usages.find takes it.
    #[serde(flatten)]
    pub usages: UsagesParams,
    /// The new name.
    pub new_name: String,
}

/// What `db.usages.find` and `db.usages.rename` answer.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UsagesAnswer {
    /// The tab of the Find tool window that lists them; search.results.tree reads it, search.results.replace replaces in files.
    pub tab: u64,
    /// The connection.
    pub connection: String,
    /// What was looked for.
    pub target: Target,
    /// What it is, e.g. `Table public.orders`.
    pub label: String,
    /// The new name, for a rename.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_name: Option<String>,
    /// Whether the search ended.
    pub done: bool,
    /// The usages found so far, files first, at most `limit`.
    pub usages: Vec<UsageRow>,
    /// How many were found so far.
    pub total: usize,
    /// Whether usages were left out because of `limit`.
    pub truncated: bool,
}

struct Looked {
    connection: String,
    target: Target,
    object: Object,
    schema: String,
}

fn refused(code: &str, message: String) -> CommandError {
    CommandError::with_code(code, message)
}

fn snapshot_of(session: &mut Session, connection: &str) -> Result<Snapshot, CommandError> {
    let entry = find(session, connection)?;
    structure::ensure(session, &entry, true);
    let snapshot = session
        .state::<Structure>()
        .and_then(|structure| structure.snapshot(connection))
        .cloned()
        .unwrap_or_else(|| Snapshot::new(connection, &entry.source.driver));
    let snapshot = zen_db::assist::relational_view(&snapshot).unwrap_or(snapshot);
    if snapshot.catalogs.is_empty() {
        return Err(refused("noStructure", t!("database-usages-no-structure")));
    }
    Ok(snapshot)
}

fn looked(snapshot: &Snapshot, connection: &str, target: Target) -> Result<Looked, CommandError> {
    let path = target.path().to_vec();
    let found = zen_db::schema::find_object(snapshot, &path)
        .ok_or_else(|| refused("unknownObject", t!("database-usages-unknown", name = path.join("."))))?;
    if matches!(found.object.kind, ObjectKind::Sequence | ObjectKind::Type | ObjectKind::Event) {
        return Err(refused("unsupported", t!("database-usages-unsupported")));
    }
    if let Target::Column { column, .. } = &target
        && !found.object.columns.iter().any(|known| known.name == *column)
    {
        return Err(refused("unknownObject", t!("database-usages-unknown", name = column.as_str())));
    }
    Ok(Looked {
        connection: connection.to_owned(),
        target,
        object: found.object.clone(),
        schema: found.schema.name.clone(),
    })
}

fn target_of(session: &mut Session, params: &UsagesParams) -> Result<(Looked, Snapshot), CommandError> {
    if let Some(file) = &params.file {
        let path =
            session.state::<crate::Workbench>().map_or_else(|| file.clone(), |workbench| workbench.resolve(file));
        let offset = match params.offset {
            Some(offset) => offset,
            None => session
                .state::<crate::Workbench>()
                .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
                .map(|document| crate::language::char_offset(document.text(), document.selections().primary().head))
                .unwrap_or_default(),
        };
        let (connection, target) = target_at(session, &path, offset)
            .ok_or_else(|| refused("unknownName", t!("workbench-nav-no-declaration")))?;
        if matches!(target, Target::Schema { .. }) {
            return Err(refused("unsupported", t!("database-usages-unsupported")));
        }
        let snapshot = snapshot_of(session, &connection)?;
        return Ok((looked(&snapshot, &connection, target)?, snapshot));
    }
    let (connection, path) = match (&params.connection, &params.path) {
        (Some(connection), Some(path)) => (connection.clone(), path.clone()),
        _ => {
            let (_, data) = object_tab(session, None)?;
            (data.connection, data.path)
        }
    };
    let snapshot = snapshot_of(session, &connection)?;
    let target = match (path.as_slice(), &params.column) {
        ([_, _, _, _], Some(column)) => Target::Column { path: path.clone(), column: column.clone() },
        ([_, _, _, _], None) => Target::Object { path: path.clone() },
        ([object @ .., sub, column], _) if object.len() == 4 && sub == "columns" => {
            Target::Column { path: object.to_vec(), column: column.clone() }
        }
        _ => return Err(refused("unsupported", t!("database-usages-unsupported"))),
    };
    Ok((looked(&snapshot, &connection, target)?, snapshot))
}

fn plain(name: &str) -> &str {
    name.split_once('(').map_or(name, |(plain, _)| plain)
}

fn label_of(looked: &Looked) -> String {
    let name = format!("{}.{}", looked.schema, plain(&looked.object.name));
    match &looked.target {
        Target::Column { column, .. } => {
            t!("database-usages-target", kind = t!("database-sql-kind-column"), name = format!("{name}.{column}"))
        }
        _ => t!("database-usages-target", kind = kind_word(looked.object.kind), name = name),
    }
}

fn shown_name(looked: &Looked) -> String {
    match &looked.target {
        Target::Column { column, .. } => format!("{}.{column}", plain(&looked.object.name)),
        _ => plain(&looked.object.name).to_owned(),
    }
}

fn access(access: Option<Access>) -> Option<UsageAccess> {
    access.map(|access| match access {
        Access::Read => UsageAccess::Read,
        Access::Write => UsageAccess::Write,
    })
}

fn char_offsets(text: &str, bytes: &[usize]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..bytes.len()).collect();
    order.sort_by_key(|at| bytes[*at]);
    let mut chars = vec![0; bytes.len()];
    let mut walked = text.char_indices().map(|(byte, _)| byte).enumerate().peekable();
    let mut count = 0;
    for at in order {
        while let Some(&(index, byte)) = walked.peek() {
            if byte >= bytes[at] {
                break;
            }
            count = index + 1;
            walked.next();
        }
        chars[at] = count;
    }
    chars
}

struct DbFinder {
    connection: String,
    group: String,
    label: String,
    shown: String,
    new_name: Option<String>,
    target: Target,
    name: String,
    schema: String,
    table: String,
    snapshot: Snapshot,
    dialect: SqlDialect,
    split: Dialect,
    database: Option<String>,
    bound: HashMap<PathBuf, ScriptBinding>,
    project_default: Option<ScriptBinding>,
    consoles: HashMap<PathBuf, Option<String>>,
    known: Vec<String>,
    highlighters: Option<HighlighterSource>,
    project: Option<PathBuf>,
}

enum Piece {
    Whole,
    Code(Code),
}

impl DbFinder {
    fn binding(&self, path: &Path, code: bool) -> Option<Option<String>> {
        if let Some(schema) = self.consoles.get(path) {
            return Some(schema.clone());
        }
        let own = self.bound.get(path).filter(|binding| self.known.contains(&binding.connection));
        let binding = own.or(if code { self.project_default.as_ref() } else { None });
        match binding {
            Some(binding) if binding.connection == self.connection => Some(binding.schema.clone()),
            Some(_) => None,
            None => Some(None),
        }
    }

    fn pieces(&self, path: &Path, text: &str) -> Vec<Piece> {
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        if name.to_ascii_lowercase().ends_with(".sql") || self.consoles.contains_key(path) {
            return vec![Piece::Whole];
        }
        if !zen_db::inject::hosts(&name) {
            return Vec::new();
        }
        let Some(highlighter) = self.highlighters.as_ref().and_then(|source| source(&name)) else {
            return Vec::new();
        };
        let mut highlighting = Highlighting::new(highlighter, text);
        let injector: Arc<dyn Injector> = Arc::new(zen_db::inject::SqlStrings::new(None));
        highlighting.inject(text, vec![injector]);
        highlighting
            .fragments()
            .iter()
            .filter(|fragment| &*fragment.language == "sql")
            .map(|fragment| Piece::Code(fragment.code(text)))
            .collect()
    }

    fn in_sql(&self, sql: &str, schema: Option<&str>) -> Vec<(Range<usize>, Option<Access>)> {
        let view = Catalogue::new(&self.snapshot, self.dialect, schema, self.database.as_deref());
        let mut found = Vec::new();
        for statement in zen_db::sql::split(sql, self.split) {
            if statement.client {
                continue;
            }
            let text = statement.text(sql);
            for usage in zen_db::assist::usages(text, &view, &self.target) {
                found.push((statement.start + usage.range.start..statement.start + usage.range.end, usage.access));
            }
        }
        found
    }

    fn holder_path(&self, schema: &str, object: &Object) -> Vec<String> {
        let catalog = self.target.path().first().cloned().unwrap_or_default();
        vec![catalog, schema.to_owned(), object.kind.folder().to_owned(), object.key()]
    }

    fn open(&self, path: Vec<String>, page: &str) -> NodeCommand {
        NodeCommand {
            command: "db.object.open".to_owned(),
            params: json!({ "connection": self.connection, "path": path, "page": page }),
        }
    }

    fn push(
        &self,
        out: &mut Vec<Elsewhere>,
        holder: String,
        key: String,
        text: &str,
        ranges: Vec<(Range<usize>, Option<Access>)>,
        open: &NodeCommand,
    ) {
        let starts: Vec<usize> = ranges.iter().flat_map(|(range, _)| [range.start, range.end]).collect();
        let chars = char_offsets(text, &starts);
        for (index, (_, used)) in ranges.into_iter().enumerate() {
            out.push(Elsewhere {
                group: self.group.clone(),
                holder: holder.clone(),
                key: key.clone(),
                text: text.to_owned(),
                range: chars[index * 2]..chars[index * 2 + 1],
                access: access(used),
                open: open.clone(),
            });
        }
    }

    fn is_target(&self, schema: &str, object: &Object) -> bool {
        self.target.path().get(1).is_some_and(|known| known == schema)
            && self.target.path().get(3).is_some_and(|key| *key == object.key())
            && self.target.path().get(2).is_some_and(|folder| folder == object.kind.folder())
    }

    fn holder_label(&self, kind: String, schema: &str, name: &str) -> String {
        format!("{kind} {schema}.{}", plain(name))
    }

    fn definitions(&self, cancel: &AtomicBool) -> Vec<Elsewhere> {
        let mut out = Vec::new();
        let column = matches!(self.target, Target::Column { .. });
        for catalog in
            self.snapshot.catalogs.iter().filter(|catalog| catalog.current || self.snapshot.catalogs.len() == 1)
        {
            for schema in catalog.schemas.iter().filter(|schema| !schema.system) {
                if cancel.load(Ordering::Relaxed) {
                    return out;
                }
                for object in &schema.objects {
                    let own = self.is_target(&schema.name, object);
                    let path = self.holder_path(&schema.name, object);
                    let label = self.holder_label(kind_word(object.kind), &schema.name, &object.name);
                    if matches!(object.kind, ObjectKind::View | ObjectKind::MaterializedView)
                        && !own
                        && let Some(definition) = &object.definition
                    {
                        let found = self.in_sql(definition, Some(&schema.name));
                        if !found.is_empty() {
                            let open = self.open(path.clone(), "ddl");
                            self.push(
                                &mut out,
                                label.clone(),
                                format!("{}#definition", path.join("/")),
                                definition,
                                found,
                                &open,
                            );
                        }
                    }
                    if matches!(object.kind, ObjectKind::Function | ObjectKind::Procedure) && !own {
                        let body = object
                            .routine
                            .as_ref()
                            .and_then(|routine| routine.definition.as_ref())
                            .or(object.definition.as_ref());
                        if let Some(body) = body {
                            let names_table =
                                !column || !zen_db::assist::mentions(body, self.dialect, &self.table, None).is_empty();
                            let qualifier = (!column).then_some(self.schema.as_str());
                            let found: Vec<_> = if names_table {
                                zen_db::assist::mentions(body, self.dialect, &self.name, qualifier)
                                    .into_iter()
                                    .map(|range| (range, None))
                                    .collect()
                            } else {
                                Vec::new()
                            };
                            if !found.is_empty() {
                                let open = self.open(path.clone(), "ddl");
                                self.push(
                                    &mut out,
                                    label.clone(),
                                    format!("{}#body", path.join("/")),
                                    body,
                                    found,
                                    &open,
                                );
                            }
                        }
                    }
                    for trigger in &object.triggers {
                        let routine = matches!(self.target_kind(), Some(ObjectKind::Function | ObjectKind::Procedure));
                        let found = if own || (routine && !column) {
                            zen_db::assist::mentions(&trigger.definition, self.dialect, &self.name, None)
                        } else {
                            Vec::new()
                        };
                        if !found.is_empty() {
                            let open = self.open(path.clone(), "properties");
                            let label = format!("{} {} · {}", t!("database-usages-kind-trigger"), trigger.name, label);
                            let found = found.into_iter().map(|range| (range, None)).collect();
                            self.push(
                                &mut out,
                                label,
                                format!("{}#trigger:{}", path.join("/"), trigger.name),
                                &trigger.definition,
                                found,
                                &open,
                            );
                        }
                    }
                    for constraint in
                        object.constraints.iter().filter(|constraint| constraint.kind == ConstraintKind::ForeignKey)
                    {
                        let Some(reference) = &constraint.references else { continue };
                        let to_target = reference.schema == self.schema && reference.table == self.table;
                        let used = match &self.target {
                            Target::Column { column, .. } => {
                                (to_target && reference.columns.contains(column))
                                    || (own && constraint.columns.contains(column))
                            }
                            Target::Object { .. } => {
                                to_target && !own && self.target_kind().is_some_and(ObjectKind::has_columns)
                            }
                            Target::Schema { .. } => false,
                        };
                        if !used {
                            continue;
                        }
                        let text = if constraint.definition.is_empty() {
                            format!(
                                "FOREIGN KEY ({}) REFERENCES {}.{}({})",
                                constraint.columns.join(", "),
                                reference.schema,
                                reference.table,
                                reference.columns.join(", ")
                            )
                        } else {
                            constraint.definition.clone()
                        };
                        let mut found = zen_db::assist::mentions(&text, self.dialect, &self.name, None);
                        if found.is_empty() {
                            found.push(0..0);
                        }
                        let open = self.open(path.clone(), "properties");
                        let label =
                            format!("{} {} · {}", t!("database-usages-kind-foreign-key"), constraint.name, label);
                        let found = found.into_iter().map(|range| (range, None)).collect();
                        self.push(
                            &mut out,
                            label,
                            format!("{}#fk:{}", path.join("/"), constraint.name),
                            &text,
                            found,
                            &open,
                        );
                    }
                }
            }
        }
        out
    }

    fn target_kind(&self) -> Option<ObjectKind> {
        self.target.path().get(2).and_then(|folder| ObjectKind::from_folder(folder))
    }
}

impl UsageFinder for DbFinder {
    fn title(&self) -> String {
        match &self.new_name {
            Some(new_name) => t!("database-usages-rename-title", name = self.shown.as_str(), to = new_name.as_str()),
            None => t!("database-usages-title", name = self.shown.as_str()),
        }
    }

    fn target(&self) -> String {
        match &self.new_name {
            Some(new_name) => t!("database-usages-rename-target", target = self.label.as_str(), to = new_name.as_str()),
            None => self.label.clone(),
        }
    }

    fn scope(&self) -> String {
        t!("database-usages-scope", connection = self.group.as_str())
    }

    fn request(&self) -> serde_json::Value {
        json!({ "connection": self.connection, "target": self.target })
    }

    fn search(&self) -> SearchParams {
        let mut masks = vec!["*.sql".to_owned()];
        masks.extend(zen_db::inject::host_extensions().iter().map(|extension| format!("*.{extension}")));
        SearchParams {
            query: zen_text::search::Query {
                text: self.name.clone(),
                whole_words: true,
                ..zen_text::search::Query::default()
            },
            mask: masks.join(","),
            ..SearchParams::default()
        }
    }

    fn extra(&self) -> Vec<(PathBuf, String)> {
        let mut consoles: Vec<(PathBuf, String)> = self
            .consoles
            .keys()
            .filter(|path| self.project.as_deref().is_none_or(|root| !path.starts_with(root)))
            .map(|path| {
                let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                (path.clone(), format!("{}/{name}", t!("database-usages-consoles")))
            })
            .collect();
        consoles.sort();
        consoles
    }

    fn keep(&self, path: &Path, text: &str, _: Vec<Range<usize>>) -> Vec<(Range<usize>, Option<UsageAccess>)> {
        let pieces = self.pieces(path, text);
        let code = pieces.iter().any(|piece| matches!(piece, Piece::Code(_)));
        let Some(schema) = self.binding(path, code) else { return Vec::new() };
        let mut bytes: Vec<(Range<usize>, Option<Access>)> = Vec::new();
        for piece in &pieces {
            match piece {
                Piece::Whole => bytes.extend(self.in_sql(text, schema.as_deref())),
                Piece::Code(code) => {
                    for (range, used) in self.in_sql(&code.text, schema.as_deref()) {
                        bytes.push((code.to_text(range.start)..code.to_text(range.end), used));
                    }
                }
            }
        }
        bytes.sort_by_key(|(range, _)| range.start);
        bytes.dedup_by_key(|(range, _)| range.start);
        let ends: Vec<usize> = bytes.iter().flat_map(|(range, _)| [range.start, range.end]).collect();
        let chars = char_offsets(text, &ends);
        bytes
            .into_iter()
            .enumerate()
            .map(|(index, (_, used))| (chars[index * 2]..chars[index * 2 + 1], access(used)))
            .collect()
    }

    fn elsewhere(&self, cancel: &AtomicBool) -> Vec<Elsewhere> {
        self.definitions(cancel)
    }
}

fn finder(
    session: &mut Session,
    looked: Looked,
    snapshot: Snapshot,
    new_name: Option<String>,
) -> Result<DbFinder, CommandError> {
    let (dialect, split, database) = dialects(session, &looked.connection)
        .ok_or_else(|| refused("unsupported", t!("database-usages-unsupported")))?;
    let group =
        find(session, &looked.connection).map(|entry| entry.source.name).unwrap_or_else(|_| looked.connection.clone());
    let consoles = scripts::ListScripts
        .run(session, scripts::ListParams { connection: Some(looked.connection.clone()), limit: Some(usize::MAX) })
        .map(|listed| listed.scripts)
        .unwrap_or_default()
        .into_iter()
        .filter(|entry| entry.console)
        .map(|entry| (entry.path, entry.schema))
        .collect();
    let known = super::consoles::known_connections(session);
    let name = match &looked.target {
        Target::Column { column, .. } => column.clone(),
        _ => plain(&looked.object.name).to_owned(),
    };
    Ok(DbFinder {
        group,
        label: label_of(&looked),
        shown: shown_name(&looked),
        new_name,
        name,
        table: plain(&looked.object.name).to_owned(),
        schema: looked.schema,
        target: looked.target,
        connection: looked.connection,
        snapshot,
        dialect,
        split,
        database,
        bound: scripts::all_bound(session).into_iter().collect(),
        project_default: scripts::project_default(session),
        consoles,
        known,
        highlighters: crate::search::highlighters(session),
        project: session.state::<crate::Workbench>().and_then(crate::Workbench::project).map(Path::to_owned),
    })
}

fn open(session: &mut Session, params: &UsagesParams, new_name: Option<String>) -> Result<UsagesAnswer, CommandError> {
    let (looked, snapshot) = target_of(session, params)?;
    let connection = looked.connection.clone();
    let target = looked.target.clone();
    let finder = finder(session, looked, snapshot, new_name.clone())?;
    let label = finder.label.clone();
    let new_tab = params.new_tab.unwrap_or_else(|| FindUsagesOptions::of(session).new_tab);
    let tab = provided::open_tab(session, Arc::new(finder), new_name.clone(), new_tab)?;
    let done = provided::wait(session, tab, Duration::from_millis(params.wait_ms.unwrap_or(0).min(MAX_WAIT)));
    let limit = params.limit.unwrap_or(LISTED);
    let (usages, total, _) = provided::rows(session, tab, limit).unwrap_or_default();
    Ok(UsagesAnswer { tab, connection, target, label, new_name, done, usages, truncated: total > limit, total })
}

pub(crate) fn find_at(
    session: &mut Session,
    path: &Path,
    offset: usize,
    new_tab: Option<bool>,
    wait_ms: Option<u64>,
) -> Option<Result<UsagesAnswer, CommandError>> {
    target_at(session, path, offset).filter(|(_, target)| !matches!(target, Target::Schema { .. }))?;
    let params =
        UsagesParams { file: Some(path.to_owned()), offset: Some(offset), new_tab, wait_ms, ..UsagesParams::default() };
    Some(open(session, &params, None))
}

pub struct FindDbUsages;

impl Command for FindDbUsages {
    type Params = UsagesParams;
    type Output = UsagesAnswer;
    const ID: &'static str = "db.usages.find";
    const TITLE: &'static str = "Find Usages of Database Object";
    const DESCRIPTION: &'static str = "Finds the usages of a table, view, routine or column, as ⌥F7 in the navigator, an object tab or SQL does: the object is `connection` and `path` (a db.navigator.* row's path; a column's `[db, schema, folder, table, \"columns\", column]`, or `column`), else the name at `offset` of the open console, script or code file `file`, else the active object tab. Looks, in the background (a cancellable task), in the project's `.sql` files and SQL strings of code bound to this connection or to none (by their binding, else resolved against this connection), the connection's consoles, and the snapshot's views, routines, triggers and foreign keys; names are resolved as ⌘B resolves them (aliases, CTEs, schemas), statements that write mark the usage `write`. Lists them on a tab of the Find tool window (search.results.tree reads it). `waitMs` waits for the end; answers the usages found, files first, at most `limit` (50) with `total` and `truncated`. Never connects; refused with `unsupported` for schemas, sequences, types and key-value stores.";

    fn run(&self, session: &mut Session, params: UsagesParams) -> Result<UsagesAnswer, CommandError> {
        open(session, &params, None)
    }
}

pub struct RenameDbUsages;

impl Command for RenameDbUsages {
    type Params = RenameUsagesParams;
    type Output = UsagesAnswer;
    const ID: &'static str = "db.usages.rename";
    const TITLE: &'static str = "Rename Usages of Database Object";
    const DESCRIPTION: &'static str = "Lists the usages of a table, view, routine or column (as db.usages.find takes it) on a tab of the Find tool window that suggests replacing them with `newName` in files: search.results.replace with `nodes: [\"found\"]` replaces every one not excluded, or chosen nodes; usages in consoles and in the database's own objects are listed but never replaced. Changes nothing by itself and never the database: the rename there is a structure change (db.structure.stage). Answers as db.usages.find.";

    fn run(&self, session: &mut Session, params: RenameUsagesParams) -> Result<UsagesAnswer, CommandError> {
        let new_name = params.new_name.trim().to_owned();
        if new_name.is_empty() {
            return Err(refused("noName", t!("database-usages-no-new-name")));
        }
        open(session, &params.usages, Some(new_name))
    }
}

#[cfg(test)]
mod tests;
