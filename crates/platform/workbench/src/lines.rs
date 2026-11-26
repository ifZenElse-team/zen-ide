use schemars::JsonSchema;
use serde::Deserialize;
use zen_core::{Command, CommandError, NoParams, Registry, RegistryError, Session};
use zen_editor::lines;
use zen_i18n::t;

use crate::editing::SelectionInfo;
use crate::{Document, Workbench, project_settings};

pub(crate) fn register(registry: &mut Registry) -> Result<(), RegistryError> {
    registry.register(LineAction::<0>)?;
    registry.register(LineAction::<1>)?;
    registry.register(LineAction::<2>)?;
    registry.register(LineAction::<3>)?;
    registry.register(LineAction::<4>)?;
    registry.register(LineAction::<5>)?;
    registry.register(LineAction::<6>)?;
    registry.register(LineAction::<7>)?;
    registry.register(LineComment)?;
    registry.register(BlockComment)?;
    registry.register(TypeText)?;
    registry.register(Tab)?;
    registry.register(KeepPrimary)?;
    Ok(())
}

fn document(session: &mut Session) -> Result<&mut Document, CommandError> {
    session
        .state_mut::<Workbench>()
        .active_document_mut()
        .filter(|document| document.image().is_none())
        .ok_or_else(|| CommandError::new(t!("workbench-error-no-document")))
}

fn indent_unit(session: &Session) -> String {
    session
        .state::<Workbench>()
        .and_then(Workbench::active_document)
        .map_or_else(|| "    ".to_owned(), |document| project_settings::indent_unit_of(session, document))
}

fn continuation_unit(session: &Session) -> Option<String> {
    let document = session.state::<Workbench>().and_then(Workbench::active_document)?;
    let style = project_settings::file_code_style(session, document.path());
    let columns = usize::from(style.continuation_indent.filter(|columns| *columns > 0)?);
    Some(if style.use_tabs {
        let width = usize::from(style.tab_size.max(1));
        format!("{}{}", "\t".repeat(columns / width), " ".repeat(columns % width))
    } else {
        " ".repeat(columns)
    })
}

fn has_text_document(session: &Session) -> bool {
    session.state::<Workbench>().and_then(Workbench::active_document).is_some_and(|document| document.image().is_none())
}

fn report(document: &Document) -> Vec<SelectionInfo> {
    crate::editing::describe_selections(document)
}

pub struct LineAction<const K: u8>;

impl<const K: u8> Command for LineAction<K> {
    type Params = NoParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = match K {
        0 => "editor.line.duplicate",
        1 => "editor.line.delete",
        2 => "editor.line.moveUp",
        3 => "editor.line.moveDown",
        4 => "editor.line.indent",
        5 => "editor.line.outdent",
        6 => "editor.edit.newline",
        _ => "editor.edit.backspace",
    };
    const TITLE: &'static str = match K {
        0 => "Duplicate Line or Selection",
        1 => "Delete Line",
        2 => "Move Line Up",
        3 => "Move Line Down",
        4 => "Indent Selection",
        5 => "Unindent Line or Selection",
        6 => "Enter",
        _ => "Backspace",
    };
    const DESCRIPTION: &'static str = match K {
        0 => "Repeats the selection after itself, or the caret's line below it, at every caret (⌘D).",
        1 => "Deletes the lines the carets and selections are on (⌘⌫).",
        2 => "Moves the lines the carets and selections are on one line up (⌥⇧↑).",
        3 => "Moves the lines the carets and selections are on one line down (⌥⇧↓).",
        4 => "Indents the lines the selections cover by the file's indentation (Tab).",
        5 => {
            "Unindents the lines the carets and selections are on (⇧Tab); in a live template being filled in, goes to the previous variable."
        }
        6 => {
            "Starts a new line at every caret, keeping the indentation, indenting after an opening bracket \
              and moving a closing bracket to its own line; in a live template being filled in, goes to the next variable as Tab does; \
              after the opening of a doc comment whose language puts its stub on Enter, puts that stub."
        }
        _ => "Deletes the character before every caret, or both characters of an empty bracket or quote pair.",
    };

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<SelectionInfo>, CommandError> {
        if (K == 5 && crate::templates::step(session, false)) || (K == 6 && crate::templates::step(session, true)) {
            return Ok(report(document(session)?));
        }
        if K == 6 && crate::templates::doc_comments::on_enter(session)? {
            return Ok(report(document(session)?));
        }
        let unit = indent_unit(session);
        let continuation = continuation_unit(session).unwrap_or_else(|| unit.clone());
        let document = document(session)?;
        let text = document.snapshot().clone();
        let selections = document.selections().clone();
        let result = match K {
            0 => Some(lines::duplicate(&text, &selections)),
            1 => Some(lines::delete_lines(&text, &selections)),
            2 => lines::move_lines(&text, &selections, false),
            3 => lines::move_lines(&text, &selections, true),
            4 => Some(lines::indent(&text, &selections, &unit, false)),
            5 => Some(lines::indent(&text, &selections, &unit, true)),
            6 => Some(lines::smart_newline_continuing(&text, &selections, &unit, &continuation)),
            _ => lines::delete_pair(&text, &selections),
        };
        match result {
            Some((edits, selections)) => {
                document.apply_action(edits, selections);
            }
            None if K == 7 => {
                document.apply_op(&zen_editor::EditOp::DeleteBackward);
            }
            None => {}
        }
        Ok(report(document))
    }
}

