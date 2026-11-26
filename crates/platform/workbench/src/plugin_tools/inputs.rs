use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::Serialize;
use zen_i18n::t;
use zen_syntax::{ToolAction, ToolInput, ToolInputKind, ToolRow, localized};

pub type Inputs = BTreeMap<String, String>;

pub const COUNT: (u32, u32) = (0, 1000);

pub const PORT: (u32, u32) = (1024, 65535);

pub const REMOTE_PORT: (u32, u32) = (1, 65535);

/// A value the person types for an action, as `plugins.toolWindow.list` answers it.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InputInfo {
    /// Its id, as the action's `${input.<id>}` names it.
    pub id: String,
    /// Its label.
    pub title: String,
    /// `count` (0–1000), `port` (1024–65535) or `forward` (`LOCAL:REMOTE`, a local port 1024–65535 and a remote one 1–65535).
    pub kind: String,
    /// The smallest value (the local port's, for `forward`).
    pub min: u32,
    /// The largest value.
    pub max: u32,
    /// What the field starts with, if anything.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<u32>,
    /// The row column whose first number the field starts with, when it is in range.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub default_from: String,
}

pub fn kind_name(kind: ToolInputKind) -> &'static str {
    match kind {
        ToolInputKind::Count => "count",
        ToolInputKind::Port => "port",
        ToolInputKind::Forward => "forward",
    }
}

fn bounds(kind: ToolInputKind) -> (u32, u32) {
    match kind {
        ToolInputKind::Count => COUNT,
        ToolInputKind::Port | ToolInputKind::Forward => PORT,
    }
}

pub fn info(input: &ToolInput, language: &str) -> InputInfo {
    let (min, max) = bounds(input.kind);
    InputInfo {
        id: input.id.clone(),
        title: localized(&input.title, language),
        kind: kind_name(input.kind).to_owned(),
        min,
        max,
        default: input.default,
        default_from: input.default_from.clone(),
    }
}

fn number(text: &str, (min, max): (u32, u32)) -> Option<u32> {
    let plain = !text.is_empty() && text.len() <= 5 && text.bytes().all(|byte| byte.is_ascii_digit());
    if !plain || (text.len() > 1 && text.starts_with('0')) {
        return None;
    }
    text.parse::<u32>().ok().filter(|value| (min..=max).contains(value))
}

pub fn checked(kind: ToolInputKind, text: &str) -> Option<String> {
    match kind {
        ToolInputKind::Count | ToolInputKind::Port => number(text, bounds(kind)).map(|value| value.to_string()),
        ToolInputKind::Forward => {
            let (local, remote) = text.split_once(':')?;
            let (local, remote) = (number(local, PORT)?, number(remote, REMOTE_PORT)?);
            Some(format!("{local}:{remote}"))
        }
    }
}

pub fn first_number(text: &str, range: (u32, u32)) -> Option<u32> {
    let digits: String =
        text.chars().skip_while(|c| !c.is_ascii_digit()).take_while(char::is_ascii_digit).take(6).collect();
    number(&digits, range)
}

pub fn starting_value(input: &ToolInput, columns: &[String], row: Option<&ToolRow>) -> Option<String> {
    let from = (!input.default_from.is_empty())
        .then(|| columns.iter().position(|column| column == &input.default_from))
        .flatten()
        .and_then(|index| row?.columns.get(index));
    match input.kind {
        ToolInputKind::Forward => {
            let remote = from.and_then(|text| first_number(text, REMOTE_PORT));
            let local =
                input.default.filter(|port| (PORT.0..=PORT.1).contains(port)).or(remote.filter(|port| *port >= PORT.0));
            match (local, remote) {
                (Some(local), Some(remote)) => Some(format!("{local}:{remote}")),
                (None, Some(remote)) => Some(format!(":{remote}")),
                _ => None,
            }
        }
        kind => from
            .and_then(|text| first_number(text, bounds(kind)))
            .or(input.default.filter(|value| number(&value.to_string(), bounds(kind)).is_some()))
            .map(|value| value.to_string()),
    }
}

pub fn values(action: &ToolAction, given: &Inputs) -> Result<Inputs, String> {
    if let Some(unknown) = given.keys().find(|id| !action.inputs.iter().any(|input| &input.id == *id)) {
        return Err(t!("workbench-tool-input-unknown", input = unknown.as_str()));
    }
    let language = zen_i18n::language();
    action
        .inputs
        .iter()
        .map(|input| {
            let title = localized(&input.title, &language);
            let text = given.get(&input.id).map(String::as_str).unwrap_or_default();
            let value = checked(input.kind, text.trim()).ok_or_else(|| {
                let (min, max) = bounds(input.kind);
                match input.kind {
                    ToolInputKind::Forward => t!("workbench-tool-input-forward", input = title.as_str()),
                    _ => t!("workbench-tool-input-range", input = title.as_str(), min = min, max = max),
                }
            })?;
            Ok((input.id.clone(), value))
        })
        .collect()
}

pub fn fill(
    args: Vec<String>,
    declared: &[String],
    action: &ToolAction,
    inputs: &Inputs,
) -> Result<Vec<String>, String> {
    declared
        .iter()
        .zip(args)
        .map(|(arg, filled)| {
            let Some(name) = arg.strip_prefix("${input.").and_then(|rest| rest.strip_suffix('}')) else {
                return if arg.contains("${input.") {
                    Err(t!("workbench-tool-bad-argument", arg = arg.as_str()))
                } else {
                    Ok(filled)
                };
            };
            if !action.inputs.iter().any(|input| input.id == name) {
                return Err(t!("workbench-tool-bad-argument", arg = arg.as_str()));
            }
            inputs.get(name).cloned().ok_or_else(|| t!("workbench-tool-bad-argument", arg = arg.as_str()))
        })
        .collect()
}

pub fn masked(args: &[String]) -> Vec<String> {
    args.iter()
        .map(|arg| if arg.starts_with("${input.") && arg.ends_with('}') { String::new() } else { arg.clone() })
        .collect()
}

pub fn in_question(text: String, inputs: &Inputs) -> String {
    inputs.iter().fold(text, |text, (id, value)| text.replace(&format!("${{input.{id}}}"), value))
}
