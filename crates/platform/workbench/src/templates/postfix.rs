use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use super::{Context, LiveTemplates, SessionInfo, Template, Variable, context_at, document_at, insert, is_word};
use crate::code_style;
use crate::language::{byte_offset, char_offset};

pub const EXPR: &str = "$EXPR$";

/// A postfix template: typed after an expression and a dot, it replaces both, e.g. `ok.if`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PostfixTemplate {
    /// The language id of the files it is for, e.g. `go`.
    pub language: String,
    /// What is typed after the dot, e.g. `if`.
    pub key: String,
    /// What it is for: a message id of the module or plug-in that declares it, or the text itself.
    #[serde(default)]
    pub description: String,
    /// The text, a live template where `$EXPR$` is the expression before the dot.
    pub text: String,
    /// The variables' default values and expressions, as in a live template.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<Variable>,
    /// The packages the text uses, imported into the file when it is put in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub imports: Vec<String>,
}

impl PostfixTemplate {
    pub fn parse_list(json: &str) -> Result<Vec<PostfixTemplate>, String> {
        serde_json::from_str(json).map_err(|error| error.to_string())
    }

    pub fn id(&self) -> String {
        format!("{}/{}", self.language, self.key)
    }

    pub fn same_key(&self, language: &str, key: &str) -> bool {
        self.language == language && self.key == key
    }

    pub fn description_text(&self) -> String {
        if self.description.is_empty() { String::new() } else { zen_i18n::text(&self.description, &[]) }
    }

    pub fn applied_to(&self, expression: &str) -> Template {
        Template {
            language: self.language.clone(),
            abbreviation: self.key.clone(),
            description: self.description.clone(),
            text: self.text.replace(EXPR, &expression.replace('$', "$$")),
            variables: self.variables.clone(),
            imports: self.imports.clone(),
            contexts: Vec::new(),
            enabled: true,
        }
    }
}

/// A postfix template as `editor.postfix.list` gives it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PostfixInfo {
    /// The template as declared.
    pub template: PostfixTemplate,
    /// Its description in the interface's language.
    pub description_text: String,
    /// Whether it is offered.
    pub enabled: bool,
    /// The plug-in that declares it; a language module when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
}

fn before(text: &str, at: usize) -> Option<char> {
    text[..at].chars().next_back()
}

fn quote_start(text: &str, end: usize, quote: char) -> Option<usize> {
    let mut at = end - quote.len_utf8();
    loop {
        let found = text[..at].rfind(quote)?;
        let escaped = text[..found].chars().rev().take_while(|c| *c == '\\').count() % 2 == 1;
        if !escaped {
            return Some(found);
        }
        at = found;
    }
}

fn group_start(text: &str, end: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut at = end;
    while let Some(c) = before(text, at) {
        match c {
            ')' | ']' | '}' => depth += 1,
            '(' | '[' | '{' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(at - 1);
                }
            }
            '"' | '`' => {
                at = quote_start(text, at, c)?;
                continue;
            }
            _ => {}
        }
        at -= c.len_utf8();
    }
    None
}

fn word_start(text: &str, end: usize) -> usize {
    end - text[..end].chars().rev().take_while(|c| is_word(*c)).map(char::len_utf8).sum::<usize>()
}

pub fn expression_before(text: &str, dot: usize) -> Option<usize> {
    let mut at = dot;
    loop {
        while before(text, at) == Some('?') {
            at -= 1;
        }
        match before(text, at)? {
            ')' | ']' | '}' => {
                at = group_start(text, at)?;
                if before(text, at) == Some('!') && before(text, at - 1).is_some_and(is_word) {
                    at -= 1;
                }
                at = word_start(text, at);
            }
            quote @ ('"' | '`') => at = quote_start(text, at, quote)?,
            c if is_word(c) => at = word_start(text, at),
            _ => return None,
        }
        if before(text, at) == Some('.') && before(text, at - 1).is_some_and(|c| c != '.' && !c.is_whitespace()) {
            at -= 1;
        } else if text[..at].ends_with("::") {
            at -= 2;
        } else {
            break;
        }
    }
    (at < dot && !text[at..dot].trim().is_empty()).then_some(at)
}

