use std::sync::Arc;
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};

use super::find_session;
use super::session::{DebugSession, Switch, ViewGate};

pub const COLLECTION_VIEWS: &str = "debugger.collectionViews";

pub fn shows_raw(session: &Session) -> bool {
    !session.settings().get_or(COLLECTION_VIEWS, true)
}

fn send_switch(
    session: &mut Session,
    debug: &DebugSession,
    command: &str,
    arguments: Vec<serde_json::Value>,
    server: Option<&str>,
) -> Option<Switch> {
    match server {
        Some(server) => crate::language::with_languages(session, |languages, _| {
            let client = languages.client_of(server)?;
            let request = serde_json::json!({ "command": command, "arguments": arguments });
            client.request("workspace/executeCommand", request).ok().map(Switch::Server)
        }),
        None => {
            let client = debug.client().ok()?;
            let arguments = arguments.into_iter().next().unwrap_or(serde_json::Value::Null);
            client.request(command, arguments).ok().map(Switch::Adapter)
        }
    }
}

pub(crate) fn view_ready(session: &mut Session, debug: &Arc<DebugSession>, raw: bool) -> bool {
    match debug.view_gate(raw) {
        ViewGate::Ready => true,
        ViewGate::Waiting => false,
        ViewGate::Switch { command, arguments, server } => {
            let switch = send_switch(session, debug, &command, arguments, server.as_deref());
            let sent = switch.is_some();
            debug.switching(raw, switch);
            !sent
        }
    }
}

pub(crate) fn wait_for_view(
    session: &mut Session,
    debug: &Arc<DebugSession>,
    raw: bool,
    wait: Duration,
) -> Result<bool, CommandError> {
    let deadline = Instant::now() + wait;
    loop {
        if view_ready(session, debug, raw) {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            return Ok(false);
        }
        debug.stopped_client()?;
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(crate) fn resolve_lazy(
    debug: &DebugSession,
    variables: Vec<zen_dap::Variable>,
    raw: bool,
    wait: Duration,
) -> Result<Option<Vec<zen_dap::Variable>>, CommandError> {
    if !wait.is_zero() {
        resolve_lazy(debug, variables.clone(), raw, Duration::ZERO)?;
    }
    let mut resolved = Vec::with_capacity(variables.len());
    let mut pending = false;
    for variable in variables {
        let lazy =
            variable.presentation_hint.as_ref().is_some_and(|hint| hint.lazy) && variable.variables_reference > 0;
        if !lazy {
            resolved.push(variable);
            continue;
        }
        match debug.variables_soon(variable.variables_reference, None, None, raw, wait) {
            Ok(Some(inside)) => match inside.into_iter().next() {
                Some(value) => resolved.push(zen_dap::Variable {
                    value: value.value,
                    variables_reference: value.variables_reference,
                    indexed_variables: value.indexed_variables,
                    named_variables: value.named_variables,
                    presentation_hint: value.presentation_hint,
                    type_: variable.type_.or(value.type_).filter(|type_| !type_.is_empty()),
                    ..variable
                }),
                None => resolved.push(variable),
            },
            Ok(None) => pending = true,
            Err(_) => resolved.push(variable),
        }
    }
    Ok((!pending).then_some(resolved))
}

/// The parameters of `debug.variables.raw`.
#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct RawParams {
    /// Whether collections, maps and arrays show their fields instead of their elements.
    pub raw: bool,
}

/// What `debug.variables.raw` answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RawAnswer {
    /// Whether the raw fields are shown now.
    pub raw: bool,
    /// Whether the current session's debugger can show collections by their elements.
    pub supported: bool,
}

pub struct ShowRawFields;

impl Command for ShowRawFields {
    type Params = RawParams;
    type Output = RawAnswer;
    const ID: &'static str = "debug.variables.raw";
    const TITLE: &'static str = "Show Raw Fields";
    const DESCRIPTION: &'static str = "Shows collections, maps and arrays in Variables, watches and Evaluate by their \
         fields (`raw: true`) or by their elements, map entries as key → value (`raw: false`, the default; Settings → \
         Debugger → Data Views → Enable alternative view for collections). One row's raw fields: \
         debug.variables.list with `raw`.";

    fn run(&self, session: &mut Session, params: RawParams) -> Result<RawAnswer, CommandError> {
        session
            .settings_mut()
            .set(COLLECTION_VIEWS, !params.raw)
            .map_err(|error| CommandError::new(error.to_string()))?;
        let supported = find_session(session, None).is_ok_and(|debug| debug.has_views());
        for debug in session.state::<super::Debugger>().map(super::Debugger::sessions).unwrap_or_default() {
            debug.forget_values();
        }
        Ok(RawAnswer { raw: params.raw, supported })
    }
}

fn entry_like(variable: &zen_dap::Variable) -> bool {
    let type_ = variable.type_.as_deref().unwrap_or_default();
    variable.variables_reference > 0
        && (type_.ends_with("Entry")
            || type_.ends_with("Node")
            || type_.ends_with("KeyValueHolder")
            || variable.value.contains("\":\""))
}

pub(crate) fn map_entries(
    debug: &DebugSession,
    variables: &[zen_dap::Variable],
    raw: bool,
    wait: Duration,
) -> Option<Vec<Option<(String, String)>>> {
    if raw {
        return Some(vec![None; variables.len()]);
    }
    if !wait.is_zero() {
        map_entries(debug, variables, raw, Duration::ZERO);
    }
    let mut entries = Vec::with_capacity(variables.len());
    let mut pending = false;
    for variable in variables {
        if !entry_like(variable) {
            entries.push(None);
            continue;
        }
        let inside = match debug.variables_soon(variable.variables_reference, None, None, raw, wait) {
            Ok(Some(inside)) => inside,
            Ok(None) => {
                pending = true;
                entries.push(None);
                continue;
            }
            Err(_) => {
                entries.push(None);
                continue;
            }
        };
        let [key, value] = inside.as_slice() else {
            entries.push(None);
            continue;
        };
        if key.name != "key" || value.name != "value" {
            entries.push(None);
            continue;
        }
        match resolve_lazy(debug, vec![key.clone(), value.clone()], raw, wait) {
            Ok(Some(resolved)) => entries.push(Some((resolved[0].value.clone(), resolved[1].value.clone()))),
            Ok(None) => {
                pending = true;
                entries.push(None);
            }
            Err(_) => entries.push(None),
        }
    }
    (!pending).then_some(entries)
}
