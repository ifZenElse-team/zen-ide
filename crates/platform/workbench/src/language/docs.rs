use std::path::PathBuf;
use std::time::Duration;

use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Value, json};
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};

use super::{Languages, LookupKind, LookupParams, WaitParams, char_offset, with_languages};
use crate::Workbench;

/// The quick documentation of a symbol.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DocumentationState {
    /// Whether the popup is open.
    pub open: bool,
    /// Whether the server has not answered yet.
    pub pending: bool,
    /// The documentation, Markdown; `None` when there is none.
    pub markdown: Option<String>,
    /// The characters of the symbol it is about in the file, when the server says.
    pub range: Option<[usize; 2]>,
    /// Why there is nothing, in words for the user.
    pub error: Option<String>,
}

/// A signature of the called function.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Signature {
    /// The signature as shown, e.g. `func Println(a ...any) (n int, err error)`.
    pub label: String,
    /// Each parameter's characters in the label.
    pub parameters: Vec<[usize; 2]>,
    /// The index of the parameter at the caret in this signature.
    pub active_parameter: Option<usize>,
    /// Its documentation, Markdown or plain text.
    pub documentation: Option<String>,
}

/// The parameter info of a call.
#[derive(Clone, Debug, Default, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ParameterInfoState {
    /// Whether the popup is open.
    pub open: bool,
    /// Whether the server has not answered yet.
    pub pending: bool,
    /// The signatures that fit the call.
    pub signatures: Vec<Signature>,
    /// The index of the signature that fits best.
    pub active: Option<usize>,
    /// The index of the parameter at the caret in the active signature.
    pub active_parameter: Option<usize>,
    /// Why there is nothing, in words for the user.
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Popups {
    documentation: Option<ShownDocumentation>,
    parameter_info: Option<ShownParameterInfo>,
}

struct ShownDocumentation {
    path: PathBuf,
    offset: usize,
    state: DocumentationState,
}

struct ShownParameterInfo {
    path: PathBuf,
    follows_caret: bool,
    caret: usize,
    version: u64,
    help: Value,
    state: ParameterInfoState,
}

impl Popups {
    pub fn documentation_open(&self) -> bool {
        self.documentation.is_some()
    }

    pub fn parameter_info_open(&self) -> bool {
        self.parameter_info.is_some()
    }
}

fn with_popups<T>(session: &mut Session, f: impl FnOnce(&mut Popups, &mut Languages, &mut Workbench) -> T) -> T {
    let mut popups = std::mem::take(session.state_mut::<Popups>());
    let result = with_languages(session, |languages, workbench| f(&mut popups, languages, workbench));
    session.set_state(popups);
    result
}

fn caret_of(workbench: &Workbench, path: &std::path::Path) -> Option<(usize, u64)> {
    workbench
        .documents()
        .iter()
        .find(|document| document.path() == path)
        .map(|document| (document.selections().primary().head, document.version()))
}

fn is_active(workbench: &Workbench, path: &std::path::Path) -> bool {
    workbench.active_document().is_some_and(|document| document.path() == path)
}

pub fn hover_markdown(contents: &Value) -> Option<String> {
    let markdown = match contents {
        Value::String(text) => text.clone(),
        Value::Array(list) => list.iter().filter_map(hover_markdown).collect::<Vec<_>>().join("\n\n"),
        Value::Object(object) if object.contains_key("kind") => {
            let value = object.get("value").and_then(Value::as_str).unwrap_or_default();
            if object.get("kind").and_then(Value::as_str) == Some("plaintext") {
                plain_to_markdown(value)
            } else {
                value.to_owned()
            }
        }
        Value::Object(object) => {
            let value = object.get("value").and_then(Value::as_str).unwrap_or_default();
            let language = object.get("language").and_then(Value::as_str).unwrap_or_default();
            code_fence(language, value)
        }
        _ => String::new(),
    };
    Some(markdown).filter(|markdown| !markdown.trim().is_empty())
}

