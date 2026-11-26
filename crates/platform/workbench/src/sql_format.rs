use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_db::format::{Flavor, FormatOptions, Formatted};
use zen_db::sql::Dialect;
use zen_i18n::t;

use crate::Workbench;
use crate::language::format::FormatResult;

pub const OPTIONS: &str = "editor.sql.format";

pub const EXTENSIONS: &[&str] = &["sql", "ddl", "dml", "pgsql", "psql", "mysql", "plsql"];

const MAX_TEXT: usize = 1024 * 1024;

const DEFAULT_LIMIT: usize = 20_000;

const MAX_LIMIT: usize = 200_000;

pub fn is_sql(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| EXTENSIONS.iter().any(|known| known.eq_ignore_ascii_case(extension)))
}

pub fn options(session: &Session) -> FormatOptions {
    session.settings().get(OPTIONS).unwrap_or_default()
}

#[cfg(not(target_arch = "wasm32"))]
fn language_of(session: &Session, connection: &str) -> Option<String> {
    let entry = crate::database::sources::find(session, connection).ok()?;
    session
        .state::<crate::database::Database>()?
        .drivers()
        .iter()
        .find(|driver| driver.id() == entry.source.driver)
        .map(|driver| driver.capabilities().language.clone())
}

#[cfg(target_arch = "wasm32")]
fn language_of(_session: &Session, _connection: &str) -> Option<String> {
    None
}

#[cfg(not(target_arch = "wasm32"))]
fn connection_of(session: &Session, path: &Path) -> Option<String> {
    session.state::<crate::database::Database>()?;
    crate::database::consoles::connection_of(session, path)
}

#[cfg(target_arch = "wasm32")]
fn connection_of(_session: &Session, _path: &Path) -> Option<String> {
    None
}

pub fn flavor_of(session: &Session, path: &Path) -> Flavor {
    connection_of(session, path)
        .and_then(|connection| language_of(session, &connection))
        .map(|language| Flavor::of_language(&language))
        .unwrap_or_else(|| {
            Flavor::of_extension(path.extension().and_then(|extension| extension.to_str()).unwrap_or_default())
        })
}

struct Run {
    path: PathBuf,
    version: u64,
    within: Option<Range<usize>>,
    cancel: Arc<AtomicBool>,
    #[cfg(not(target_arch = "wasm32"))]
    answer: std::sync::mpsc::Receiver<Option<Formatted>>,
    #[cfg(target_arch = "wasm32")]
    answer: Option<Formatted>,
}

impl Run {
    #[cfg(not(target_arch = "wasm32"))]
    fn wait(&self, wait: Duration) -> Option<Option<Formatted>> {
        match self.answer.recv_timeout(wait) {
            Ok(answer) => Some(answer),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Some(None),
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn wait(&mut self, _wait: Duration) -> Option<Option<Formatted>> {
        Some(self.answer.take())
    }
}

#[derive(Default)]
struct SqlFormatting {
    run: Option<Run>,
}

struct Job {
    text: String,
    flavor: Flavor,
    options: FormatOptions,
    unit: String,
}

fn spawn(job: Job, path: &Path, version: u64, within: Option<Range<usize>>) -> Run {
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let range = within.clone();
    let work = move || {
        zen_db::format::format(&job.text, job.flavor, &job.options, &job.unit, range, &|| flag.load(Ordering::Relaxed))
    };
    #[cfg(not(target_arch = "wasm32"))]
    let answer = {
        let (sender, answer) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(work());
        });
        answer
    };
    #[cfg(target_arch = "wasm32")]
    let answer = work();
    Run { path: path.to_owned(), version, within, cancel, answer }
}

fn byte_at(text: &str, chars: usize) -> usize {
    text.char_indices().nth(chars).map_or(text.len(), |(byte, _)| byte)
}

fn char_edits(text: &str, edits: Vec<(Range<usize>, String)>) -> Vec<(Range<usize>, String)> {
    let mut out = Vec::with_capacity(edits.len());
    let (mut byte, mut chars) = (0, 0);
    for (range, replacement) in edits {
        chars += text[byte..range.start].chars().count();
        let start = chars;
        chars += text[range.start..range.end].chars().count();
        byte = range.end;
        out.push((start..chars, replacement));
    }
    out
}

pub fn reformat(session: &mut Session, path: &Path, whole_file: bool, wait: Duration) -> FormatResult {
    let Some(workbench) = session.state::<Workbench>() else {
        return FormatResult { error: Some(t!("workbench-error-no-document")), ..FormatResult::default() };
    };
    let active = workbench.active_document().is_some_and(|document| document.path() == path);
    let Some(document) = workbench.documents().iter().find(|document| document.path() == path) else {
        return FormatResult { error: Some(t!("workbench-error-no-document")), ..FormatResult::default() };
    };
    let (text, version) = (document.text().to_owned(), document.version());
    let selection = document.selections().primary().range();
    let within = (active && !whole_file && !selection.is_empty())
        .then(|| byte_at(&text, selection.start)..byte_at(&text, selection.end));
    let mut state = std::mem::take(session.state_mut::<SqlFormatting>());
    #[cfg_attr(not(target_arch = "wasm32"), allow(unused_mut))]
    let mut run = match state.run.take() {
        Some(run) if run.path == path && run.version == version && run.within == within => run,
        other => {
            if let Some(old) = other {
                old.cancel.store(true, Ordering::Relaxed);
            }
            let style = crate::file_indent::configured_at(session, path).style;
            let flavor = flavor_of(session, path);
            let job = Job {
                text: text.clone(),
                flavor,
                options: options(session),
                unit: crate::code_style::indent_unit(style),
            };
            spawn(job, path, version, within)
        }
    };
    let result = match run.wait(wait) {
        None => {
            state.run = Some(run);
            FormatResult { pending: true, ..FormatResult::default() }
        }
        Some(None) => FormatResult { error: Some(t!("workbench-format-sql-stopped")), ..FormatResult::default() },
        Some(Some(formatted)) => apply(session, path, version, &text, formatted),
    };
    session.set_state(state);
    result
}

