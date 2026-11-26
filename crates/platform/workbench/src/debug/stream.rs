use std::path::PathBuf;
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;

use super::session::{DebugSession, REQUEST_TIMEOUT};
use super::{WAIT_LIMIT, find_session, views};
use crate::Workbench;
use crate::java::stream::{ChainRefusal, StepKind, TraceStep, chain_at, trace_steps};

const LIMIT_DEFAULT: i64 = 100;
const LIMIT_MAX: i64 = 1000;

/// The parameters of `debug.stream.trace`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TraceParams {
    /// The session; the current one when absent.
    pub session_id: Option<u64>,
    /// The frame the chain is evaluated in; the selected one when absent.
    pub frame_id: Option<i64>,
    /// The file; the frame's when absent.
    pub path: Option<PathBuf>,
    /// The line, from 0; the frame's when absent.
    pub line: Option<u32>,
    /// How many values each column lists, at most 1000; 100 when absent.
    pub limit: Option<i64>,
    /// How long to wait for the debugger, at most 10 000; 0 answers `pending` at once with the columns known so far, then call again with the same parameters.
    pub wait_ms: Option<u64>,
}

/// One operation of a traced stream chain and the values after it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TraceColumn {
    /// The call as written, such as `filter(n -> n > 0)`; the first column is the stream's source.
    pub label: String,
    /// `elements` (the values after the operation), `result` (what the terminal operation returns), `unbounded` (an infinite stream before its `limit`, not evaluated) or `notRun` (a terminal operation with side effects, such as `forEach`, not evaluated).
    pub kind: String,
    /// The expression evaluated for it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    /// The values, at most `limit`.
    pub values: Vec<String>,
    /// How many values there are in all.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<i64>,
    /// The terminal operation's result, as text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    /// Why it could not be evaluated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Whether the debugger has not answered for it yet.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
}

/// What `debug.stream.trace` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamTrace {
    /// The chain as written.
    pub chain: String,
    pub columns: Vec<TraceColumn>,
    /// Whether some columns wait for the debugger; ask again.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
}

fn refusal(refusal: ChainRefusal) -> CommandError {
    CommandError::new(match refusal {
        ChainRefusal::NoStream => t!("debug-stream-none"),
        ChainRefusal::NotCreatedHere => t!("debug-stream-not-created"),
        ChainRefusal::NoOperations => t!("debug-stream-no-operations"),
        ChainRefusal::SideEffect(call) => t!("debug-stream-side-effect", call = call),
        ChainRefusal::Infinite => t!("debug-stream-infinite"),
    })
}

fn kind_name(kind: &StepKind) -> &'static str {
    match kind {
        StepKind::Elements => "elements",
        StepKind::Result => "result",
        StepKind::Unbounded => "unbounded",
        StepKind::NotRun => "notRun",
    }
}

enum Filled {
    Done,
    Pending,
}

fn fill(
    session: &mut Session,
    debug: &std::sync::Arc<DebugSession>,
    frame: i64,
    step: &TraceStep,
    column: &mut TraceColumn,
    limit: i64,
    wait: Duration,
) -> Filled {
    let Some(expression) = &step.expression else { return Filled::Done };
    let arguments = serde_json::json!({ "expression": expression, "frameId": frame, "context": "watch" });
    let body = match debug.ask_once("evaluate", arguments, wait) {
        Ok(Some(body)) => body,
        Ok(None) => return Filled::Pending,
        Err(error) => {
            column.error = Some(error);
            return Filled::Done;
        }
    };
    let answer = match serde_json::from_value::<zen_dap::EvaluateResponse>(body) {
        Ok(answer) => answer,
        Err(error) => {
            column.error = Some(error.to_string());
            return Filled::Done;
        }
    };
    if step.kind == StepKind::Result {
        column.result = Some(answer.result.clone());
    }
    column.count = answer.indexed_variables.filter(|count| *count > 0 || answer.variables_reference > 0);
    if answer.variables_reference <= 0 {
        return Filled::Done;
    }
    let ready = if wait.is_zero() {
        views::view_ready(session, debug, false)
    } else {
        views::wait_for_view(session, debug, false, wait).unwrap_or(false)
    };
    if !ready {
        return Filled::Pending;
    }
    let count = answer.indexed_variables.map(|count| count.min(limit));
    let listed = match debug.variables_soon(answer.variables_reference, count.map(|_| 0), count, false, wait) {
        Ok(Some(listed)) => listed,
        Ok(None) => return Filled::Pending,
        Err(error) => {
            column.error = Some(error.message);
            return Filled::Done;
        }
    };
    let Ok(Some(listed)) = views::resolve_lazy(debug, listed, false, wait) else { return Filled::Pending };
    let limit = usize::try_from(limit).unwrap_or(usize::MAX);
    column.values = listed.into_iter().take(limit).map(|variable| variable.value).collect();
    Filled::Done
}

