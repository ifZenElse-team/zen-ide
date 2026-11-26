use std::collections::HashMap;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use zen_core::{Command, CommandError, Registry, RegistryError, Session};
use zen_i18n::t;

use crate::folding::{document, has_text};

const HEADER_LOOKBACK: usize = 16;

pub const ENABLED: &str = "editor.stickyLines.enabled";
pub const MAX: &str = "editor.stickyLines.max";
pub const DEFAULT_MAX: usize = 5;
pub const MAX_LIMIT: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    pub start_line: usize,
    pub end_line: usize,
}

pub fn sticky_scopes(scopes: &[Scope], top_line: usize, max: usize, next_shown: impl Fn(usize) -> usize) -> Vec<Scope> {
    let mut stuck: Vec<Scope> = Vec::new();
    let mut under = top_line;
    for scope in scopes {
        if stuck.len() >= max || scope.start_line >= under {
            break;
        }
        if scope.end_line <= under {
            continue;
        }
        if stuck.last().is_some_and(|outer| scope.start_line == outer.start_line || scope.end_line > outer.end_line) {
            continue;
        }
        stuck.push(*scope);
        under = next_shown(under);
    }
    stuck
}

pub fn sticky_line<S: AsRef<str>>(
    start_line: usize,
    opening: Option<char>,
    line: impl Fn(usize) -> Option<S>,
) -> Option<usize> {
    if matches!(opening, Some('(' | '[')) {
        return None;
    }
    let first = line(start_line)?;
    let code = code_of(first.as_ref());
    let trimmed = code.trim_start();
    let comment = ["/*", "*", "<!--"].iter().any(|start| trimmed.starts_with(start));
    if trimmed.is_empty() || comment || starts_with_word(trimmed, "import") {
        return None;
    }
    let head = match code.strip_suffix('{') {
        Some(before) => {
            let spaced = before.ends_with(char::is_whitespace) || before.ends_with(')');
            let words = before.trim_end();
            let continues = ["=", "(", "[", ",", ":", "?", "&&", "||"].iter().any(|end| words.ends_with(end))
                || ["return", "import", "export"].iter().any(|word| ends_with_word(words, word));
            if !spaced || words.is_empty() || continues {
                return None;
            }
            before
        }
        None if code.ends_with(['(', '[', ',']) || balance(code) > 0 => return None,
        None => code,
    };
    let mut depth = balance(head);
    let mut header = start_line;
    for _ in 0..HEADER_LOOKBACK {
        let above = header.checked_sub(1).and_then(&line);
        let Some(above) = above else { break };
        let above = code_of(above.as_ref());
        if depth >= 0 && !above.ends_with("&&") && !above.ends_with("||") {
            break;
        }
        header -= 1;
        depth += balance(above);
    }
    Some(if depth < 0 { start_line } else { header })
}

fn code_of(line: &str) -> &str {
    let mut quote = None;
    let mut chars = line.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        match (quote, c) {
            (Some(_), '\\') => {
                chars.next();
            }
            (Some(open), _) if c == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'' | '`') => quote = Some(c),
            (None, '/') if chars.peek().is_some_and(|(_, next)| *next == '/') => return line[..at].trim_end(),
            _ => {}
        }
    }
    line.trim_end()
}

fn balance(code: &str) -> i32 {
    let mut quote = None;
    let mut depth = 0;
    let mut chars = code.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(_), '\\') => {
                chars.next();
            }
            (Some(open), _) if c == open => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'' | '`') => quote = Some(c),
            (None, '(' | '[' | '{') => depth += 1,
            (None, ')' | ']' | '}') => depth -= 1,
            _ => {}
        }
    }
    depth
}

fn starts_with_word(text: &str, word: &str) -> bool {
    text.strip_prefix(word).is_some_and(|rest| !rest.starts_with(|c: char| c.is_alphanumeric() || c == '_'))
}

fn ends_with_word(text: &str, word: &str) -> bool {
    text.strip_suffix(word).is_some_and(|rest| !rest.ends_with(|c: char| c.is_alphanumeric() || c == '_'))
}