/// A comment prefix, when the file type is not known or another one is wanted.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct LineCommentParams {
    /// The line comment prefix, such as `//` or `#`; the file type's when absent.
    pub prefix: Option<String>,
}

pub struct LineComment;

impl Command for LineComment {
    type Params = LineCommentParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.comment.line";
    const TITLE: &'static str = "Comment with Line Comment";
    const DESCRIPTION: &'static str = "Comments the lines the carets and selections are on with the file type's line comment, \
         or uncomments them when they all are commented (⌘/).";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: LineCommentParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let document = document(session)?;
        let syntax = zen_syntax::comment_syntax(&document.name());
        let prefix = params.prefix.or_else(|| syntax.and_then(|syntax| syntax.line).map(str::to_owned));
        let Some(prefix) = prefix else { return Err(CommandError::new(t!("workbench-error-no-comment"))) };
        let (edits, selections) = lines::toggle_line_comment(document.snapshot(), document.selections(), &prefix);
        document.apply_action(edits, selections);
        Ok(report(document))
    }
}

/// Block comment delimiters, when the file type's are not wanted.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema)]
#[serde(default)]
pub struct BlockCommentParams {
    /// The opening, such as `/*`.
    pub open: Option<String>,
    /// The closing, such as `*/`.
    pub close: Option<String>,
}

pub struct BlockComment;

impl Command for BlockComment {
    type Params = BlockCommentParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.comment.block";
    const TITLE: &'static str = "Comment with Block Comment";
    const DESCRIPTION: &'static str =
        "Wraps every selection in the file type's block comment, or unwraps it when it is wrapped (⌥⌘/).";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: BlockCommentParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let document = document(session)?;
        let known = zen_syntax::comment_syntax(&document.name()).and_then(|syntax| syntax.block);
        let open = params.open.or_else(|| known.map(|(open, _)| open.to_owned()));
        let close = params.close.or_else(|| known.map(|(_, close)| close.to_owned()));
        let (Some(open), Some(close)) = (open, close) else {
            return Err(CommandError::new(t!("workbench-error-no-comment")));
        };
        let (edits, selections) =
            lines::toggle_block_comment(document.snapshot(), document.selections(), &open, &close);
        document.apply_action(edits, selections);
        Ok(report(document))
    }
}

pub struct Tab;

impl Command for Tab {
    type Params = NoParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.edit.tab";
    const TITLE: &'static str = "Tab";
    const DESCRIPTION: &'static str = "Goes to the next variable of a live template being filled in; else expands the \
         abbreviation before the caret with the file's plug-in (Emmet), a live template, or a postfix template typed \
         after an expression and a dot (`ok.if`); otherwise indents the selected \
         lines when a selection covers several lines, or types the indentation (a tab in Go).";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<SelectionInfo>, CommandError> {
        if crate::templates::step(session, true) {
            return Ok(report(document(session)?));
        }
        if let Some((path, caret)) = crate::plugins::emmet::tab_expands(session)
            && crate::plugins::emmet::expand(session, &path, caret)?
        {
            return Ok(report(document(session)?));
        }
        if crate::templates::expand_at_caret(session)? {
            return Ok(report(document(session)?));
        }
        let unit = indent_unit(session);
        let document = document(session)?;
        let text = document.snapshot().clone();
        let spans_lines = document.selections().iter().any(|selection| {
            let range = selection.range();
            text.offset_to_point(range.start).map(|p| p.line).ok()
                != text.offset_to_point(range.end).map(|p| p.line).ok()
        });
        if spans_lines {
            let (edits, selections) = lines::indent(&text, document.selections(), &unit, false);
            document.apply_action(edits, selections);
        } else {
            document.apply_op(&zen_editor::EditOp::Insert { text: unit });
        }
        Ok(report(document))
    }
}