pub struct Offered {
    pub template: Template,
    pub expression_start: usize,
}

pub fn offered_at(text: &str, caret: usize, templates: &[PostfixTemplate]) -> Vec<Offered> {
    let key_start = word_start(text, caret);
    if before(text, key_start) != Some('.') || text[caret..].chars().next().is_some_and(is_word) {
        return Vec::new();
    }
    let dot = key_start - 1;
    let Some(start) = expression_before(text, dot) else { return Vec::new() };
    let expression = &text[start..dot];
    templates
        .iter()
        .map(|template| Offered { template: template.applied_to(expression), expression_start: start })
        .collect()
}

pub(crate) fn postfix_of(session: &Session, path: &Path) -> Vec<PostfixTemplate> {
    session
        .state::<LiveTemplates>()
        .map(|templates| templates.postfix_for(&code_style::language_of(path)))
        .unwrap_or_default()
}

pub(crate) fn offered(session: &Session, path: &Path, text: &str, caret: usize) -> Vec<Offered> {
    let templates = postfix_of(session, path);
    if templates.is_empty() {
        return Vec::new();
    }
    let offered = offered_at(text, caret, &templates);
    if offered.is_empty() {
        return offered;
    }
    let in_code = session
        .state::<crate::Workbench>()
        .and_then(|workbench| workbench.documents().iter().find(|document| document.path() == path))
        .is_none_or(|document| {
            let caret = char_offset(text, caret);
            context_at(session, document, caret.saturating_sub(1), caret) == Context::Code
        });
    if in_code { offered } else { Vec::new() }
}

pub(crate) fn expand_at_caret(session: &mut Session, place: Context) -> Result<bool, CommandError> {
    if place != Context::Code {
        return Ok(false);
    }
    let Some(document) = session.state::<crate::Workbench>().and_then(crate::Workbench::active_document) else {
        return Ok(false);
    };
    let (path, text) = (document.path().to_owned(), document.text().to_owned());
    let caret = byte_offset(&text, document.selections().primary().head);
    let key = &text[word_start(&text, caret)..caret];
    let chosen = offered(session, &path, &text, caret).into_iter().find(|offered| offered.template.abbreviation == key);
    let Some(chosen) = chosen else { return Ok(false) };
    let range = char_offset(&text, chosen.expression_start)..char_offset(&text, caret);
    insert(session, Some(&path), range, &chosen.template)?;
    Ok(true)
}

/// The parameters of `editor.postfix.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ListParams {
    /// Only this language's postfix templates, e.g. `go`.
    pub language: Option<String>,
}

/// What `editor.postfix.list` gives.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PostfixList {
    /// Whether postfix completion is on at all.
    pub enabled: bool,
    /// The postfix templates of the language modules and plug-ins.
    pub templates: Vec<PostfixInfo>,
}

pub struct ListPostfix;

impl Command for ListPostfix {
    type Params = ListParams;
    type Output = PostfixList;
    const ID: &'static str = "editor.postfix.list";
    const TITLE: &'static str = "Postfix Templates";
    const DESCRIPTION: &'static str = "The postfix templates the language modules and plug-ins declare (`expr.if` and \
         the like), each with whether it is offered, and whether postfix completion is on.";

    fn run(&self, session: &mut Session, params: ListParams) -> Result<PostfixList, CommandError> {
        let Some(templates) = session.state::<LiveTemplates>() else { return Ok(PostfixList::default()) };
        let listed = templates
            .postfix_list()
            .into_iter()
            .filter(|info| params.language.as_ref().is_none_or(|language| info.template.language == *language))
            .collect();
        Ok(PostfixList { enabled: templates.postfix_enabled(), templates: listed })
    }
}

/// The parameters of `editor.postfix.set`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct SetParams {
    /// The language of the templates; with neither this nor `key`, postfix completion as a whole.
    #[serde(default)]
    pub language: Option<String>,
    /// The template's key, e.g. `if`; every template of the language when absent.
    #[serde(default)]
    pub key: Option<String>,
    /// Whether they are offered.
    pub enabled: bool,
}

pub struct SetPostfix;