pub(crate) fn fold_sticky_line(text: &zen_text::Text, fold: &zen_editor::Fold) -> Option<usize> {
    if fold.kind != zen_editor::FoldKind::Block {
        return None;
    }
    let start_line = text.offset_to_point(fold.range.start).ok()?.line;
    let opening = fold.range.start.checked_sub(1).and_then(|at| text.slice(at..at + 1).ok()?.chars().next());
    sticky_line(start_line, opening, |line| text.line(line).ok())
}

pub fn sort_scopes(scopes: &mut [Scope]) {
    scopes.sort_by_key(|scope| (scope.start_line, std::cmp::Reverse(scope.end_line)));
}

#[derive(Debug, Default)]
pub struct ShownTop(HashMap<PathBuf, usize>);

impl ShownTop {
    pub fn set(&mut self, path: &Path, line: usize) {
        if self.0.get(path) != Some(&line) {
            self.0.insert(path.to_owned(), line);
        }
    }

    pub fn get(&self, path: &Path) -> Option<usize> {
        self.0.get(path).copied()
    }
}

pub fn max_lines(settings: &zen_core::Settings) -> usize {
    settings.get_or(MAX, DEFAULT_MAX).clamp(1, MAX_LIMIT)
}

pub fn enabled(settings: &zen_core::Settings) -> bool {
    settings.get_or(ENABLED, true)
}

/// The parameters of `editor.stickyLines.list`.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ListParams {
    /// The line shown at the top of the view, from 0; the one the editor shows when absent.
    pub top_line: Option<usize>,
    /// How many lines may stick; the setting `editor.stickyLines.max` when absent.
    pub max: Option<usize>,
}

/// A line that sticks.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StickyLine {
    /// The line, from 0.
    pub line: usize,
    /// The line where its scope ends, from 0.
    pub end_line: usize,
    /// The line's text.
    pub text: String,
}

/// What `editor.stickyLines.list` answers.
#[derive(Clone, Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StickyLines {
    /// Whether the setting shows sticky lines; when off, `lines` is empty.
    pub enabled: bool,
    /// The line shown at the top of the view, from 0.
    pub top_line: usize,
    /// The lines that stick, outermost first.
    pub lines: Vec<StickyLine>,
}

type Hidden = Vec<(usize, usize)>;

fn scopes(session: &mut Session) -> Result<(Vec<Scope>, Hidden), CommandError> {
    let foldable = crate::folding::foldable(session, false)?;
    let document = document(session)?;
    let text = document.snapshot();
    let line = |offset: usize| text.offset_to_point(offset).map(|point| point.line).unwrap_or_default();
    let folded = document.folds();
    let mut scopes: Vec<Scope> = foldable
        .iter()
        .filter(|fold| !folded.contains(&fold.range))
        .filter_map(|fold| Some(Scope { start_line: fold_sticky_line(text, fold)?, end_line: line(fold.range.end) }))
        .filter(|scope| scope.end_line > scope.start_line)
        .collect();
    sort_scopes(&mut scopes);
    let mut hidden: Hidden = Vec::new();
    for range in folded {
        let (first, last) = (line(range.start), line(range.end));
        if last > first && hidden.last().is_none_or(|&(_, end)| first > end) {
            hidden.push((first, last));
        }
    }
    Ok((scopes, hidden))
}

pub struct ListStickyLines;

