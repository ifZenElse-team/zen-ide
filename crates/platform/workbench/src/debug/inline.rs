use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Session};
use zen_i18n::t;
use zen_syntax::SymbolKind;

use super::{WAIT_LIMIT, find_session, session};
use crate::Workbench;
use crate::language::structure::{Symbol, resolve};

const LINES_BACK: usize = 200;
const LINES_WITHOUT_STRUCTURE: usize = 30;
const VALUE_CHARS: usize = 80;

/// The parameters of `debug.inlineValues.get`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct InlineValuesParams {
    /// The session; the current one when absent.
    pub session_id: Option<u64>,
    /// The file; values come only for the selected frame's file, which is the one when absent.
    pub path: Option<PathBuf>,
    /// How long to wait for the debugger, at most 10 000; 0 answers `pending` at once, then call again with the same parameters.
    pub wait_ms: Option<u64>,
}

/// A variable's value shown at the end of a line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InlineValue {
    pub name: String,
    /// The value on one line, cut after 80 characters.
    pub value: String,
    /// Whether it differs from the value at the previous stop in the same function.
    pub changed: bool,
}

/// The values shown at the end of one line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InlineLine {
    /// The line, from 0.
    pub line: u32,
    /// The variables named on the line, in the order they come.
    pub values: Vec<InlineValue>,
}

/// What `debug.inlineValues.get` answers.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct InlineValuesAnswer {
    /// The selected frame's file; absent when the frame has no source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    /// The lines of the frame's function up to the stopped line that name a variable of the frame.
    pub lines: Vec<InlineLine>,
    /// Whether the debugger has not answered within `waitMs`; ask again.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub pending: bool,
}

pub struct InlineValues;

impl Command for InlineValues {
    type Params = InlineValuesParams;
    type Output = InlineValuesAnswer;
    const ID: &'static str = "debug.inlineValues.get";
    const TITLE: &'static str = "Inline Values";
    const DESCRIPTION: &'static str = "The values the editor shows at the end of lines while the program is stopped \
         (Settings → Debugger → Data Views → Show values inline): the variables of the selected frame's scopes, \
         matched by name on the lines of its function up to the stopped line, each marked when it changed since \
         the previous stop.";

    fn run(&self, session: &mut Session, params: InlineValuesParams) -> Result<InlineValuesAnswer, CommandError> {
        let debug = find_session(session, params.session_id)?;
        debug.stopped_client()?;
        let frame = debug.selected_frame().ok_or_else(|| CommandError::new(t!("debug-no-frame")))?;
        let Some(path) = frame.path.clone() else { return Ok(InlineValuesAnswer::default()) };
        if let Some(wanted) = &params.path {
            let wanted =
                session.state::<Workbench>().map_or_else(|| wanted.clone(), |workbench| workbench.resolve(wanted));
            if wanted != path {
                return Ok(InlineValuesAnswer::default());
            }
        }
        let wait = params.wait_ms.map_or(session::REQUEST_TIMEOUT, |wait| Duration::from_millis(wait.min(WAIT_LIMIT)));
        let pending = || Ok(InlineValuesAnswer { path: Some(path.clone()), lines: Vec::new(), pending: true });
        let Some(scopes) = debug.scopes_soon(frame.id, wait)? else { return pending() };
        let mut values: HashMap<String, String> = HashMap::new();
        for scope in scopes.iter().filter(|scope| !scope.expensive && scope.variables_reference > 0) {
            let Some(variables) =
                debug.variables_soon(scope.variables_reference, None, None, super::views::shows_raw(session), wait)?
            else {
                return pending();
            };
            for variable in variables.into_iter().filter(|variable| is_identifier(&variable.name)) {
                values.entry(variable.name).or_insert_with(|| one_line(&variable.value));
            }
        }
        let text = document_text(session, &path);
        let stopped = frame.line as usize;
        let first = function_start(session, &path, &text, stopped)
            .unwrap_or_else(|| stopped.saturating_sub(LINES_WITHOUT_STRUCTURE))
            .max(stopped.saturating_sub(LINES_BACK));
        let before = debug.values_before(&frame.name, &values);
        let lines = text
            .split('\n')
            .enumerate()
            .skip(first)
            .take(stopped + 1 - first.min(stopped + 1))
            .filter_map(|(line, content)| {
                let values: Vec<InlineValue> = names_on(content)
                    .into_iter()
                    .filter_map(|name| {
                        let value = values.get(&name)?.clone();
                        let changed = before.get(&name).is_some_and(|old| *old != value);
                        Some(InlineValue { name, value, changed })
                    })
                    .collect();
                (!values.is_empty()).then(|| InlineLine { line: u32::try_from(line).unwrap_or(u32::MAX), values })
            })
            .collect();
        Ok(InlineValuesAnswer { path: Some(path), lines, pending: false })
    }
}