pub fn plain_to_markdown(text: &str) -> String {
    let mut out = String::new();
    let lines: Vec<&str> = text.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let body = line.trim_start_matches([' ', '\t']);
        for c in line[..line.len() - body.len()].chars() {
            out.push_str(if c == '\t' { "\u{a0}\u{a0}\u{a0}\u{a0}" } else { "\u{a0}" });
        }
        for c in body.chars() {
            if c.is_ascii_punctuation() {
                out.push('\\');
            }
            out.push(c);
        }
        let next_has_text = lines.get(index + 1).is_some_and(|next| !next.trim().is_empty());
        if !body.trim().is_empty() && next_has_text {
            out.push('\\');
        }
        if index + 1 < lines.len() {
            out.push('\n');
        }
    }
    out
}

fn code_fence(language: &str, code: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in code.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    let fence = "`".repeat((longest + 1).max(3));
    format!("{fence}{language}\n{}\n{fence}", code.trim_end_matches('\n'))
}

pub fn hover(text: &str, answer: &Value) -> (Option<String>, Option<[usize; 2]>) {
    if answer.is_null() {
        return (None, None);
    }
    let position = |at: &Value| -> Option<lsp_types::Position> {
        Some(lsp_types::Position { line: at["line"].as_u64()? as u32, character: at["character"].as_u64()? as u32 })
    };
    let range = (|| {
        let start = zen_lsp::position::offset(text, position(&answer["range"]["start"])?)?;
        let end = zen_lsp::position::offset(text, position(&answer["range"]["end"])?)?.max(start);
        Some([char_offset(text, start), char_offset(text, end)])
    })();
    (hover_markdown(&answer["contents"]), range)
}

fn utf16_to_chars(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (index, c) in text.chars().enumerate() {
        if seen >= units {
            return index;
        }
        seen += c.len_utf16();
    }
    text.chars().count()
}

fn documentation_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Object(markup) => markup.get("value").and_then(Value::as_str).map(str::to_owned),
        _ => None,
    }
    .filter(|text| !text.trim().is_empty())
}

pub fn signature(value: &Value, active_parameter: Option<u64>) -> Option<Signature> {
    let label = value["label"].as_str()?.to_owned();
    let mut from = 0;
    let parameters: Vec<[usize; 2]> = value["parameters"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|parameter| match &parameter["label"] {
                    Value::String(name) => {
                        let found =
                            label[from..].find(name.as_str()).map(|at| at + from).or_else(|| label.find(name.as_str()));
                        match found {
                            Some(start) => {
                                from = start + name.len();
                                [char_offset(&label, start), char_offset(&label, start + name.len())]
                            }
                            None => [0, 0],
                        }
                    }
                    Value::Array(pair) => {
                        let start = pair.first().and_then(Value::as_u64).unwrap_or(0) as usize;
                        let end = pair.get(1).and_then(Value::as_u64).unwrap_or(0) as usize;
                        let (start, end) = (utf16_to_chars(&label, start), utf16_to_chars(&label, end));
                        [start, end.max(start)]
                    }
                    _ => [0, 0],
                })
                .collect()
        })
        .unwrap_or_default();
    let active = value["activeParameter"].as_u64().or(active_parameter).map(|index| index as usize);
    Some(Signature {
        active_parameter: active.filter(|index| *index < parameters.len()),
        parameters,
        documentation: documentation_text(&value["documentation"]),
        label,
    })
}

pub fn signature_help(answer: &Value) -> Option<(Vec<Signature>, usize, Option<usize>)> {
    let list = answer["signatures"].as_array().filter(|list| !list.is_empty())?;
    let top = answer["activeParameter"].as_u64();
    let signatures: Vec<Signature> = list.iter().filter_map(|value| signature(value, top)).collect();
    if signatures.is_empty() {
        return None;
    }
    let active = answer["activeSignature"].as_u64().map_or(0, |index| index as usize);
    let active = if active < signatures.len() { active } else { 0 };
    let parameter = signatures[active].active_parameter;
    Some((signatures, active, parameter))
}

fn answered(languages: &mut Languages, kind: LookupKind, wait: Duration) -> Option<Option<Result<Value, String>>> {
    match languages.lookup_mut(kind) {
        Some(lookup) => lookup.poll(wait).then(|| lookup.answer.clone()),
        None => Some(None),
    }
}

