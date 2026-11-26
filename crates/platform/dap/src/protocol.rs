use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Message {
    Request(Request),
    Response(Response),
    Event(Event),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub seq: i64,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub seq: i64,
    #[serde(rename = "request_seq")]
    pub request_seq: i64,
    pub success: bool,
    pub command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub seq: i64,
    pub event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ack;

impl<'de> Deserialize<'de> for Ack {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        serde::de::IgnoredAny::deserialize(deserializer).map(|_| Self)
    }
}

/// The arguments of `initialize`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InitializeArguments {
    /// The client's id.
    #[serde(rename = "clientID", default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// The client's name, for people.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_name: Option<String>,
    /// The id of the adapter the client talks to, e.g. `go`.
    #[serde(rename = "adapterID")]
    pub adapter_id: String,
    /// The client's language, e.g. `en`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    /// Whether lines count from 1.
    #[serde(default)]
    pub lines_start_at1: bool,
    /// Whether columns count from 1.
    #[serde(default)]
    pub columns_start_at1: bool,
    /// How paths are written: `path` or `uri`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_format: Option<String>,
    /// Whether the client shows the types of variables.
    #[serde(default)]
    pub supports_variable_type: bool,
    /// Whether the client can run the debuggee in a terminal for the adapter.
    #[serde(default)]
    pub supports_run_in_terminal_request: bool,
}

impl InitializeArguments {
    pub fn zen(adapter_id: &str) -> Self {
        Self {
            client_id: Some("zen".to_owned()),
            client_name: Some("Zen IDE".to_owned()),
            adapter_id: adapter_id.to_owned(),
            locale: None,
            lines_start_at1: true,
            columns_start_at1: true,
            path_format: Some("path".to_owned()),
            supports_variable_type: true,
            supports_run_in_terminal_request: false,
        }
    }
}

/// What the adapter can do, as far as the client reads it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct Capabilities {
    /// Whether the adapter takes `configurationDone`.
    pub supports_configuration_done_request: bool,
    /// Whether breakpoints can have conditions.
    pub supports_conditional_breakpoints: bool,
    /// Whether breakpoints can have hit conditions.
    pub supports_hit_conditional_breakpoints: bool,
    /// Whether breakpoints can be log points.
    pub supports_log_points: bool,
    /// Whether breakpoints on functions can be set.
    pub supports_function_breakpoints: bool,
    /// Whether `evaluate` takes the `hover` context.
    pub supports_evaluate_for_hovers: bool,
    /// Whether variables can be changed.
    pub supports_set_variable: bool,
    /// Whether an expression, such as a variable's `evaluateName`, can be assigned.
    pub supports_set_expression: bool,
    /// Whether the adapter takes `terminate`.
    pub supports_terminate_request: bool,
    /// Whether stack frames can be loaded in pages.
    pub supports_delayed_stack_trace_loading: bool,
    /// Whether `disconnect` takes `terminateDebuggee`.
    pub support_terminate_debuggee: bool,
    /// Whether data breakpoints (watchpoints) can be set.
    pub supports_data_breakpoints: bool,
    /// Whether exception filters take conditions.
    pub supports_exception_filter_options: bool,
    /// Whether `continue` and the steps can move one thread alone.
    pub supports_single_thread_execution_requests: bool,
    /// Whether `stepInTargets` lists the calls a step can go into.
    pub supports_step_in_targets_request: bool,
    /// Whether `restartFrame` runs a frame again from its start.
    pub supports_restart_frame: bool,
    /// The exception filters `setExceptionBreakpoints` takes.
    pub exception_breakpoint_filters: Vec<ExceptionBreakpointsFilter>,
}

/// An exception filter the adapter offers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionBreakpointsFilter {
    /// The filter's id.
    pub filter: String,
    /// The filter's name, for people.
    pub label: String,
    /// Whether it is on unless turned off.
    #[serde(default)]
    pub default: bool,
    /// What it catches, for people.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether it takes a condition.
    #[serde(default)]
    pub supports_condition: bool,
    /// What its condition means, for people.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition_description: Option<String>,
}

/// A source file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    /// The file's short name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The file's path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// A reference to source that only the adapter has; above 0 when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_reference: Option<i64>,
}

impl Source {
    pub fn path(path: &str) -> Self {
        Self { path: Some(path.to_owned()), ..Self::default() }
    }
}

/// A breakpoint the client asks for in a file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SourceBreakpoint {
    /// The line, from 1.
    pub line: i64,
    /// The column, from 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<i64>,
    /// An expression that must be true for the breakpoint to stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// How many hits it takes to stop, as the adapter reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
    /// A message logged instead of stopping, with `{expression}` parts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_message: Option<String>,
}