pub struct KeepPrimary;

impl Command for KeepPrimary {
    type Params = NoParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.caret.keepPrimary";
    const TITLE: &'static str = "Remove Other Carets";
    const DESCRIPTION: &'static str = "Keeps only the primary caret and its selection (Escape with several carets); ends a live template being filled in and unpins the usages of Highlight Usages in File.";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, _: NoParams) -> Result<Vec<SelectionInfo>, CommandError> {
        crate::language::highlights::clear(session);
        if crate::templates::end(session, true) {
            return Ok(report(document(session)?));
        }
        let document = document(session)?;
        let mut selections = document.selections().clone();
        if selections.len() > 1 {
            selections.collapse_to_primary();
            document.set_selections(selections);
        }
        Ok(report(document))
    }
}

/// Typed text.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct TypeParams {
    /// What was typed: one character pairs brackets and quotes and may make the file's plug-in add
    /// something, such as an end tag after `>`; longer text is inserted as is.
    pub text: String,
}

pub struct TypeText;

impl Command for TypeText {
    type Params = TypeParams;
    type Output = Vec<SelectionInfo>;
    const ID: &'static str = "editor.edit.type";
    const TITLE: &'static str = "Type";
    const DESCRIPTION: &'static str = "Types text at every caret as the keyboard does: an opening bracket or quote gets its \
         closing one (or wraps the selection), a closing one steps over the same character; with a single caret, the \
         file's plug-in may add something, such as the end tag after `>` of an HTML start tag or the element's name \
         after `</`; typing the opening of a doc comment (`///` in Rust, `//` in Go) above a declaration puts the \
         language's stub.";

    fn is_available(&self, session: &Session) -> bool {
        has_text_document(session)
    }