impl Popups {
    fn documentation_state(&mut self, languages: &mut Languages, wait: Duration) -> DocumentationState {
        let Some(shown) = &mut self.documentation else { return DocumentationState::default() };
        if shown.state.pending
            && let Some(answer) = answered(languages, LookupKind::Documentation, wait)
        {
            let text = languages.synced_text(&shown.path).unwrap_or_default().to_owned();
            match answer {
                Some(Ok(value)) => {
                    let (markdown, range) = hover(&text, &value);
                    shown.state.markdown = markdown;
                    shown.state.range = range;
                }
                Some(Err(error)) => shown.state.error = Some(error),
                None => {}
            }
            shown.state.pending = false;
        }
        DocumentationState { open: true, ..shown.state.clone() }
    }

    fn close_parameter_info(&mut self, languages: &mut Languages) -> ParameterInfoState {
        languages.end_lookup(LookupKind::ParameterInfo);
        self.parameter_info = None;
        ParameterInfoState::default()
    }

    fn parameter_info_state(
        &mut self,
        languages: &mut Languages,
        workbench: &Workbench,
        wait: Duration,
    ) -> ParameterInfoState {
        let Some(shown) = &mut self.parameter_info else { return ParameterInfoState::default() };
        if shown.follows_caret {
            let Some((caret, version)) = caret_of(workbench, &shown.path).filter(|_| is_active(workbench, &shown.path))
            else {
                return self.close_parameter_info(languages);
            };
            if (caret, version) != (shown.caret, shown.version) {
                shown.caret = caret;
                shown.version = version;
                let mut context = json!({ "triggerKind": 3, "isRetrigger": true });
                if !shown.help.is_null() {
                    context["activeSignatureHelp"] = shown.help.clone();
                }
                let path = shown.path.clone();
                let started = languages.start_lookup(
                    workbench,
                    LookupKind::ParameterInfo,
                    &path,
                    caret,
                    "textDocument/signatureHelp",
                    json!({ "context": context }),
                );
                if started.is_err() {
                    return self.close_parameter_info(languages);
                }
                let Some(shown) = &mut self.parameter_info else { return ParameterInfoState::default() };
                shown.state.pending = true;
            }
        }
        let Some(shown) = &mut self.parameter_info else { return ParameterInfoState::default() };
        if shown.state.pending {
            match answered(languages, LookupKind::ParameterInfo, wait) {
                Some(Some(Ok(value))) => match signature_help(&value) {
                    Some((signatures, active, parameter)) => {
                        shown.help = value;
                        shown.state = ParameterInfoState {
                            open: true,
                            pending: false,
                            signatures,
                            active: Some(active),
                            active_parameter: parameter,
                            error: None,
                        };
                    }
                    None => return self.close_parameter_info(languages),
                },
                Some(Some(Err(error))) => {
                    shown.state.error = Some(error);
                    shown.state.pending = false;
                }
                Some(None) => shown.state.pending = false,
                None => {}
            }
        }
        ParameterInfoState { open: true, ..shown.state.clone() }
    }
}

pub struct ShowDocumentation;

impl Command for ShowDocumentation {
    type Params = LookupParams;
    type Output = DocumentationState;
    const ID: &'static str = "editor.documentation.show";
    const TITLE: &'static str = "Quick Documentation";
    const DESCRIPTION: &'static str = "Asks the language server for the documentation of the symbol at a place (the caret by \
         default) and shows it in a popup; editor.documentation.state reads it.";