impl Command for ListStickyLines {
    type Params = ListParams;
    type Output = StickyLines;
    const ID: &'static str = "editor.stickyLines.list";
    const TITLE: &'static str = "Sticky Lines";
    const DESCRIPTION: &'static str = "The lines that stick over the top of the editor: the first lines of the scopes \
         the view is inside, outermost first, for the line the editor shows at the top (or another).";

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, params: ListParams) -> Result<StickyLines, CommandError> {
        let path = document(session)?.path().to_owned();
        let top_line =
            params.top_line.or_else(|| session.state::<ShownTop>().and_then(|shown| shown.get(&path))).unwrap_or(0);
        if !enabled(session.settings()) {
            return Ok(StickyLines { enabled: false, top_line, lines: Vec::new() });
        }
        let max = params.max.unwrap_or_else(|| max_lines(session.settings()));
        let (scopes, hidden) = scopes(session)?;
        let next_shown = |line: usize| {
            let at = hidden.partition_point(|&(first, _)| first < line);
            match hidden.get(at) {
                Some(&(first, last)) if first == line => last + 1,
                _ => line + 1,
            }
        };
        let stuck = sticky_scopes(&scopes, top_line, max, next_shown);
        let text = document(session)?.snapshot();
        let lines = stuck
            .iter()
            .map(|scope| StickyLine {
                line: scope.start_line,
                end_line: scope.end_line,
                text: text.line(scope.start_line).map(|line| line.into_owned()).unwrap_or_default(),
            })
            .collect();
        Ok(StickyLines { enabled: true, top_line, lines })
    }
}

/// The parameters of `editor.stickyLines.goTo`.
#[derive(Deserialize, JsonSchema)]
pub struct GoToParams {
    /// The line, from 0, as `editor.stickyLines.list` gives it.
    pub line: usize,
}

pub struct GoToStickyLine;

impl Command for GoToStickyLine {
    type Params = GoToParams;
    type Output = usize;
    const ID: &'static str = "editor.stickyLines.goTo";
    const TITLE: &'static str = "Go to Sticky Line";
    const DESCRIPTION: &'static str = "Moves the caret to the first character of a sticky line that is not a space, as a click on the line does; \
         returns the caret's character offset.";

    fn is_available(&self, session: &Session) -> bool {
        has_text(session)
    }

    fn run(&self, session: &mut Session, params: GoToParams) -> Result<usize, CommandError> {
        let document = document(session)?;
        let text = document.snapshot();
        let start = text
            .point_to_offset(zen_text::Point::new(params.line, 0))
            .map_err(|_| CommandError::new(t!("workbench-sticky-no-line", line = params.line + 1)))?;
        let indent =
            text.line(params.line).map_or(0, |line| line.chars().take_while(|c| *c == ' ' || *c == '\t').count());
        let caret = start + indent;
        document.set_selections(zen_editor::Selections::single(zen_editor::Selection::caret(caret)));
        Ok(caret)
    }
}

