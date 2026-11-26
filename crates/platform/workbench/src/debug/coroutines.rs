use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;

use super::session::REQUEST_TIMEOUT;
use super::{WAIT_LIMIT, find_session};
use crate::java::coroutines::{Coroutine, DUMP, FRAME_LIMIT, parse_dump};

/// The parameters of `debug.coroutines.list`.
#[derive(Clone, Copy, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct CoroutinesParams {
    /// The session; the current one when absent.
    pub session_id: Option<u64>,
    /// How long to wait for the debugger, at most 10 000; 0 answers `pending` at once, then call again with the same parameters.
    pub wait_ms: Option<u64>,
}

/// What `debug.coroutines.list` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoroutinesAnswer {
    /// The program's coroutines, at most 200.
    pub coroutines: Vec<Coroutine>,
    /// Whether the program has more coroutines than are listed.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub more: bool,
    /// The frame the list was read in.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame: Option<String>,
    /// Why there is no list: `noEvent` (the thread was stopped by Pause, not by a breakpoint or a step), `notJvm`, `notInstalled` (no coroutine debug agent in the program), `noLibrary` or `noFrame` (none of the first 8 frames can read it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
    /// That reason, for people.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// What the debugger said last, when no frame could read the list.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Whether the debugger has not answered within `waitMs`; ask again.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
}

fn unavailable(code: &str, detail: Option<String>) -> CoroutinesAnswer {
    let message = match code {
        "noEvent" => t!("debug-coroutines-no-event"),
        "notJvm" => t!("debug-coroutines-not-jvm"),
        "notInstalled" => t!("debug-coroutines-not-installed"),
        "noLibrary" => t!("debug-coroutines-no-library"),
        _ => t!("debug-coroutines-no-frame", count = FRAME_LIMIT),
    };
    CoroutinesAnswer {
        unavailable: Some(code.to_owned()),
        message: Some(message),
        detail,
        ..CoroutinesAnswer::default()
    }
}

fn is_jvm_source(path: &std::path::Path) -> bool {
    path.extension().is_some_and(|extension| extension == "java" || extension == "kt")
}

pub struct ListCoroutines;

impl Command for ListCoroutines {
    type Params = CoroutinesParams;
    type Output = CoroutinesAnswer;
    const ID: &'static str = "debug.coroutines.list";
    const TITLE: &'static str = "Coroutines";
    const DESCRIPTION: &'static str = "The Kotlin coroutines of a stopped Java or Kotlin program, as \
         kotlinx-coroutines-debug sees them: state, name, dispatcher, job and where each last ran. It evaluates one \
         fixed read-only dump in the first of the top 8 frames of the thread a breakpoint or a step stopped that can \
         run it, and needs the coroutine debug agent (Track coroutines). With `waitMs` 0 it answers `pending` at once \
         while the debugger works: call it again with the same parameters.";

    fn run(&self, session: &mut Session, params: CoroutinesParams) -> Result<CoroutinesAnswer, CommandError> {
        let debug = find_session(session, params.session_id)?;
        debug.stopped_client()?;
        let Some(thread) = debug.stopped_thread().filter(|_| debug.stop_reason().as_deref() != Some("pause")) else {
            return Ok(unavailable("noEvent", None));
        };
        let frames = debug.frames(thread, 0, FRAME_LIMIT)?;
        if !frames.iter().any(|frame| frame.path.as_deref().is_some_and(is_jvm_source)) {
            return Ok(unavailable("notJvm", None));
        }
        let wait = params.wait_ms.map_or(REQUEST_TIMEOUT, |wait| Duration::from_millis(wait.min(WAIT_LIMIT)));
        let mut no_library = false;
        let mut detail = None;
        for frame in frames.iter().filter(|frame| frame.hint.as_deref() != Some("label")) {
            let arguments = serde_json::json!({ "expression": DUMP, "frameId": frame.id, "context": "watch" });
            match debug.ask_once("evaluate", arguments, wait) {
                Ok(None) => return Ok(CoroutinesAnswer { pending: true, ..CoroutinesAnswer::default() }),
                Ok(Some(body)) => {
                    let shown = body.get("result").and_then(serde_json::Value::as_str).unwrap_or_default();
                    if !shown.starts_with('"') {
                        if shown.contains("not installed") {
                            return Ok(unavailable("notInstalled", Some(shown.to_owned())));
                        }
                        detail = Some(shown.to_owned());
                        continue;
                    }
                    let (coroutines, more) = parse_dump(shown);
                    return Ok(CoroutinesAnswer {
                        coroutines,
                        more,
                        frame: Some(frame.name.clone()),
                        ..CoroutinesAnswer::default()
                    });
                }
                Err(message) if message.contains("not installed") => {
                    return Ok(unavailable("notInstalled", Some(message)));
                }
                Err(message) => {
                    no_library |= message.contains("kotlinx") && message.contains("cannot be resolved");
                    detail = Some(message);
                }
            }
        }
        Ok(unavailable(if no_library { "noLibrary" } else { "noFrame" }, detail))
    }
}