    fn run(&self, session: &mut Session, params: LookupParams) -> Result<DocumentationState, CommandError> {
        let wait = params.wait();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(workbench) = session.state::<Workbench>()
            && let Ok((path, offset)) = params.place(workbench)
            && let Some(documented) = crate::database::assist::documentation(session, &path, offset)
        {
            let state = DocumentationState {
                open: true,
                pending: false,
                markdown: documented.as_ref().map(|documented| documented.markdown.clone()),
                range: documented.as_ref().map(|documented| documented.range),
                error: None,
            };
            session.state_mut::<Popups>().documentation =
                Some(ShownDocumentation { path, offset, state: state.clone() });
            return Ok(state);
        }
        if let Some(workbench) = session.state::<Workbench>()
            && let Ok((path, offset)) = params.place(workbench)
            && let Some(documented) = crate::spring_boot::documentation(session, &path, offset)
        {
            let state = DocumentationState {
                open: true,
                pending: false,
                markdown: Some(documented.markdown),
                range: Some(documented.range),
                error: None,
            };
            session.state_mut::<Popups>().documentation =
                Some(ShownDocumentation { path, offset, state: state.clone() });
            return Ok(state);
        }
        with_popups(session, |popups, languages, workbench| {
            let (path, offset) = params.place(workbench)?;
            let same = popups.documentation.as_ref().is_some_and(|shown| shown.path == path && shown.offset == offset);
            if !same {
                languages
                    .start_lookup(workbench, LookupKind::Documentation, &path, offset, "textDocument/hover", json!({}))
                    .map_err(CommandError::new)?;
                popups.documentation = Some(ShownDocumentation {
                    path,
                    offset,
                    state: DocumentationState { pending: true, ..DocumentationState::default() },
                });
            }
            Ok(popups.documentation_state(languages, wait))
        })
    }
}

pub struct DocumentationStateCommand;

impl Command for DocumentationStateCommand {
    type Params = WaitParams;
    type Output = DocumentationState;
    const ID: &'static str = "editor.documentation.state";
    const TITLE: &'static str = "Quick Documentation State";
    const DESCRIPTION: &'static str = "The quick documentation shown, waiting up to waitMs for the server's answer.";

    fn run(&self, session: &mut Session, params: WaitParams) -> Result<DocumentationState, CommandError> {
        let wait = params.wait();
        Ok(with_popups(session, |popups, languages, _| popups.documentation_state(languages, wait)))
    }
}

pub struct HideDocumentation;

impl Command for HideDocumentation {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "editor.documentation.hide";
    const TITLE: &'static str = "Hide Quick Documentation";
    const DESCRIPTION: &'static str = "Closes the quick documentation popup, cancelling its request if it still waits.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        with_popups(session, |popups, languages, _| {
            languages.end_lookup(LookupKind::Documentation);
            popups.documentation = None;
        });
        Ok(())
    }
}

pub struct ShowParameterInfo;

impl Command for ShowParameterInfo {
    type Params = LookupParams;
    type Output = ParameterInfoState;
    const ID: &'static str = "editor.parameterInfo.show";
    const TITLE: &'static str = "Parameter Info";
    const DESCRIPTION: &'static str = "Asks the language server for the signatures of the call at a place (the caret by \
         default) and shows them with the current parameter; editor.parameterInfo.state reads them, asks again as the \
         caret moves and closes them when the caret leaves the call.";

    fn run(&self, session: &mut Session, params: LookupParams) -> Result<ParameterInfoState, CommandError> {
        let wait = params.wait();
        with_popups(session, |popups, languages, workbench| {
            let (path, offset) = params.place(workbench)?;
            let (caret, version) = caret_of(workbench, &path).unwrap_or((offset, 0));
            languages
                .start_lookup(
                    workbench,
                    LookupKind::ParameterInfo,
                    &path,
                    offset,
                    "textDocument/signatureHelp",
                    json!({ "context": { "triggerKind": 1, "isRetrigger": false } }),
                )
                .map_err(CommandError::new)?;
            popups.parameter_info = Some(ShownParameterInfo {
                follows_caret: is_active(workbench, &path) && caret == offset,
                path,
                caret,
                version,
                help: Value::Null,
                state: ParameterInfoState { pending: true, ..ParameterInfoState::default() },
            });
            Ok(popups.parameter_info_state(languages, workbench, wait))
        })
    }
}

pub struct ParameterInfoStateCommand;

impl Command for ParameterInfoStateCommand {
    type Params = WaitParams;
    type Output = ParameterInfoState;
    const ID: &'static str = "editor.parameterInfo.state";
    const TITLE: &'static str = "Parameter Info State";
    const DESCRIPTION: &'static str = "The parameter info shown, waiting up to waitMs for the server's answer; asks again \
         when the caret moved and closes when the caret left the call.";

    fn run(&self, session: &mut Session, params: WaitParams) -> Result<ParameterInfoState, CommandError> {
        let wait = params.wait();
        Ok(with_popups(session, |popups, languages, workbench| popups.parameter_info_state(languages, workbench, wait)))
    }
}

pub struct HideParameterInfo;