/// The arguments of `setBreakpoints`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetBreakpointsArguments {
    /// The file.
    pub source: Source,
    /// Every breakpoint of the file; the ones not listed are removed.
    pub breakpoints: Vec<SourceBreakpoint>,
    /// Whether the file has changed since it was built.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_modified: Option<bool>,
}

/// A breakpoint as the adapter set it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Breakpoint {
    /// The adapter's id of the breakpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    /// Whether it could be set.
    pub verified: bool,
    /// Why it could not be set, or what the adapter says about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Its file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    /// The line it is on, from 1; it may differ from the line asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<i64>,
    /// The column it is on, from 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<i64>,
}

/// The body of the `setBreakpoints` response.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetBreakpointsResponse {
    /// The breakpoints, in the order they were asked for.
    #[serde(default)]
    pub breakpoints: Vec<Breakpoint>,
}

/// The arguments of `setExceptionBreakpoints`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetExceptionBreakpointsArguments {
    /// The ids of the exception filters to turn on.
    pub filters: Vec<String>,
    /// Filters turned on with a condition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_options: Option<Vec<ExceptionFilterOptions>>,
}

/// An exception filter turned on with a condition.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExceptionFilterOptions {
    pub filter_id: String,
    /// An expression that must be true for it to stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BreakpointsResponse {
    /// The breakpoints, in the order they were asked for.
    #[serde(default)]
    pub breakpoints: Vec<Breakpoint>,
}

/// A breakpoint on a function, by its name.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FunctionBreakpoint {
    pub name: String,
    /// An expression that must be true for it to stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// How many hits it takes to stop, as the adapter reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetFunctionBreakpointsArguments {
    /// Every function breakpoint; the ones not listed are removed.
    pub breakpoints: Vec<FunctionBreakpoint>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataBreakpointInfoArguments {
    /// The container of the variable; absent for an expression.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variables_reference: Option<i64>,
    /// The variable's name, or an expression.
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<i64>,
}

/// Whether a data breakpoint can watch a variable, and how.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataBreakpointInfoResponse {
    /// The id to watch; absent when it cannot be watched.
    #[serde(default)]
    pub data_id: Option<String>,
    /// What would be watched, or why it cannot be.
    #[serde(default)]
    pub description: String,
    /// The accesses it can stop on: `read`, `write`, `readWrite`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_types: Option<Vec<String>>,
    /// Whether the id stays valid in later sessions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_persist: Option<bool>,
}

/// A breakpoint that stops when data is read or written.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DataBreakpoint {
    pub data_id: String,
    /// The access it stops on: `read`, `write` or `readWrite`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_type: Option<String>,
    /// An expression that must be true for it to stop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition: Option<String>,
    /// How many hits it takes to stop, as the adapter reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_condition: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetDataBreakpointsArguments {
    /// Every data breakpoint; the ones not listed are removed.
    pub breakpoints: Vec<DataBreakpoint>,
}

/// A thread of the debuggee; a goroutine for Delve.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    /// The thread's id.
    pub id: i64,
    /// The thread's name.
    pub name: String,
}

/// The body of the `threads` response.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThreadsResponse {
    /// Every thread.
    #[serde(default)]
    pub threads: Vec<Thread>,
}

/// The arguments of `stackTrace`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StackTraceArguments {
    /// The thread.
    pub thread_id: i64,
    /// The first frame to return, from 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_frame: Option<i64>,
    /// How many frames to return; all when absent or 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub levels: Option<i64>,
}

/// A frame of a thread's stack.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StackFrame {
    /// The frame's id, for `scopes` and `evaluate`.
    pub id: i64,
    /// The function's name.
    pub name: String,
    /// The file it is in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
    /// The line, from 1; 0 when there is no source.
    pub line: i64,
    /// The column, from 1.
    pub column: i64,
    /// Whether `restartFrame` can run it again; true when absent and the adapter can restart frames.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub can_restart: Option<bool>,
    /// How it is shown: `normal`, `label` (a separator, such as the start of an async part) or `subtle`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_hint: Option<String>,
}

/// The body of the `stackTrace` response.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StackTraceResponse {
    /// The frames, the innermost first.
    #[serde(default)]
    pub stack_frames: Vec<StackFrame>,
    /// How many frames the stack has in all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_frames: Option<i64>,
}

/// The arguments of `scopes`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScopesArguments {
    /// The frame.
    pub frame_id: i64,
}

/// A group of variables of a frame, such as its locals.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Scope {
    /// The scope's name, e.g. `Locals`.
    pub name: String,
    /// The reference to pass to `variables`.
    pub variables_reference: i64,
    /// Whether reading it is slow.
    #[serde(default)]
    pub expensive: bool,
}

