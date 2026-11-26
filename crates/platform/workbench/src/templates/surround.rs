use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::PoisonError;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use super::{
    Around, Expanded, LiveTemplates, SELECTION, SessionInfo, Template, Variable, document_at, expand_around, surrounds,
};
use crate::brackets::Highlighters;
use crate::language::{byte_offset, char_offset};
use crate::{Document, Workbench, code_style, project_settings};

/// A construct of a language that code can be surrounded with, as a language module declares it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Surrounder {
    /// The language id of the files it is for, e.g. `go`.
    pub language: String,
    /// Its stable name, e.g. `if`.
    pub id: String,
    /// What the list shows: a message id of the module that declares it, or the text itself.
    pub title: String,
    /// The text, as a live template's, with `$SELECTION$` for what it surrounds.
    pub text: String,
    /// The variables' default values, in the order Tab visits them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<Variable>,
    /// Whether it surrounds an expression: it is offered only for a selection, taken as it is.
    #[serde(default)]
    pub expression: bool,
}

impl Surrounder {
    pub fn parse_list(json: &str) -> Result<Vec<Surrounder>, String> {
        serde_json::from_str(json).map_err(|error| error.to_string())
    }

    fn template(&self) -> Template {
        Template {
            language: self.language.clone(),
            abbreviation: self.id.clone(),
            description: self.title.clone(),
            text: self.text.clone(),
            variables: self.variables.clone(),
            imports: Vec::new(),
            contexts: Vec::new(),
            enabled: true,
        }
    }
}

const REGION: &str = "region";
const EDITOR_FOLD: &str = "editor-fold";

fn comment_surrounders(file_name: &str, language: &str) -> Vec<Surrounder> {
    let Some(syntax) = zen_syntax::comment_syntax(file_name) else { return Vec::new() };
    let comment = |body: &str| match (syntax.line, syntax.block) {
        (Some(line), _) => format!("{line} {body}"),
        (None, Some((open, close))) => format!("{open} {body} {close}"),
        (None, None) => String::new(),
    };
    if syntax.line.is_none() && syntax.block.is_none() {
        return Vec::new();
    }
    let surrounder = |id: &str, title: &str, open: &str, close: &str| Surrounder {
        language: language.to_owned(),
        id: id.to_owned(),
        title: title.to_owned(),
        text: format!("{}\n${SELECTION}$\n{}", comment(open), comment(close)),
        variables: vec![Variable {
            name: "DESCRIPTION".to_owned(),
            default: t!("workbench-surround-description"),
            ..Variable::default()
        }],
        expression: false,
    };
    vec![
        surrounder(
            EDITOR_FOLD,
            "workbench-surround-editor-fold",
            "<editor-fold desc=\"$DESCRIPTION$\">",
            "</editor-fold>",
        ),
        surrounder(REGION, "workbench-surround-region", "region $DESCRIPTION$", "endregion"),
    ]
}

/// One way to surround the code, as `editor.surround.list` gives it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SurroundItem {
    /// Its place in the list, from 0.
    pub index: usize,
    /// The key that picks it in the popup: `1`–`9`, `0`, then `A`–`Z`; empty past those.
    pub mnemonic: String,
    /// The surrounder's id, or the live template's abbreviation, for `editor.surround.apply`.
    pub name: String,
    /// Whether it is a live template.
    pub template: bool,
    /// What the list shows, in the interface's language.
    pub title: String,
    /// A live template's description.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The code it surrounds, as character offsets `[start, end]`.
    pub range: [usize; 2],
    /// The lines it changes as they would read after it, when asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

/// What `editor.surround.list` gives.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SurroundList {
    /// The file.
    pub path: PathBuf,
    /// The ways to surround the code: the language's, then the region comments, then the live templates.
    pub items: Vec<SurroundItem>,
}

pub fn mnemonic(index: usize) -> String {
    match index {
        0..=8 => (index + 1).to_string(),
        9 => "0".to_owned(),
        10..=35 => char::from(b'A' + (index - 10) as u8).to_string(),
        _ => String::new(),
    }
}

struct Choice {
    name: String,
    template: bool,
    title: String,
    description: String,
    expression: bool,
    surrounds_with: Template,
}