impl Command for SetPostfix {
    type Params = SetParams;
    type Output = ();
    const ID: &'static str = "editor.postfix.set";
    const TITLE: &'static str = "Switch Postfix Template";
    const DESCRIPTION: &'static str = "Turns a postfix template, a language's postfix templates, or postfix completion \
         as a whole on or off; saved in templates.json.";

    fn run(&self, session: &mut Session, params: SetParams) -> Result<(), CommandError> {
        let templates = session.state_mut::<LiveTemplates>().clone();
        templates
            .set_postfix(params.language.as_deref(), params.key.as_deref(), params.enabled)
            .map_err(CommandError::new)
    }
}

/// The parameters of `editor.postfix.expand`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ExpandParams {
    /// The template's key, e.g. `if`.
    pub key: String,
    /// The file, open in the editor; the active one when absent.
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// A character offset right after `expression.key` or `expression.`; the primary caret when absent.
    #[serde(default)]
    pub offset: Option<usize>,
}

pub struct ExpandPostfix;

impl Command for ExpandPostfix {
    type Params = ExpandParams;
    type Output = SessionInfo;
    const ID: &'static str = "editor.postfix.expand";
    const TITLE: &'static str = "Expand Postfix Template";
    const DESCRIPTION: &'static str = "Replaces the expression, the dot and what is typed of the key before an offset \
         with a postfix template of the file's language and starts filling it in.";

    fn run(&self, session: &mut Session, params: ExpandParams) -> Result<SessionInfo, CommandError> {
        let document = document_at(session, params.path.as_deref())?;
        let (path, text) = (document.path().to_owned(), document.text().to_owned());
        let offset = params.offset.unwrap_or_else(|| document.selections().primary().head);
        let caret = byte_offset(&text, offset.min(text.chars().count()));
        let chosen = offered_at(&text, caret, &postfix_of(session, &path))
            .into_iter()
            .find(|offered| offered.template.abbreviation == params.key)
            .ok_or_else(|| CommandError::new(t!("workbench-postfix-none", name = params.key.as_str())))?;
        let range = char_offset(&text, chosen.expression_start)..char_offset(&text, caret);
        insert(session, Some(&path), range, &chosen.template)
    }
}

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListPostfix)?;
    registry.register(SetPostfix)?;
    registry.register(ExpandPostfix)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expression(line: &str) -> Option<&str> {
        let dot = line.rfind('.')?;
        expression_before(line, dot).map(|start| &line[start..dot])
    }

    #[test]
    fn the_expression_before_the_dot_is_the_whole_chain() {
        assert_eq!(expression("\tok."), Some("ok"));
        assert_eq!(expression("x := a.b(c, \"d)\").e[1]."), Some("a.b(c, \"d)\").e[1]"));
        assert_eq!(expression("let v = vec![1, 2]."), Some("vec![1, 2]"));
        assert_eq!(expression("std::env::args()?."), Some("std::env::args()?"));
        assert_eq!(expression("return \"a.b\"."), Some("\"a.b\""));
        assert_eq!(expression("if x == y."), Some("y"));
        assert_eq!(expression("\t."), None);
        assert_eq!(expression("f(a."), Some("a"));
        assert_eq!(expression("0..10."), Some("10"), "a range's end, not the range");
    }

    #[test]
    fn a_template_is_offered_after_a_dot_and_takes_the_expression() {
        let ifs = PostfixTemplate {
            language: "go".to_owned(),
            key: "if".to_owned(),
            description: String::new(),
            text: "if $EXPR$ {\n\t$END$\n}".to_owned(),
            variables: Vec::new(),
            imports: Vec::new(),
        };
        let text = "\tm[\"$k\"].i";
        let offered = offered_at(text, text.len(), std::slice::from_ref(&ifs));
        assert_eq!(offered.len(), 1);
        assert_eq!(offered[0].expression_start, 1);
        assert_eq!(offered[0].template.text, "if m[\"$$k\"] {\n\t$END$\n}", "a dollar of the code stays one");
        assert!(offered_at("x.ifx", 4, std::slice::from_ref(&ifs)).is_empty(), "inside a word");
        assert!(offered_at("x if", 4, &[ifs]).is_empty(), "no dot");
    }
}