/// The body of the `scopes` response.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScopesResponse {
    /// The frame's scopes.
    #[serde(default)]
    pub scopes: Vec<Scope>,
}

/// The arguments of `variables`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VariablesArguments {
    /// A scope's or a variable's reference.
    pub variables_reference: i64,
    /// The first child to return, from 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<i64>,
    /// How many children to return; all when absent or 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<i64>,
}

/// A variable, or a child of one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Variable {
    /// The variable's name.
    pub name: String,
    /// Its value, as text.
    pub value: String,
    /// Its type.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    /// The reference to its children; 0 when it has none.
    #[serde(default)]
    pub variables_reference: i64,
    /// How many children it has by index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_variables: Option<i64>,
    /// How many children it has by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named_variables: Option<i64>,
    /// An expression that evaluates to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evaluate_name: Option<String>,
    /// How to show it; with `lazy` its value is the one child its `variablesReference` lists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_hint: Option<VariablePresentationHint>,
}

/// How a variable is shown.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct VariablePresentationHint {
    /// What it is, such as `property`, `method`, `class`, `data` or `virtual`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Attributes such as `readOnly`, `rawString` or `hasSideEffects`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<String>,
    /// `public`, `private`, `protected`, `internal` or `final`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,
    /// Whether the value is fetched by listing the variable's children, which holds it as the only child.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub lazy: bool,
}

/// The body of the `variables` response.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct VariablesResponse {
    /// The children.
    #[serde(default)]
    pub variables: Vec<Variable>,
}

/// The arguments of `evaluate`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateArguments {
    /// The expression.
    pub expression: String,
    /// The frame it is evaluated in; the global scope when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<i64>,
    /// Where it comes from: `watch`, `repl`, `hover` or `clipboard`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

/// The body of the `evaluate` response.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateResponse {
    /// The value, as text.
    pub result: String,
    /// Its type.
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    /// The reference to its children; 0 when it has none.
    #[serde(default)]
    pub variables_reference: i64,
    /// How many children it has by index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_variables: Option<i64>,
    /// How many children it has by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named_variables: Option<i64>,
    /// How to show it; with `lazy` its value is the one child its `variablesReference` lists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation_hint: Option<VariablePresentationHint>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetVariableArguments {
    /// The container of the variable: a scope or the parent variable.
    pub variables_reference: i64,
    pub name: String,
    /// The new value, in the program's language.
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetExpressionArguments {
    /// The expression to assign to, such as a variable's `evaluateName`.
    pub expression: String,
    /// The new value, in the program's language.
    pub value: String,
    /// The frame it is assigned in; the global scope when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<i64>,
}

/// The answer of `setVariable` and `setExpression`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SetValueResponse {
    /// The value now, as text.
    pub value: String,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    /// The reference to its children; 0 when it has none.
    #[serde(default)]
    pub variables_reference: i64,
    /// How many children it has by index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indexed_variables: Option<i64>,
    /// How many children it has by name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub named_variables: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FrameArguments {
    pub frame_id: i64,
}

/// A call on the stopped line that a step can go into.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StepInTarget {
    /// The target's id, for `stepIn`.
    pub id: i64,
    /// The call's name, for people.
    pub label: String,
    /// The line of the call, from 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<i64>,
    /// The column the call starts at, from 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<i64>,
    /// The line the call ends on, from 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<i64>,
    /// The column the call ends before, from 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_column: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StepInTargetsResponse {
    #[serde(default)]
    pub targets: Vec<StepInTarget>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StepInArguments {
    pub thread_id: i64,
    /// The call to step into, from `stepInTargets`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<i64>,
}

/// The arguments of `continue`, `next`, `stepIn`, `stepOut` and `pause`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThreadArguments {
    /// The thread.
    pub thread_id: i64,
    /// Whether only this thread moves.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub single_thread: Option<bool>,
}

/// The body of the `continue` response.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContinueResponse {
    /// Whether every thread runs again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all_threads_continued: Option<bool>,
}

/// The arguments of `disconnect`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DisconnectArguments {
    /// Whether a session starts again after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<bool>,
    /// Whether the debuggee is ended too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminate_debuggee: Option<bool>,
}

/// The arguments of `terminate`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TerminateArguments {
    /// Whether a session starts again after it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<bool>,
}

/// The body of the `stopped` event.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StoppedEvent {
    /// Why it stopped: `step`, `breakpoint`, `exception`, `pause`, `entry` and others.
    pub reason: String,
    /// The reason, for people.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The thread that stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<i64>,
    /// More text about the stop, such as an exception's message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Whether every thread stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all_threads_stopped: Option<bool>,
    /// The ids of the breakpoints that were hit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_breakpoint_ids: Option<Vec<i64>>,
}