impl Command for HideParameterInfo {
    type Params = NoParams;
    type Output = ();
    const ID: &'static str = "editor.parameterInfo.hide";
    const TITLE: &'static str = "Hide Parameter Info";
    const DESCRIPTION: &'static str = "Closes the parameter info popup, cancelling its request if it still waits.";

    fn run(&self, session: &mut Session, _: NoParams) -> Result<(), CommandError> {
        with_popups(session, |popups, languages, _| {
            popups.close_parameter_info(languages);
        });
        Ok(())
    }
}

pub(super) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ShowDocumentation)?;
    registry.register(DocumentationStateCommand)?;
    registry.register(HideDocumentation)?;
    registry.register(ShowParameterInfo)?;
    registry.register(ParameterInfoStateCommand)?;
    registry.register(HideParameterInfo)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::testing::fixture;
    use zen_lsp::fake::Reply;

    const MAIN: &str = "package main\n\nimport \"fmt\"\n\n// привет\nfunc main() { fmt.Println(\"ё\", 1) }\n";

    fn chars(text: &str, needle: &str) -> usize {
        text[..text.find(needle).unwrap()].chars().count()
    }

    fn set_caret(fixture: &mut crate::language::testing::Fixture, caret: usize) {
        let document = fixture.session.state_mut::<Workbench>().active_document_mut().unwrap();
        document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(caret)));
    }

    fn parameter_state(fixture: &mut crate::language::testing::Fixture) -> Value {
        fixture.dispatch("editor.parameterInfo.state", json!({ "waitMs": 5000 })).unwrap()
    }

    #[test]
    fn hover_contents_of_every_form_become_markdown() {
        let markup = json!({ "kind": "markdown", "value": "```go\nfunc Println()\n```\n\nPrintln *prints*." });
        assert_eq!(hover_markdown(&markup).as_deref(), Some("```go\nfunc Println()\n```\n\nPrintln *prints*."));
        let plain = json!({ "kind": "plaintext", "value": "a*b\n  c_d\n\nnext" });
        assert_eq!(hover_markdown(&plain).as_deref(), Some("a\\*b\\\n\u{a0}\u{a0}c\\_d\n\nnext"));
        assert_eq!(hover_markdown(&json!("**bold**")).as_deref(), Some("**bold**"));
        let marked = json!([{ "language": "go", "value": "var x int" }, "x counts."]);
        assert_eq!(hover_markdown(&marked).as_deref(), Some("```go\nvar x int\n```\n\nx counts."));
        let ticks = json!({ "language": "md", "value": "```" });
        assert_eq!(hover_markdown(&ticks).as_deref(), Some("````md\n```\n````"));
        assert_eq!(hover_markdown(&json!({ "kind": "markdown", "value": "  " })), None);
        assert_eq!(hover_markdown(&json!([])), None);
    }

    #[test]
    fn the_hover_range_is_in_characters() {
        let text = "// привет\nvar ёлка = 1\n";
        let answer = json!({
            "contents": { "kind": "markdown", "value": "var ёлка int" },
            "range": { "start": { "line": 1, "character": 4 }, "end": { "line": 1, "character": 8 } }
        });
        let (markdown, range) = hover(text, &answer);
        assert_eq!(markdown.as_deref(), Some("var ёлка int"));
        let [start, end] = range.unwrap();
        assert_eq!(text.chars().skip(start).take(end - start).collect::<String>(), "ёлка");
        assert_eq!(hover(text, &json!(null)), (None, None));
    }

    #[test]
    fn signature_labels_become_character_ranges() {
        let by_name = json!({
            "label": "func Printf(формат string, a ...any)",
            "parameters": [{ "label": "формат string" }, { "label": "a ...any" }]
        });
        let signature = signature(&by_name, Some(1)).unwrap();
        let label: Vec<char> = signature.label.chars().collect();
        let part = |[start, end]: [usize; 2]| label[start..end].iter().collect::<String>();
        assert_eq!(part(signature.parameters[0]), "формат string");
        assert_eq!(part(signature.parameters[1]), "a ...any");
        assert_eq!(signature.active_parameter, Some(1));

        let label_text = "func Emoji(😀 int, ё string)";
        let utf16 = |needle: &str| label_text[..label_text.find(needle).unwrap()].encode_utf16().count() as u64;
        let by_offsets = json!({
            "label": label_text,
            "parameters": [
                { "label": [utf16("😀"), utf16(", ё")] },
                { "label": [utf16("ё"), utf16(")")] }
            ],
            "activeParameter": 0,
            "documentation": { "kind": "markdown", "value": "Emoji." }
        });
        let signature = super::signature(&by_offsets, Some(1)).unwrap();
        let label: Vec<char> = signature.label.chars().collect();
        let part = |[start, end]: [usize; 2]| label[start..end].iter().collect::<String>();
        assert_eq!(part(signature.parameters[0]), "😀 int");
        assert_eq!(part(signature.parameters[1]), "ё string");
        assert_eq!(signature.active_parameter, Some(0), "the signature's own active parameter wins");
        assert_eq!(signature.documentation.as_deref(), Some("Emoji."));

        let repeated = json!({ "label": "f(a, a)", "parameters": [{ "label": "a" }, { "label": "a" }] });
        assert_eq!(super::signature(&repeated, None).unwrap().parameters, vec![[2, 3], [5, 6]]);
    }

    #[test]
    fn the_active_signature_and_parameter_are_chosen() {
        let answer = json!({
            "signatures": [
                { "label": "f(a int)", "parameters": [{ "label": "a int" }] },
                { "label": "f(a int, b int)", "parameters": [{ "label": "a int" }, { "label": "b int" }] }
            ],
            "activeSignature": 1,
            "activeParameter": 1
        });
        let (signatures, active, parameter) = signature_help(&answer).unwrap();
        assert_eq!((signatures.len(), active, parameter), (2, 1, Some(1)));
        assert_eq!(signatures[0].active_parameter, None, "past its parameters");
        let out_of_range = json!({ "signatures": [{ "label": "g()" }], "activeSignature": 4 });
        let (signatures, active, parameter) = signature_help(&out_of_range).unwrap();
        assert_eq!((active, parameter), (0, None));
        assert!(signatures[0].parameters.is_empty());
        assert!(signature_help(&json!(null)).is_none());
        assert!(signature_help(&json!({ "signatures": [] })).is_none());
    }

    #[test]
    fn quick_documentation_comes_from_hover_and_stays_on_a_second_press() {
        let mut fixture = fixture(&[("main.go", MAIN)]);
        let line = MAIN.lines().nth(5).unwrap();
        let character = line[..line.find("Println").unwrap()].encode_utf16().count();
        fixture.server.on(
            "textDocument/hover",
            json!({
                "contents": { "kind": "markdown", "value": "```go\nfunc fmt.Println(a ...any)\n```\n\nPrintln formats." },
                "range": { "start": { "line": 5, "character": character }, "end": { "line": 5, "character": character + 7 } }
            }),
        );
        fixture.open("main.go");
        let caret = chars(MAIN, "Println") + 2;
        set_caret(&mut fixture, caret);
        let shown = fixture.dispatch("editor.documentation.show", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(shown["open"], true);
        assert_eq!(shown["pending"], false);
        assert!(shown["markdown"].as_str().unwrap().starts_with("```go\nfunc fmt.Println"));
        let start = chars(MAIN, "Println");
        assert_eq!(shown["range"], json!([start, start + 7]));
        let sent = fixture.server.requests("textDocument/hover");
        assert_eq!(sent[0]["position"], json!({ "line": 5, "character": character + 2 }));
        fixture.dispatch("editor.documentation.show", json!({})).unwrap();
        assert_eq!(fixture.server.requests("textDocument/hover").len(), 1, "a second press keeps the popup");
        assert!(fixture.session.state::<Popups>().unwrap().documentation_open());
        fixture.dispatch("editor.documentation.hide", json!({})).unwrap();
        let state = fixture.dispatch("editor.documentation.state", json!({})).unwrap();
        assert_eq!(state["open"], false);
    }

    #[test]
    fn no_hover_means_no_documentation_and_a_held_one_is_cancelled() {
        let mut fixture = fixture(&[("main.go", MAIN)]);
        fixture.open("main.go");
        let shown = fixture.dispatch("editor.documentation.show", json!({ "offset": 0, "waitMs": 5000 })).unwrap();
        assert_eq!((shown["open"].clone(), shown["pending"].clone()), (json!(true), json!(false)));
        assert_eq!(shown["markdown"], Value::Null);
        assert_eq!(shown["error"], Value::Null);
        fixture.server.reply("textDocument/hover", Reply::Hold);
        let asked = fixture.server.requests("textDocument/hover").len();
        let shown = fixture.dispatch("editor.documentation.show", json!({ "offset": 3 })).unwrap();
        assert_eq!(shown["pending"], true);
        fixture.server.wait_for("textDocument/hover", asked + 1, Duration::from_secs(5));
        fixture.dispatch("editor.documentation.hide", json!({})).unwrap();
        assert_eq!(fixture.server.wait_for("$/cancelRequest", 1, Duration::from_secs(5)).len(), 1);
    }

    #[test]
    fn parameter_info_follows_the_caret_and_closes_outside_the_call() {
        let mut fixture = fixture(&[("main.go", MAIN)]);
        let call = chars(MAIN, "\"ё\"");
        let second = chars(MAIN, "1)");
        let outside = chars(MAIN, " }");
        fixture.server.on_with("textDocument/signatureHelp", move |params| {
            let character = params["position"]["character"].as_u64().unwrap_or(0) as usize;
            let line = MAIN.lines().nth(5).unwrap();
            let utf16 = |needle: &str| line[..line.find(needle).unwrap()].encode_utf16().count();
            let close = line[..line.rfind(')').unwrap()].encode_utf16().count();
            if character > close || character < utf16("\"ё\"") {
                return Value::Null;
            }
            let parameter = if character >= utf16("1)") { 1 } else { 0 };
            json!({
                "signatures": [{
                    "label": "func(format string, a ...any)",
                    "parameters": [{ "label": [5, 18] }, { "label": "a ...any" }]
                }],
                "activeSignature": 0,
                "activeParameter": parameter
            })
        });
        fixture.open("main.go");
        set_caret(&mut fixture, call);
        let shown = fixture.dispatch("editor.parameterInfo.show", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(shown["open"], true);
        assert_eq!(shown["signatures"][0]["parameters"], json!([[5, 18], [20, 28]]));
        assert_eq!(shown["activeParameter"], 0);
        let first = &fixture.server.requests("textDocument/signatureHelp")[0];
        assert_eq!(first["context"], json!({ "triggerKind": 1, "isRetrigger": false }));

        assert_eq!(parameter_state(&mut fixture)["activeParameter"], 0, "no new request while the caret stays");
        assert_eq!(fixture.server.requests("textDocument/signatureHelp").len(), 1);

        set_caret(&mut fixture, second);
        let moved = parameter_state(&mut fixture);
        assert_eq!((moved["open"].clone(), moved["activeParameter"].clone()), (json!(true), json!(1)));
        let retrigger = &fixture.server.requests("textDocument/signatureHelp")[1];
        assert_eq!(retrigger["context"]["triggerKind"], 3);
        assert_eq!(retrigger["context"]["isRetrigger"], true);
        assert_eq!(retrigger["context"]["activeSignatureHelp"]["activeParameter"], 0);

        set_caret(&mut fixture, outside);
        assert_eq!(parameter_state(&mut fixture)["open"], false);
        assert!(!fixture.session.state::<Popups>().unwrap().parameter_info_open());
    }

    #[test]
    fn parameter_info_closes_when_hidden_or_when_there_is_no_call() {
        let mut fixture = fixture(&[("main.go", MAIN)]);
        fixture.open("main.go");
        let shown = fixture.dispatch("editor.parameterInfo.show", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(shown["open"], false, "the server knows no call there");
        fixture
            .server
            .on("textDocument/signatureHelp", json!({ "signatures": [{ "label": "main()", "parameters": [] }] }));
        let shown = fixture.dispatch("editor.parameterInfo.show", json!({ "waitMs": 5000 })).unwrap();
        assert_eq!(shown["open"], true);
        assert_eq!(shown["signatures"][0]["parameters"], json!([]));
        fixture.dispatch("editor.parameterInfo.hide", json!({})).unwrap();
        assert_eq!(parameter_state(&mut fixture)["open"], false);
    }
}