fn choices(session: &Session, document: &Document, templates_only: bool, selected: bool) -> Vec<Choice> {
    let language = code_style::language_of(document.path());
    let Some(live) = session.state::<LiveTemplates>() else { return Vec::new() };
    let mut choices = Vec::new();
    if !templates_only {
        let declared = live.surrounders_for(&language);
        let all = declared.into_iter().chain(comment_surrounders(&document.name(), &language));
        choices.extend(all.filter(|surrounder| selected || !surrounder.expression).map(|surrounder| Choice {
            name: surrounder.id.clone(),
            template: false,
            title: zen_i18n::text(&surrounder.title, &[]),
            description: String::new(),
            expression: surrounder.expression,
            surrounds_with: surrounder.template(),
        }));
    }
    choices.extend(live.for_language(&language).into_iter().filter(|template| surrounds(&template.text)).map(
        |template| Choice {
            name: template.abbreviation.clone(),
            template: true,
            title: template.abbreviation.clone(),
            description: template.description_text(),
            expression: !template.text.lines().any(|line| line.trim() == format!("${SELECTION}$")),
            surrounds_with: template,
        },
    ));
    choices
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |found| found + 1)
}

fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |found| at + found)
}

fn content_start(text: &str, at: usize) -> usize {
    let start = line_start(text, at);
    let line = &text[start..line_end(text, at)];
    start + line.len() - line.trim_start().len()
}

fn trimmed_end(text: &str, from: usize, to: usize) -> usize {
    from + text[from..to].trim_end().len()
}

fn indentation(text: &str, at: usize) -> &str {
    let start = line_start(text, at);
    let line = &text[start..at];
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

struct Bracket {
    at: usize,
    open: bool,
    pair: Option<usize>,
}

struct Code<'a> {
    text: &'a str,
    skipped: &'a [Range<usize>],
    brackets: Vec<Bracket>,
}

impl<'a> Code<'a> {
    fn new(text: &'a str, skipped: &'a [Range<usize>]) -> Self {
        let mut brackets: Vec<Bracket> = Vec::new();
        let mut open: Vec<usize> = Vec::new();
        let mut next = 0;
        for (at, byte) in text.bytes().enumerate() {
            let opens = match byte {
                b'(' | b'[' | b'{' => true,
                b')' | b']' | b'}' => false,
                _ => continue,
            };
            while skipped.get(next).is_some_and(|range| range.end <= at) {
                next += 1;
            }
            if skipped.get(next).is_some_and(|range| range.start <= at) {
                continue;
            }
            let index = brackets.len();
            let mut pair = None;
            if !opens
                && let Some(&last) = open.last()
                && text.as_bytes()[brackets[last].at] == partner(byte)
            {
                open.pop();
                brackets[last].pair = Some(index);
                pair = Some(last);
            }
            if opens {
                open.push(index);
            }
            brackets.push(Bracket { at, open: opens, pair });
        }
        Self { text, skipped, brackets }
    }

    fn skipped_at(&self, at: usize) -> Option<&Range<usize>> {
        let index = self.skipped.partition_point(|range| range.end <= at);
        self.skipped.get(index).filter(|range| range.start <= at)
    }

    fn starts_inside(&self, at: usize) -> Option<usize> {
        self.skipped_at(at).filter(|range| range.start < at).map(|range| range.start)
    }

    fn first_from(&self, at: usize) -> usize {
        self.brackets.partition_point(|bracket| bracket.at < at)
    }

    fn enclosing(&self, at: usize) -> Option<usize> {
        let mut index = self.first_from(at);
        while index > 0 {
            let bracket = &self.brackets[index - 1];
            if bracket.open {
                return Some(index - 1);
            }
            index = bracket.pair.unwrap_or(index - 1);
        }
        None
    }

    fn reopened(&self, from: usize, to: usize) -> Option<usize> {
        let mut depth = 0usize;
        for bracket in &self.brackets[self.first_from(from)..self.first_from(to)] {
            if bracket.open {
                depth += 1;
            } else if depth > 0 {
                depth -= 1;
            } else if let Some(pair) = bracket.pair {
                return Some(self.brackets[pair].at);
            }
        }
        None
    }