pub struct TraceStream;

impl Command for TraceStream {
    type Params = TraceParams;
    type Output = StreamTrace;
    const ID: &'static str = "debug.stream.trace";
    const TITLE: &'static str = "Trace Current Stream Chain";
    const DESCRIPTION: &'static str = "The values a Java stream chain on the stopped line (or `path` and `line`) \
         holds after each operation: the chain is evaluated once per operation in the stopped frame, each prefix \
         collected with `toArray()`, then the terminal operation. Only a chain whose stream is made in the \
         statement (`list.stream()`, `Stream.of(…)`, `Arrays.stream(…)`, `IntStream.range(…)`…); refused with \
         `peek` and for an infinite stream without `limit`/`takeWhile`; `forEach` is not run. The lambdas run again \
         for each column. With `waitMs` 0 it answers `pending` with the columns known so far: call it again.";

    fn run(&self, session: &mut Session, params: TraceParams) -> Result<StreamTrace, CommandError> {
        let debug = find_session(session, params.session_id)?;
        debug.stopped_client()?;
        let frame = match params.frame_id {
            Some(id) => debug.frame(id).ok_or_else(|| CommandError::new(t!("debug-unknown-frame", id = id)))?,
            None => debug.selected_frame().ok_or_else(|| CommandError::new(t!("debug-no-frame")))?,
        };
        let path = match params.path.as_ref() {
            Some(path) => {
                session.state::<Workbench>().map_or_else(|| path.clone(), |workbench| workbench.resolve(path))
            }
            None => frame.path.clone().ok_or_else(|| CommandError::new(t!("debug-no-source")))?,
        };
        if path.extension().is_none_or(|extension| extension != "java") {
            return Err(CommandError::new(t!("debug-stream-java-only")));
        }
        let line = params.line.unwrap_or(frame.line) as usize;
        let (text, _) = crate::run::file_text(session, &path)?;
        let chain = chain_at(&text, line).map_err(refusal)?;
        let steps = trace_steps(&chain).map_err(refusal)?;
        let limit = params.limit.unwrap_or(LIMIT_DEFAULT).clamp(1, LIMIT_MAX);
        let soon = params.wait_ms.map(|wait| Duration::from_millis(wait.min(WAIT_LIMIT)));
        let wait = soon.unwrap_or(REQUEST_TIMEOUT);
        let mut trace = StreamTrace { chain: chain.text.clone(), columns: Vec::new(), pending: false };
        for step in &steps {
            let mut column = TraceColumn {
                label: step.label.clone(),
                kind: kind_name(&step.kind).to_owned(),
                expression: step.expression.clone(),
                ..TraceColumn::default()
            };
            if !trace.pending
                && matches!(fill(session, &debug, frame.id, step, &mut column, limit, wait), Filled::Pending)
            {
                if soon.is_none() {
                    column.error = Some(t!("debug-error-timeout"));
                } else {
                    trace.pending = true;
                    column.pending = true;
                }
            } else if trace.pending && step.expression.is_some() {
                column.pending = true;
            }
            trace.columns.push(column);
        }
        Ok(trace)
    }
}