    fn run(&self, session: &mut Session, params: TypeParams) -> Result<Vec<SelectionInfo>, CommandError> {
        let open = document(session)?;
        let mut chars = params.text.chars();
        let single = match (chars.next(), chars.next()) {
            (Some(c), None) => Some(c),
            _ => None,
        };
        let paired = single.and_then(|c| lines::type_paired(open.snapshot(), open.selections(), c));
        let typed = match paired {
            Some((edits, selections)) => open.apply_action(edits, selections),
            None => open.apply_op(&zen_editor::EditOp::Insert { text: params.text }),
        };
        if typed && let Some(c) = single {
            if !c.is_whitespace() {
                crate::wrap_on_typing::after_typing(session);
            }
            crate::plugins::after_typing(session, c);
            crate::templates::doc_comments::after_typing(session)?;
        }
        Ok(report(document(session)?))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::json;
    use zen_core::Registry;

    use super::*;
    use crate::code_style::{self, CodeStyle};

    fn open(name: &str, contents: &str) -> (Registry, Session, tempfile::TempDir) {
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(name);
        std::fs::write(&file, contents).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        (registry, session, dir)
    }

    fn text(session: &Session) -> String {
        session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
    }

    struct Continuation;

    impl zen_project::settings::SettingsProvider for Continuation {
        fn id(&self) -> &str {
            "continuation"
        }

        fn watches(&self, _: &std::path::Path) -> bool {
            false
        }

        fn load(
            &self,
            _: &std::path::Path,
            _: &dyn zen_project::settings::ReadFiles,
        ) -> Option<zen_project::settings::ProjectSettings> {
            let mut project = zen_project::settings::ProjectSettings::default();
            let style = zen_project::settings::CodeStyleOverride {
                indent: Some(4),
                use_tabs: Some(false),
                continuation_indent: Some(8),
                ..Default::default()
            };
            project.code_style.insert("java".to_owned(), style);
            Some(project)
        }
    }

    #[test]
    fn enter_after_a_parenthesis_takes_the_continuation_indent_the_project_sets() {
        let (registry, mut session, dir) = open("A.java", "foo(");
        let state = session.state_mut::<project_settings::ProjectSettingsState>();
        state.set_providers(vec![std::sync::Arc::new(Continuation)]);
        state.refresh(Some(dir.path()));
        set_caret(&registry, &mut session, 4, 4);
        registry.dispatch(&mut session, "editor.edit.newline", json!({})).unwrap();
        assert_eq!(text(&session), "foo(\n        ");
    }

    #[test]
    fn wrap_on_typing_moves_the_word_past_the_margin_to_a_new_line() {
        let (registry, mut session, _dir) = open("a.txt", "  one two");
        session.settings_mut().set(code_style::HARD_WRAP_AT, 9).unwrap();
        set_caret(&registry, &mut session, 9, 9);
        registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "x" })).unwrap();
        assert_eq!(text(&session), "  one twox", "off by default");
        session.settings_mut().set(code_style::WRAP_ON_TYPING, true).unwrap();
        registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "y" })).unwrap();
        assert_eq!(text(&session), "  one\n  twoxy");
        registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "z" })).unwrap();
        assert_eq!(text(&session), "  one\n  twoxyz", "the caret went along");
    }

    fn set_caret(registry: &Registry, session: &mut Session, anchor: usize, head: usize) {
        registry
            .dispatch(session, "editor.selection.set", json!({ "selections": [{ "anchor": anchor, "head": head }] }))
            .unwrap();
    }

    fn configure(session: &mut Session, language: &str, style: CodeStyle) {
        let mut styles = BTreeMap::new();
        styles.insert(language.to_owned(), style);
        code_style::apply(session.settings_mut(), &styles);
    }

    #[test]
    fn tab_types_the_configured_unit() {
        let (registry, mut session, _dir) = open("a.json", "{}\n");
        set_caret(&registry, &mut session, 1, 1);
        registry.dispatch(&mut session, "editor.edit.tab", json!({})).unwrap();
        assert_eq!(text(&session), "{  }\n", "JSON indents by two spaces");
        configure(&mut session, "json", CodeStyle { use_tabs: false, tab_size: 4, indent: 3 });
        registry.dispatch(&mut session, "editor.edit.tab", json!({})).unwrap();
        assert_eq!(text(&session), "{     }\n");
    }

    #[test]
    fn enter_after_a_bracket_indents_by_the_configured_unit() {
        let (registry, mut session, _dir) = open("main.go", "func f() {}\n");
        set_caret(&registry, &mut session, 10, 10);
        registry.dispatch(&mut session, "editor.edit.newline", json!({})).unwrap();
        assert_eq!(text(&session), "func f() {\n\t\n}\n", "Go takes a tab");
        let (registry, mut session, _dir) = open("main.go", "func f() {}\n");
        configure(&mut session, "go", CodeStyle { use_tabs: false, tab_size: 4, indent: 2 });
        set_caret(&registry, &mut session, 10, 10);
        registry.dispatch(&mut session, "editor.edit.newline", json!({})).unwrap();
        assert_eq!(text(&session), "func f() {\n  \n}\n");
    }

    #[test]
    fn indent_and_unindent_use_the_configured_unit() {
        let (registry, mut session, _dir) = open("lib.rs", "a\nb\n");
        configure(&mut session, code_style::DEFAULT_LANGUAGE, CodeStyle { use_tabs: false, tab_size: 2, indent: 2 });
        set_caret(&registry, &mut session, 0, 3);
        registry.dispatch(&mut session, "editor.line.indent", json!({})).unwrap();
        assert_eq!(text(&session), "  a\n  b\n");
        registry.dispatch(&mut session, "editor.line.outdent", json!({})).unwrap();
        assert_eq!(text(&session), "a\nb\n");
        configure(&mut session, "rust", CodeStyle { use_tabs: true, tab_size: 4, indent: 4 });
        registry.dispatch(&mut session, "editor.line.indent", json!({})).unwrap();
        assert_eq!(text(&session), "\ta\n\tb\n");
    }
}