    fn literal(&self, index: usize) -> bool {
        let Some(close) = self.brackets[index].pair else { return false };
        let (from, to) = (self.brackets[index].at + 1, self.brackets[close].at);
        let mut depth = 0usize;
        let mut last = None;
        let mut next = self.first_from(from);
        for (offset, byte) in self.text[from..to].bytes().enumerate() {
            let at = from + offset;
            if self.brackets.get(next).is_some_and(|bracket| bracket.at == at) {
                if self.brackets[next].open {
                    depth += 1;
                } else {
                    depth = depth.saturating_sub(1);
                }
                next += 1;
            }
            if byte == b'\n' && depth == 0 && self.skipped_at(at).is_none() && last == Some(b',') {
                return true;
            }
            if !byte.is_ascii_whitespace() && self.skipped_at(at).is_none() {
                last = Some(byte);
            }
        }
        last == Some(b',')
    }

    fn continues(&self, newline: usize) -> bool {
        let text = self.text;
        let start = line_start(text, newline);
        let line = text[start..newline].trim_end();
        if let Some(last) = line.char_indices().next_back()
            && self.skipped_at(start + last.0).is_none()
            && !line.ends_with("++")
            && !line.ends_with("--")
            && "+-*/%&|^=,.\\".contains(last.1)
        {
            return true;
        }
        let next = newline + 1;
        if next >= text.len() {
            return false;
        }
        let first = content_start(text, next);
        let rest = &text[first..line_end(text, next)];
        self.skipped_at(first).is_none()
            && [".", "&&", "||", "?", ":"].iter().any(|lead| rest.starts_with(lead))
            && !rest.starts_with("::")
    }

    fn statement_end(&self, start: usize) -> usize {
        let text = self.text;
        let mut depth = 0usize;
        let mut at = start;
        let mut next = self.first_from(start);
        while at < text.len() {
            if let Some(range) = self.skipped_at(at) {
                at = range.end.max(at + 1);
                next = self.first_from(at);
                continue;
            }
            if self.brackets.get(next).is_some_and(|bracket| bracket.at == at) {
                if self.brackets[next].open {
                    depth += 1;
                } else if depth == 0 {
                    return trimmed_end(text, start, at).max(start);
                } else {
                    depth -= 1;
                }
                next += 1;
            } else if text.as_bytes()[at] == b'\n' && depth == 0 && !self.continues(at) {
                return trimmed_end(text, start, at);
            }
            at += 1;
        }
        trimmed_end(text, start, text.len())
    }

    fn statement_at(&self, at: usize) -> Range<usize> {
        let text = self.text;
        let first = line_start(text, at);
        let mut start = first;
        loop {
            if let Some(from) = self.starts_inside(start) {
                start = line_start(text, from);
                continue;
            }
            let content = content_start(text, start);
            if let Some(open) = self.reopened(content, line_end(text, content)) {
                start = line_start(text, open);
                continue;
            }
            if start > 0 && self.continues(start - 1) {
                start = line_start(text, start - 1);
                continue;
            }
            match self.enclosing(content) {
                Some(index) if text.as_bytes()[self.brackets[index].at] != b'{' || self.literal(index) => {
                    start = line_start(text, self.brackets[index].at);
                }
                _ => break,
            }
        }
        if start == first && text[start..line_end(text, start)].trim().is_empty() {
            return at..at;
        }
        let content = content_start(text, start);
        content..self.statement_end(content)
    }

    fn whole(&self, mut range: Range<usize>) -> Range<usize> {
        loop {
            let mut grown = range.clone();
            let mut open = Vec::new();
            for bracket in &self.brackets[self.first_from(range.start)..self.first_from(range.end)] {
                if bracket.open {
                    open.push(bracket);
                } else if open.pop().is_none()
                    && let Some(pair) = bracket.pair
                {
                    let statement = self.statement_at(self.brackets[pair].at);
                    grown = grown.start.min(statement.start)..grown.end.max(statement.end);
                }
            }
            for bracket in open {
                if let Some(pair) = bracket.pair {
                    let statement = self.statement_at(self.brackets[pair].at);
                    grown = grown.start.min(statement.start)..grown.end.max(statement.end);
                }
            }
            if grown == range {
                return range;
            }
            range = grown;
        }
    }