pub fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(ListStickyLines)?;
    registry.register(GoToStickyLine)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workbench;
    use serde_json::json;

    fn scope(start_line: usize, end_line: usize) -> Scope {
        Scope { start_line, end_line }
    }

    fn lines(stuck: &[Scope]) -> Vec<usize> {
        stuck.iter().map(|scope| scope.start_line).collect()
    }

    #[test]
    fn the_scopes_around_the_top_line_stick_outermost_first() {
        let scopes = [scope(0, 20), scope(2, 10), scope(4, 8), scope(12, 18)];
        let next = |line: usize| line + 1;
        assert_eq!(lines(&sticky_scopes(&scopes, 0, 5, next)), Vec::<usize>::new(), "the first line is shown");
        assert_eq!(lines(&sticky_scopes(&scopes, 1, 5, next)), [0]);
        assert_eq!(lines(&sticky_scopes(&scopes, 2, 5, next)), [0, 2], "the line under the first slot sticks too");
        assert_eq!(lines(&sticky_scopes(&scopes, 5, 5, next)), [0, 2, 4]);
        assert_eq!(lines(&sticky_scopes(&scopes, 8, 5, next)), [0, 2]);
        assert_eq!(lines(&sticky_scopes(&scopes, 9, 5, next)), [0], "a scope ending under its slot does not stick");
        assert_eq!(lines(&sticky_scopes(&scopes, 11, 5, next)), [0], "a sibling that ended stays away");
        assert_eq!(lines(&sticky_scopes(&scopes, 14, 5, next)), [0, 12]);
        assert_eq!(lines(&sticky_scopes(&scopes, 25, 5, next)), Vec::<usize>::new());
    }

    #[test]
    fn at_most_max_lines_stick() {
        let scopes: Vec<Scope> = (0..8).map(|depth| scope(depth, 100 - depth)).collect();
        assert_eq!(lines(&sticky_scopes(&scopes, 50, 5, |line| line + 1)), [0, 1, 2, 3, 4]);
        assert_eq!(lines(&sticky_scopes(&scopes, 50, 2, |line| line + 1)), [0, 1]);
        assert_eq!(sticky_scopes(&scopes, 50, 0, |line| line + 1), []);
    }

    #[test]
    fn folded_lines_are_skipped_under_the_slots() {
        let scopes = [scope(0, 20), scope(4, 8)];
        let folded = |line: usize| if line == 1 { 3 } else { line + 1 };
        assert_eq!(lines(&sticky_scopes(&scopes, 1, 5, |line| line + 1)), [0]);
        assert_eq!(lines(&sticky_scopes(&scopes, 1, 5, folded)), [0], "under the first slot is line 3");
        assert_eq!(lines(&sticky_scopes(&scopes, 3, 5, folded)), [0]);
        let skips_into = |line: usize| if line == 1 { 5 } else { line + 1 };
        assert_eq!(lines(&sticky_scopes(&scopes, 1, 5, skips_into)), [0, 4], "line 5, inside the inner scope");
    }

    fn sticks(text: &str, start_line: usize, opening: char) -> Option<usize> {
        let lines: Vec<&str> = text.lines().collect();
        sticky_line(start_line, Some(opening), |line| lines.get(line).copied())
    }

    #[test]
    fn blocks_stick_and_calls_literals_and_parameter_lists_do_not() {
        for line in [
            "func (t *T) Run() error {",
            "type T struct {",
            "type I interface {",
            "\tif err != nil {",
            "\t} else if x {",
            "\tfor _, x := range []int{1, 2} {",
            "\tswitch x := y.(type) {",
            "\tselect {",
            "\tgo func() {",
            "\tfoo(func() { // later",
            "class A extends B {",
            "  async run(a, b) {",
            "  items.forEach((item) => {",
            "if(x){",
            ".a:hover {",
            "<div class=\"a(b\">",
            "key:",
        ] {
            assert_eq!(sticks(line, 0, '{'), Some(0), "{line}");
        }
        for (line, opening) in [
            ("\tx := T{", '{'),
            ("\treturn &T{", '{'),
            ("\tm := map[string]int{", '{'),
            ("\tfoo(", '('),
            ("\tfoo(a,", '('),
            ("\tbar(a, []int{1,", '{'),
            ("\tlist := []int{", '{'),
            ("const x = {", '{'),
            ("  return {", '{'),
            ("  foo({", '{'),
            ("  a: {", '{'),
            ("import {", '{'),
            ("export {", '{'),
            ("{", '{'),
            ("/**", '*'),
            ("import a from \"a\";", ';'),
            ("\tvalues := []T{", '['),
            ("\t}, {", '{'),
        ] {
            assert_eq!(sticks(line, 0, opening), None, "{line}");
        }
    }

    #[test]
    fn a_block_after_a_long_header_sticks_with_the_header_s_first_line() {
        let text = "func run(\n\tctx context.Context,\n\tname string, // \")\"\n) error {\n\tif a &&\n\t\tb(\")\") {\n";
        assert_eq!(sticks(text, 3, '{'), Some(0), "the parameter list's first line");
        assert_eq!(sticks(text, 5, '{'), Some(4), "the condition's first line");
        let unbalanced = ") {\n";
        assert_eq!(sticks(unbalanced, 0, '{'), Some(0), "no header above: the line itself");
    }

    #[test]
    fn fold_list_tells_which_folds_stick() {
        let text = "package main\n\nfunc run(\n\ta int,\n) {\n\tx := T{\n\t\tA: 1,\n\t}\n\tfoo(\n\t\t1,\n\t)\n}\n";
        let (registry, mut session, _folder) = session_with(text);
        let folds = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        let sticky: Vec<(u64, Option<u64>)> = folds
            .as_array()
            .unwrap()
            .iter()
            .map(|fold| (fold["startLine"].as_u64().unwrap(), fold["stickyLine"].as_u64()))
            .collect();
        assert_eq!(sticky, [(2, None), (4, Some(2)), (5, None), (8, None)], "{folds}");
        assert_eq!(listed(&registry, &mut session, json!({ "topLine": 9 })), [(2, "func run(".to_owned())]);
    }

    const GO: &str = "package main\n\ntype T struct{}\n\nfunc (t T) run() {\n\tif true {\n\t\tfor {\n\t\t\tx()\n\t\t\tx()\n\t\t}\n\t}\n}\n\nfunc other() {\n\ty()\n}\n";

    fn session_with(text: &str) -> (Registry, Session, tempfile::TempDir) {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("main.go");
        std::fs::write(&path, text).unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": folder.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        (registry, session, folder)
    }

    fn listed(registry: &Registry, session: &mut Session, params: serde_json::Value) -> Vec<(u64, String)> {
        let answer = registry.dispatch(session, "editor.stickyLines.list", params).unwrap();
        answer["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|line| (line["line"].as_u64().unwrap(), line["text"].as_str().unwrap().to_owned()))
            .collect()
    }

    #[test]
    fn the_commands_list_the_lines_and_go_to_one() {
        let (registry, mut session, _folder) = session_with(GO);
        assert_eq!(
            listed(&registry, &mut session, json!({ "topLine": 6 })),
            [(4, "func (t T) run() {".to_owned()), (5, "\tif true {".to_owned()), (6, "\t\tfor {".to_owned())]
        );
        assert_eq!(listed(&registry, &mut session, json!({ "topLine": 6, "max": 1 })).len(), 1);
        assert_eq!(listed(&registry, &mut session, json!({ "topLine": 14 })), [(13, "func other() {".to_owned())]);
        let path = session.state::<Workbench>().unwrap().active_document().unwrap().path().to_owned();
        session.state_mut::<ShownTop>().set(&path, 7);
        let shown = registry.dispatch(&mut session, "editor.stickyLines.list", json!({})).unwrap();
        assert_eq!(shown["topLine"], 7, "the line the editor shows at the top");
        assert_eq!(shown["lines"].as_array().unwrap().len(), 2);
        let caret = registry.dispatch(&mut session, "editor.stickyLines.goTo", json!({ "line": 5 })).unwrap();
        assert_eq!(caret, json!(GO.find("if true").unwrap()));
        assert!(registry.dispatch(&mut session, "editor.stickyLines.goTo", json!({ "line": 99 })).is_err());
    }

    #[test]
    fn folded_scopes_do_not_stick_and_the_setting_hides_them() {
        let (registry, mut session, _folder) = session_with(GO);
        let folds = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
        let inner = folds.as_array().unwrap().iter().find(|fold| fold["startLine"] == 6).unwrap()["start"].clone();
        registry.dispatch(&mut session, "editor.fold.toggle", json!({ "start": inner })).unwrap();
        let starts = |registry: &Registry, session: &mut Session, top: usize| -> Vec<u64> {
            listed(registry, session, json!({ "topLine": top })).iter().map(|(line, _)| *line).collect()
        };
        assert_eq!(starts(&registry, &mut session, 5), [4, 5], "the folded `for` does not stick");
        assert_eq!(starts(&registry, &mut session, 6), [4], "under the first slot is the end of `if`");
        session.settings_mut().set(ENABLED, false).unwrap();
        let off = registry.dispatch(&mut session, "editor.stickyLines.list", json!({ "topLine": 7 })).unwrap();
        assert_eq!(off["enabled"], false);
        assert_eq!(off["lines"], json!([]));
    }

    #[test]
    fn the_settings_limit_the_lines() {
        let (registry, mut session, _folder) = session_with(GO);
        session.settings_mut().set(MAX, 2).unwrap();
        assert_eq!(listed(&registry, &mut session, json!({ "topLine": 6 })).len(), 2);
        session.settings_mut().set(MAX, 0).unwrap();
        assert_eq!(max_lines(session.settings()), 1);
        session.settings_mut().set(MAX, 99).unwrap();
        assert_eq!(max_lines(session.settings()), MAX_LIMIT);
    }
}