fn document_text(session: &Session, path: &Path) -> String {
    session
        .state::<Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .map(|document| document.text().to_owned())
        .or_else(|| std::fs::read_to_string(path).ok())
        .unwrap_or_default()
}

fn function_start(session: &mut Session, path: &Path, text: &str, line: usize) -> Option<usize> {
    let symbols = resolve(session, path, Duration::ZERO).symbols;
    let offset: usize = text.split('\n').take(line).map(|line| line.chars().count() + 1).sum();
    let start = innermost_function(&symbols, offset)?.start;
    Some(text.chars().take(start).filter(|c| *c == '\n').count())
}

fn innermost_function(symbols: &[Symbol], offset: usize) -> Option<&Symbol> {
    let around = symbols.iter().find(|symbol| symbol.start <= offset && offset < symbol.end)?;
    let inner = innermost_function(&around.children, offset);
    let own = matches!(around.kind, SymbolKind::Function | SymbolKind::Method).then_some(around);
    inner.or(own)
}

fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_') && chars.all(|c| c.is_alphanumeric() || c == '_')
}

fn one_line(value: &str) -> String {
    let flat: String = value.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
    if flat.chars().count() <= VALUE_CHARS {
        return flat;
    }
    let mut cut: String = flat.chars().take(VALUE_CHARS).collect();
    cut.push('…');
    cut
}

pub fn names_on(line: &str) -> Vec<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut names: Vec<String> = Vec::new();
    let mut quote: Option<char> = None;
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if let Some(open) = quote {
            if c == '\\' {
                index += 1;
            } else if c == open {
                quote = None;
            }
            index += 1;
            continue;
        }
        if c == '/' && chars.get(index + 1) == Some(&'/') {
            break;
        }
        if c == '"' || c == '\'' || c == '`' {
            quote = Some(c);
            index += 1;
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = index;
            while index < chars.len() && (chars[index].is_alphanumeric() || chars[index] == '_') {
                index += 1;
            }
            let member = start > 0 && chars[start - 1] == '.';
            let name: String = chars[start..index].iter().collect();
            if !member && !names.contains(&name) {
                names.push(name);
            }
            continue;
        }
        if c.is_ascii_digit() {
            while index < chars.len() && (chars[index].is_alphanumeric() || chars[index] == '_' || chars[index] == '.')
            {
                index += 1;
            }
            continue;
        }
        index += 1;
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_on_a_line_skip_members_strings_comments_and_repeats() {
        assert_eq!(names_on("\tsum := a + b.c + a // total"), ["sum", "a", "b"]);
        assert_eq!(names_on(r#"fmt.Println("x is", x, 'y', 1e3)"#), ["fmt", "x"]);
        assert_eq!(names_on(r#"s := "a \" b" + t"#), ["s", "t"]);
    }

    #[test]
    fn a_long_value_is_cut_on_one_line() {
        assert_eq!(one_line("a\nb"), "a b");
        let long = "x".repeat(100);
        assert_eq!(one_line(&long).chars().count(), VALUE_CHARS + 1);
        assert!(is_identifier("_x1") && !is_identifier("[0]") && !is_identifier("1a"));
    }
}