    fn target(&self, selection: Range<usize>, expression: bool) -> Range<usize> {
        let picked = &self.text[selection.clone()];
        if picked.trim().is_empty() {
            return self.statement_at(selection.end);
        }
        if expression {
            return selection;
        }
        let first = selection.start + picked.len() - picked.trim_start().len();
        let last = selection.start + picked.trim_end().len();
        let start = self.statement_at(first);
        let end = self.statement_at(last - 1);
        self.whole(start.start.min(first)..end.end.max(last))
    }

    fn around(&self, range: Range<usize>) -> Around {
        let text = self.text;
        let base = indentation(text, range.start);
        let mut around = Around::default();
        let mut at = range.start;
        for (index, line) in text[range.clone()].split_inclusive('\n').enumerate() {
            let start = at;
            at += line.len();
            if index == 0 || self.starts_inside(start).is_some() {
                if index > 0 {
                    around.verbatim.push(index);
                }
                around.text.push_str(line);
            } else {
                around.text.push_str(line.strip_prefix(base).unwrap_or_else(|| line.trim_start_matches([' ', '\t'])));
            }
        }
        let end = line_end(text, range.end);
        around.crlf = if end < text.len() {
            text[..end].ends_with('\r')
        } else {
            text[..line_start(text, range.start)].ends_with("\r\n")
        };
        around
    }
}

fn partner(close: u8) -> u8 {
    match close {
        b')' => b'(',
        b']' => b'[',
        _ => b'{',
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wrapped {
    pub range: Range<usize>,
    pub expanded: Expanded,
}

pub fn wrap(
    text: &str,
    skipped: &[Range<usize>],
    selection: Range<usize>,
    template: &Template,
    expression: bool,
    unit: &str,
) -> Wrapped {
    let code = Code::new(text, skipped);
    let range = code.target(selection, expression);
    let expanded = expand_around(template, indentation(text, range.start), unit, &code.around(range.clone()));
    Wrapped { range, expanded }
}

fn preview(text: &str, wrapped: &Wrapped) -> String {
    let range = &wrapped.range;
    let after = &text[range.end..line_end(text, range.end)];
    let before = &text[line_start(text, range.start)..range.start];
    format!("{before}{}{}", wrapped.expanded.text, after.trim_end()).replace('\r', "")
}

fn primary_selection(document: &Document) -> Range<usize> {
    let range = document.selections().primary().range();
    let text = document.text();
    byte_offset(text, range.start)..byte_offset(text, range.end)
}

fn skipped(session: &Session, document: &Document) -> Vec<Range<usize>> {
    let text = document.text();
    let Some(highlighting) =
        session.state::<Highlighters>().and_then(|highlighters| highlighters.highlighting_of(document))
    else {
        return Vec::new();
    };
    let spans = highlighting.lock().unwrap_or_else(PoisonError::into_inner).spans(text, 0..text.len());
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for span in spans.into_iter().filter(|span| span.kind.is_comment() || span.kind.is_string()) {
        match ranges.last_mut() {
            Some(last) if last.end >= span.range.start => last.end = last.end.max(span.range.end),
            _ => ranges.push(span.range),
        }
    }
    for range in &mut ranges {
        range.end = range.start + text[range.clone()].trim_end_matches(['\n', '\r']).len();
    }
    ranges.retain(|range| !range.is_empty());
    ranges
}

pub(crate) fn list(
    session: &mut Session,
    path: Option<&Path>,
    templates_only: bool,
    with_preview: bool,
) -> Result<SurroundList, CommandError> {
    let path = document_at(session, path)?.path().to_owned();
    let workbench = session.state::<Workbench>().ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let document = workbench
        .documents()
        .iter()
        .find(|document| document.path() == path)
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
    let text = document.text();
    let selection = primary_selection(document);
    let unit = project_settings::indent_unit_of(session, document);
    let skipped = skipped(session, document);
    let items = choices(session, document, templates_only, !text[selection.clone()].trim().is_empty())
        .into_iter()
        .enumerate()
        .map(|(index, choice)| {
            let wrapped = wrap(text, &skipped, selection.clone(), &choice.surrounds_with, choice.expression, &unit);
            SurroundItem {
                index,
                mnemonic: mnemonic(index),
                preview: with_preview.then(|| preview(text, &wrapped)),
                range: [char_offset(text, wrapped.range.start), char_offset(text, wrapped.range.end)],
                name: choice.name,
                template: choice.template,
                title: choice.title,
                description: choice.description,
            }
        })
        .collect();
    Ok(SurroundList { path, items })
}

pub(crate) fn apply(
    session: &mut Session,
    path: Option<&Path>,
    name: &str,
    template: bool,
) -> Result<SessionInfo, CommandError> {
    let path = document_at(session, path)?.path().to_owned();
    let (range, wrapped, chosen) = {
        let document = session
            .state::<Workbench>()
            .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
            .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))?;
        let text = document.text();
        let selection = primary_selection(document);
        let chosen = choices(session, document, template, !text[selection.clone()].trim().is_empty())
            .into_iter()
            .find(|choice| choice.name == name && choice.template == template)
            .ok_or_else(|| CommandError::new(t!("workbench-surround-unknown", name = name)))?;
        let unit = project_settings::indent_unit_of(session, document);
        let skipped = skipped(session, document);
        let wrapped = wrap(text, &skipped, selection, &chosen.surrounds_with, chosen.expression, &unit);
        let range = char_offset(text, wrapped.range.start)..char_offset(text, wrapped.range.end);
        (range, wrapped, chosen.surrounds_with)
    };
    super::insert_expanded(session, &path, range, &chosen, &wrapped.expanded)
}