/// The body of the `continued` event.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContinuedEvent {
    /// The thread that runs again.
    pub thread_id: i64,
    /// Whether every thread runs again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub all_threads_continued: Option<bool>,
}

/// The body of the `output` event.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OutputEvent {
    /// Where it comes from: `console`, `stdout`, `stderr`, `important` and others; `console` when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// The text, line breaks included.
    pub output: String,
}

/// The body of the `exited` event.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExitedEvent {
    /// The debuggee's exit code.
    pub exit_code: i64,
}

/// The body of the `breakpoint` event.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BreakpointEvent {
    /// What happened: `changed`, `new` or `removed`.
    pub reason: String,
    /// The breakpoint, with its id.
    pub breakpoint: Breakpoint,
}

/// The body of the `thread` event.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThreadEvent {
    /// What happened: `started` or `exited`.
    pub reason: String,
    /// The thread.
    pub thread_id: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DebugEvent {
    Initialized,
    Stopped(StoppedEvent),
    Continued(ContinuedEvent),
    Output(OutputEvent),
    Terminated,
    Exited(ExitedEvent),
    Breakpoint(BreakpointEvent),
    Thread(ThreadEvent),
    Other { event: String, body: Value },
}

impl DebugEvent {
    pub fn from_event(event: &str, body: Value) -> Self {
        fn read<T: serde::de::DeserializeOwned>(body: &Value) -> Option<T> {
            serde_json::from_value(body.clone()).ok()
        }
        let typed = match event {
            "initialized" => Some(Self::Initialized),
            "terminated" => Some(Self::Terminated),
            "stopped" => read(&body).map(Self::Stopped),
            "continued" => read(&body).map(Self::Continued),
            "output" => read(&body).map(Self::Output),
            "exited" => read(&body).map(Self::Exited),
            "breakpoint" => read(&body).map(Self::Breakpoint),
            "thread" => read(&body).map(Self::Thread),
            _ => None,
        };
        typed.unwrap_or(Self::Other { event: event.to_owned(), body })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn messages_are_told_apart_by_their_type() {
        let request: Message =
            serde_json::from_value(json!({ "seq": 3, "type": "request", "command": "runInTerminal", "arguments": {} }))
                .unwrap();
        assert!(matches!(request, Message::Request(Request { seq: 3, ref command, .. }) if command == "runInTerminal"));
        let response = Message::Response(Response {
            seq: 1,
            request_seq: 2,
            success: true,
            command: "threads".to_owned(),
            message: None,
            body: None,
        });
        assert_eq!(
            serde_json::to_value(&response).unwrap(),
            json!({ "seq": 1, "type": "response", "request_seq": 2, "success": true, "command": "threads" })
        );
    }

    #[test]
    fn initialize_arguments_use_the_protocol_names() {
        let value = serde_json::to_value(InitializeArguments::zen("go")).unwrap();
        assert_eq!(value["clientID"], "zen");
        assert_eq!(value["adapterID"], "go");
        assert_eq!(value["linesStartAt1"], true);
        assert_eq!(value["columnsStartAt1"], true);
        assert_eq!(value["pathFormat"], "path");
        assert_eq!(value["supportsVariableType"], true);
    }

    #[test]
    fn events_are_read_into_their_types() {
        let stopped = DebugEvent::from_event(
            "stopped",
            json!({ "reason": "breakpoint", "threadId": 1, "allThreadsStopped": true, "hitBreakpointIds": [4] }),
        );
        assert_eq!(
            stopped,
            DebugEvent::Stopped(StoppedEvent {
                reason: "breakpoint".to_owned(),
                thread_id: Some(1),
                all_threads_stopped: Some(true),
                hit_breakpoint_ids: Some(vec![4]),
                ..StoppedEvent::default()
            })
        );
        assert_eq!(
            DebugEvent::from_event("exited", json!({ "exitCode": 3 })),
            DebugEvent::Exited(ExitedEvent { exit_code: 3 })
        );
        assert!(matches!(DebugEvent::from_event("module", json!({ "reason": "new" })), DebugEvent::Other { .. }));
        assert!(matches!(DebugEvent::from_event("stopped", json!(7)), DebugEvent::Other { .. }));
        let variable: Variable =
            serde_json::from_value(json!({ "name": "x", "value": "1", "type": "int", "variablesReference": 0 }))
                .unwrap();
        assert_eq!(variable.type_.as_deref(), Some("int"));
    }
}