fn apply(session: &mut Session, path: &Path, version: u64, text: &str, formatted: Formatted) -> FormatResult {
    if formatted.edits.is_empty() {
        let error = (formatted.kept > 0).then(|| t!("workbench-format-sql-kept", count = formatted.kept));
        return FormatResult { error, ..FormatResult::default() };
    }
    let keep_marked = crate::project_settings::formatter_markers(session);
    let workbench = session.state_mut::<Workbench>();
    let Some(document) = workbench.documents.iter_mut().find(|document| document.path() == path) else {
        return FormatResult { error: Some(t!("workbench-error-no-document")), ..FormatResult::default() };
    };
    if document.version() != version {
        return FormatResult { error: Some(t!("workbench-format-outdated")), ..FormatResult::default() };
    }
    let protected = if keep_marked { crate::formatter_markers::protected_ranges(text) } else { Vec::new() };
    let mut edits = formatted.edits;
    edits.retain(|(edit, _)| !protected.iter().any(|range| edit.start < range.end && range.start < edit.end));
    let changed = document.replace_ranges(char_edits(text, edits));
    FormatResult { changed, ..FormatResult::default() }
}

/// The parameters of `db.sql.format`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SqlFormatParams {
    /// The SQL to format, at most 1 MiB.
    pub text: String,
    /// The dialect it is read in; standard SQL when neither it nor `connection` is given.
    #[serde(default)]
    pub dialect: Option<Dialect>,
    /// A connection whose driver gives the dialect; nothing connects.
    #[serde(default)]
    pub connection: Option<String>,
    /// The options; those of Code Style → SQL when absent.
    #[serde(default)]
    pub options: Option<FormatOptions>,
    /// Spaces per indentation level, from 1 to 16; the SQL code style's when absent.
    #[serde(default)]
    pub indent: Option<u8>,
    /// Indent with tabs instead of spaces; the SQL code style's when absent.
    #[serde(default)]
    pub use_tabs: Option<bool>,
    /// The most characters of the formatted text answered, 20 000 by default, at most 200 000.
    #[serde(default)]
    pub limit: Option<usize>,
}

/// What `db.sql.format` gives.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SqlFormatOutput {
    /// The formatted text, cut to `limit` characters.
    pub text: String,
    /// Whether `text` was cut.
    pub truncated: bool,
    /// Whether formatting changed anything.
    pub changed: bool,
    /// How many statements were looked at.
    pub statements: usize,
    /// How many were left as they were because they could not be formatted safely.
    pub kept: usize,
}

pub struct FormatSql;

impl Command for FormatSql {
    type Params = SqlFormatParams;
    type Output = SqlFormatOutput;
    const ID: &'static str = "db.sql.format";
    const TITLE: &'static str = "Format SQL Text";
    const DESCRIPTION: &'static str = "Formats SQL given as `text` (at most 1 MiB) as Reformat Code formats a console: keyword case, a line per clause and list item, commas trailing or leading, the indentation of the SQL code style; strings, comments and quoted names stay byte for byte, and statements that cannot be formatted safely stay as they are (`kept`). The dialect comes from `dialect`, else the driver of `connection` (nothing connects), else standard SQL; `options`, `indent` and `useTabs` override Code Style → SQL. Answers the text cut to `limit` characters (20 000) with `truncated`. To format an open file or console in place use editor.code.reformat.";

    fn run(&self, session: &mut Session, params: SqlFormatParams) -> Result<SqlFormatOutput, CommandError> {
        if params.text.len() > MAX_TEXT {
            return Err(CommandError::new(t!("workbench-format-sql-too-long")));
        }
        let flavor = match (params.dialect, &params.connection) {
            (Some(dialect), _) => Flavor::of_dialect(dialect),
            (None, Some(connection)) => match language_of(session, connection) {
                Some(language) => Flavor::of_language(&language),
                None => {
                    return Err(CommandError::new(t!("workbench-format-sql-no-connection", id = connection.as_str())));
                }
            },
            (None, None) => Flavor::of_dialect(Dialect::Standard),
        };
        let mut style = crate::code_style::for_path(session.settings(), Path::new("query.sql"));
        if let Some(indent) = params.indent {
            style.indent = indent.clamp(1, 16);
        }
        if let Some(tabs) = params.use_tabs {
            style.use_tabs = tabs;
        }
        let options = params.options.unwrap_or_else(|| options(session));
        let formatted = zen_db::format::format(
            &params.text,
            flavor,
            &options,
            &crate::code_style::indent_unit(style),
            None,
            &|| false,
        )
        .unwrap_or_default();
        let text = formatted.apply(&params.text);
        let limit = params.limit.unwrap_or(DEFAULT_LIMIT).min(MAX_LIMIT);
        let truncated = text.chars().count() > limit;
        let text = if truncated { text.chars().take(limit).collect() } else { text };
        Ok(SqlFormatOutput {
            text,
            truncated,
            changed: !formatted.edits.is_empty(),
            statements: formatted.statements,
            kept: formatted.kept,
        })
    }
}

#[cfg(test)]
mod tests;