/// The parameters of `editor.surround.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// The file, open in the editor; the active one when absent.
    pub path: Option<PathBuf>,
    /// Only the live templates that hold `$SELECTION$` (Surround with Live Template, ⌥⌘J).
    pub templates_only: bool,
    /// Give each item the lines it changes as they would read after it.
    pub preview: bool,
}

pub struct ListSurrounders;

impl Command for ListSurrounders {
    type Params = ListParams;
    type Output = SurroundList;
    const ID: &'static str = "editor.surround.list";
    const TITLE: &'static str = "Surround With Items";
    const DESCRIPTION: &'static str = "What the primary selection, or the statement at the caret without one, can be \
         surrounded with in the file's language: its constructs, the custom folding region comments, then the live \
         templates that hold $SELECTION$ (templatesOnly: only those, as ⌥⌘J); each with the range it wraps and, with \
         preview, the lines as they would read.";

    fn is_available(&self, session: &Session) -> bool {
        super::has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: ListParams) -> Result<SurroundList, CommandError> {
        list(session, params.path.as_deref(), params.templates_only, params.preview)
    }
}

/// The parameters of `editor.surround.apply`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ApplyParams {
    /// The surrounder's id (`if`, `region`...) or the live template's abbreviation, as listed.
    pub name: String,
    /// Whether `name` is a live template's abbreviation.
    #[serde(default)]
    pub template: bool,
    /// The file, open in the editor; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
}

pub struct ApplySurrounder;

impl Command for ApplySurrounder {
    type Params = ApplyParams;
    type Output = SessionInfo;
    const ID: &'static str = "editor.surround.apply";
    const TITLE: &'static str = "Surround With";
    const DESCRIPTION: &'static str = "Surrounds the primary selection, or the statement at the caret without one, with \
         a construct or a live template from editor.surround.list, as one undo step; the result is indented and the \
         template's variables are filled in as after Tab (reports the variable the caret is on).";

    fn is_available(&self, session: &Session) -> bool {
        super::has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: ApplyParams) -> Result<SessionInfo, CommandError> {
        apply(session, params.path.as_deref(), &params.name, params.template)
    }
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListSurrounders)?;
    registry.register(ApplySurrounder)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{Value, json};
    use zen_syntax::{HighlightKind, Highlighter, Span};

    use super::*;

    struct Literals;

    impl Highlighter for Literals {
        fn handles(&self, file_name: &str) -> bool {
            file_name.ends_with(".go")
        }

        fn highlight(&self, text: &str) -> Vec<Span> {
            literals(text).into_iter().map(|range| Span { range, kind: HighlightKind::String }).collect()
        }
    }

    fn literals(text: &str) -> Vec<Range<usize>> {
        let mut ranges = Vec::new();
        let mut at = 0;
        while at < text.len() {
            let rest = &text[at..];
            let length = if rest.starts_with("//") {
                rest.find('\n').unwrap_or(rest.len())
            } else if let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '`') {
                rest[1..].find(quote).map_or(rest.len(), |close| close + 2)
            } else {
                at += rest.chars().next().map_or(1, char::len_utf8);
                continue;
            };
            ranges.push(at..at + length);
            at += length;
        }
        ranges
    }

    const SURROUNDERS: &str = r#"[
        { "language": "go", "id": "if", "title": "if", "text": "if $CONDITION$ {\n\t$SELECTION$\n}" },
        { "language": "go", "id": "ifElse", "title": "if / else", "text": "if $CONDITION$ {\n\t$SELECTION$\n} else {\n\t$END$\n}" },
        { "language": "go", "id": "not", "title": "!(expr)", "text": "!($SELECTION$)", "expression": true }
    ]"#;

    fn open(name: &str, text: &str) -> (Registry, Session, tempfile::TempDir) {
        zen_i18n::register(crate::LOCALES);
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(name), text).unwrap();
        let mut session = Session::new();
        let highlighter: Arc<dyn Highlighter> = Arc::new(Literals);
        session
            .state_mut::<Highlighters>()
            .set_source(Arc::new(move |name| Literals.handles(name).then(|| Arc::clone(&highlighter))));
        let templates = LiveTemplates::default();
        templates.declare_language(super::super::Builtin {
            language: "go".to_owned(),
            templates: Vec::new(),
            add_import: None,
            surrounders: Surrounder::parse_list(SURROUNDERS).unwrap(),
            ..Default::default()
        });
        session.set_state(templates);
        registry.dispatch(&mut session, "file.document.open", json!({ "path": dir.path().join(name) })).unwrap();
        (registry, session, dir)
    }

    fn run(registry: &Registry, session: &mut Session, id: &str, params: Value) -> Value {
        registry.dispatch(session, id, params).unwrap()
    }

    fn select(registry: &Registry, session: &mut Session, anchor: usize, head: usize) {
        run(registry, session, "editor.selection.set", json!({ "selections": [{ "anchor": anchor, "head": head }] }));
    }

    fn text(session: &Session) -> String {
        session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
    }

    fn selections(session: &Session) -> Vec<(usize, usize)> {
        let document = session.state::<Workbench>().unwrap().active_document().unwrap();
        document.selections().iter().map(|selection| (selection.anchor, selection.head)).collect()
    }

    fn names(list: &Value) -> Vec<String> {
        list["items"].as_array().unwrap().iter().map(|item| item["name"].as_str().unwrap().to_owned()).collect()
    }

    const BODY: &str = "package main\n\nfunc f(x int) {\n\ty := x + 1\n\tprintln(y)\n}\n";

    #[test]
    fn without_a_selection_the_statement_at_the_caret_is_wrapped_and_the_caret_goes_to_the_condition() {
        let (registry, mut session, _dir) = open("main.go", BODY);
        let caret = BODY.find("println").unwrap() + 3;
        select(&registry, &mut session, caret, caret);
        let list = run(&registry, &mut session, "editor.surround.list", json!({}));
        assert_eq!(names(&list), ["if", "ifElse", "editor-fold", "region"], "!(expr) needs a selection");
        assert_eq!(list["items"][1]["mnemonic"], "2");
        let info = run(&registry, &mut session, "editor.surround.apply", json!({ "name": "if" }));
        assert_eq!(text(&session), "package main\n\nfunc f(x int) {\n\ty := x + 1\n\tif  {\n\t\tprintln(y)\n\t}\n}\n");
        assert_eq!(info["variable"], "CONDITION");
        let condition = text(&session).find("if ").unwrap() + 3;
        assert_eq!(selections(&session), [(condition, condition)]);
        run(&registry, &mut session, "editor.edit.type", json!({ "text": "y > 0" }));
        run(&registry, &mut session, "editor.edit.tab", json!({}));
        let done = text(&session);
        assert!(done.contains("\tif y > 0 {\n\t\tprintln(y)\n\t}\n"), "{done}");
        let end = done.find("\t}\n}").unwrap() + 2;
        assert_eq!(selections(&session), [(end, end)]);
        run(&registry, &mut session, "editor.edit.undo", json!({}));
        run(&registry, &mut session, "editor.edit.undo", json!({}));
        assert_eq!(text(&session), BODY, "the surrounding is one undo step");
    }

    #[test]
    fn a_block_statement_is_wrapped_whole_and_reindented() {
        let body =
            "package main\n\nfunc f(xs []int) {\n\tfor _, x := range xs {\n\t\tprintln(\"{\", x)\n\t}\n\treturn\n}\n";
        let (registry, mut session, _dir) = open("main.go", body);
        let caret = body.find("range").unwrap();
        select(&registry, &mut session, caret, caret);
        run(&registry, &mut session, "editor.surround.apply", json!({ "name": "ifElse" }));
        assert_eq!(
            text(&session),
            "package main\n\nfunc f(xs []int) {\n\tif  {\n\t\tfor _, x := range xs {\n\t\t\tprintln(\"{\", x)\n\t\t}\n\t} else {\n\t\t\n\t}\n\treturn\n}\n"
        );
        run(&registry, &mut session, "editor.templates.next", json!({}));
        let done = text(&session);
        let inside_else = done.find("else {\n\t\t").unwrap() + 9;
        assert_eq!(selections(&session), [(inside_else, inside_else)], "$END$ is in the else branch");
    }

    #[test]
    fn a_selection_is_widened_to_whole_lines_for_a_region_and_taken_as_it_is_for_an_expression() {
        let (registry, mut session, _dir) = open("main.go", BODY);
        let start = BODY.find("x + 1").unwrap();
        let end = BODY.find("(y)").unwrap();
        select(&registry, &mut session, start, end);
        run(&registry, &mut session, "editor.surround.apply", json!({ "name": "region" }));
        let done = text(&session);
        assert_eq!(
            done,
            "package main\n\nfunc f(x int) {\n\t// region Description\n\ty := x + 1\n\tprintln(y)\n\t// endregion\n}\n"
        );
        let description = done.find("Description").unwrap();
        assert_eq!(selections(&session), [(description, description + 11)], "the description is selected");

        let (registry, mut session, _dir) = open("main.go", BODY);
        select(&registry, &mut session, start, start + 5);
        let list = run(&registry, &mut session, "editor.surround.list", json!({ "preview": true }));
        let not = list["items"].as_array().unwrap().iter().find(|item| item["name"] == "not").unwrap().clone();
        assert_eq!(not["range"], json!([start, start + 5]));
        assert_eq!(not["preview"], "\ty := !(x + 1)");
        run(&registry, &mut session, "editor.surround.apply", json!({ "name": "not" }));
        assert!(text(&session).contains("\ty := !(x + 1)\n"), "{}", text(&session));
        assert_eq!(selections(&session), [(start + 8, start + 8)]);
    }

    #[test]
    fn live_templates_with_selection_come_last_and_alone_for_templates_only() {
        let (registry, mut session, _dir) = open("main.go", BODY);
        let wrap = json!({ "language": "go", "abbreviation": "defer", "description": "Run later", "text": "defer func() {\n\t$SELECTION$\n}()$END$" });
        let plain = json!({ "language": "go", "abbreviation": "main", "text": "func main() {\n\t$END$\n}" });
        run(&registry, &mut session, "editor.templates.set", json!({ "template": wrap }));
        run(&registry, &mut session, "editor.templates.set", json!({ "template": plain }));
        let caret = BODY.find("println").unwrap();
        select(&registry, &mut session, caret, caret);
        let all = run(&registry, &mut session, "editor.surround.list", json!({}));
        assert_eq!(names(&all), ["if", "ifElse", "editor-fold", "region", "defer"]);
        assert_eq!(all["items"][4]["template"], true);
        let only =
            run(&registry, &mut session, "editor.surround.list", json!({ "templatesOnly": true, "preview": true }));
        assert_eq!(names(&only), ["defer"]);
        assert_eq!(only["items"][0]["mnemonic"], "1");
        assert_eq!(only["items"][0]["description"], "Run later");
        assert_eq!(only["items"][0]["preview"], "\tdefer func() {\n\t\tprintln(y)\n\t}()");
        run(&registry, &mut session, "editor.surround.apply", json!({ "name": "defer", "template": true }));
        let done = text(&session);
        assert!(done.contains("\tdefer func() {\n\t\tprintln(y)\n\t}()\n}"), "{done}");
        let end = done.find("}()").unwrap() + 3;
        assert_eq!(selections(&session), [(end, end)]);
        assert!(
            registry
                .dispatch(&mut session, "editor.surround.apply", json!({ "name": "if", "template": true }))
                .is_err(),
            "if is no live template"
        );
    }

    #[test]
    fn a_file_without_comments_or_surrounders_has_nothing_to_surround_with() {
        let (registry, mut session, _dir) = open("notes.txt", "a\nb\n");
        let list = run(&registry, &mut session, "editor.surround.list", json!({}));
        assert_eq!(list["items"], json!([]));
        assert!(registry.dispatch(&mut session, "editor.surround.apply", json!({ "name": "region" })).is_err());
        let (registry, mut session, _dir) = open("page.html", "<p>\n  hi\n</p>\n");
        select(&registry, &mut session, 6, 6);
        run(&registry, &mut session, "editor.surround.apply", json!({ "name": "editor-fold" }));
        assert_eq!(
            text(&session),
            "<p>\n  <!-- <editor-fold desc=\"Description\"> -->\n  hi\n  <!-- </editor-fold> -->\n</p>\n"
        );
    }

    #[test]
    fn the_mnemonics_go_from_digits_to_letters() {
        assert_eq!([mnemonic(0), mnemonic(8), mnemonic(9), mnemonic(10), mnemonic(35)], ["1", "9", "0", "A", "Z"]);
        assert_eq!(mnemonic(36), "");
    }

    fn statement(text: &str, at: usize) -> &str {
        let skipped = literals(text);
        &text[Code::new(text, &skipped).statement_at(at)]
    }

    #[test]
    fn a_statement_ends_where_its_brackets_close() {
        let text = "\tcall(a,\n\t\tb)\n\tnext()\n";
        assert_eq!(statement(text, 2), "call(a,\n\t\tb)");
        assert_eq!(statement(text, text.find("b)").unwrap()), "call(a,\n\t\tb)", "from inside the brackets");
        let text = "\tx := `{\n{`\n\ty()\n";
        assert_eq!(statement(text, 2), "x := `{\n{`");
        assert_eq!(statement(text, text.find("{`").unwrap()), "x := `{\n{`", "from inside the raw string");
        assert_eq!(statement("a\n\n\tb", 2), "");
    }

    #[test]
    fn a_closing_line_or_a_literal_takes_the_whole_statement() {
        let text = "func f() {\n\tif a {\n\t\tb()\n\t} else {\n\t\tc()\n\t}\n\tx := T{\n\t\tA: 1,\n\t}\n}\n";
        let chain = "if a {\n\t\tb()\n\t} else {\n\t\tc()\n\t}";
        assert_eq!(statement(text, text.find("} else").unwrap()), chain);
        assert_eq!(statement(text, text.find("\t}\n\tx").unwrap() + 1), chain);
        assert_eq!(statement(text, text.find("b()").unwrap()), "b()");
        assert_eq!(statement(text, text.find("A: 1").unwrap()), "x := T{\n\t\tA: 1,\n\t}");
        let text = "\tv := a +\n\t\tb\n\tw := c.\n\t\td()\n\ti++\n\tz := s.\n\t\tFoo()\n";
        assert_eq!(statement(text, 2), "v := a +\n\t\tb");
        assert_eq!(statement(text, text.find("i++").unwrap()), "i++");
        assert_eq!(statement(text, text.find("Foo").unwrap()), "z := s.\n\t\tFoo()");
        assert_eq!(statement(text, text.find("d()").unwrap()), "w := c.\n\t\td()");
    }

    #[test]
    fn a_selection_grows_to_the_whole_statements_it_touches() {
        let text = "func f() {\n\tif a {\n\t\tb()\n\t}\n\tc()\n}\n";
        let skipped = literals(text);
        let code = Code::new(text, &skipped);
        let from = text.find("b()").unwrap();
        let to = text.find("c()").unwrap() + 1;
        assert_eq!(&text[code.target(from..to, false)], "if a {\n\t\tb()\n\t}\n\tc()");
        assert_eq!(&text[code.target(from..to, true)], "b()\n\t}\n\tc", "an expression is taken as it is");
    }
}
