use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_editor::{Selection, Selections};
use zen_text::Text;

use super::{ListKind, Output, Style, heading, heading_level, link, list, toggle_style};
use crate::Workbench;

fn apply(
    text: &str,
    ranges: &[(usize, usize)],
    change: impl Fn(&Text, &Selections) -> Output,
) -> (String, Vec<(usize, usize)>) {
    let source = Text::from(text);
    let items: Vec<Selection> = ranges.iter().map(|(anchor, head)| Selection::new(*anchor, *head)).collect();
    let selections = Selections::from_vec(items, 0);
    let (edits, after) = change(&source, &selections);
    let (source, _) = source.apply(&edits).unwrap();
    let ranges = after.iter().map(|selection| (selection.range().start, selection.range().end)).collect();
    (source.to_string(), ranges)
}

fn styled(text: &str, range: (usize, usize), style: Style) -> (String, (usize, usize)) {
    let (text, ranges) = apply(text, &[range], |source, selections| toggle_style(source, selections, style));
    (text, ranges[0])
}

#[test]
fn a_style_wraps_the_selection_and_keeps_it_on_the_text() {
    assert_eq!(styled("a word here", (2, 6), Style::Bold), ("a **word** here".to_owned(), (4, 8)));
    assert_eq!(styled("a word here", (2, 6), Style::Italic), ("a *word* here".to_owned(), (3, 7)));
    assert_eq!(styled("a word here", (2, 6), Style::Strikethrough), ("a ~~word~~ here".to_owned(), (4, 8)));
    assert_eq!(styled("a word here", (2, 6), Style::Code), ("a `word` here".to_owned(), (3, 7)));
}

#[test]
fn a_style_comes_off_again_with_or_without_its_markers_selected() {
    assert_eq!(styled("a **word** here", (4, 8), Style::Bold), ("a word here".to_owned(), (2, 6)));
    assert_eq!(styled("a **word** here", (2, 10), Style::Bold), ("a word here".to_owned(), (2, 6)));
    assert_eq!(styled("a `word` here", (3, 7), Style::Code), ("a word here".to_owned(), (2, 6)));
}

#[test]
fn italic_and_bold_are_told_apart_by_their_stars() {
    assert_eq!(styled("**word**", (2, 6), Style::Italic), ("***word***".to_owned(), (3, 7)));
    assert_eq!(styled("***word***", (3, 7), Style::Italic), ("**word**".to_owned(), (2, 6)));
    assert_eq!(styled("***word***", (3, 7), Style::Bold), ("*word*".to_owned(), (1, 5)));
    assert_eq!(styled("*word*", (1, 5), Style::Bold), ("***word***".to_owned(), (3, 7)));
}

#[test]
fn a_caret_takes_its_word_and_on_nothing_leaves_the_markers_around_it() {
    assert_eq!(styled("a word here", (4, 4), Style::Bold), ("a **word** here".to_owned(), (4, 8)));
    assert_eq!(styled("a  b", (2, 2), Style::Bold), ("a **** b".to_owned(), (4, 4)));
}

#[test]
fn every_selection_is_styled_and_placed() {
    let (text, ranges) =
        apply("one two three", &[(0, 3), (8, 13)], |source, selections| toggle_style(source, selections, Style::Bold));
    assert_eq!(text, "**one** two **three**");
    assert_eq!(ranges, [(2, 5), (14, 19)]);
}

#[test]
fn a_link_leaves_the_caret_where_the_address_goes() {
    let (text, ranges) = apply("see docs now", &[(4, 8)], |source, selections| link(source, selections, None));
    assert_eq!(text, "see [docs]() now");
    assert_eq!(ranges, [(11, 11)]);
    let (text, _) = apply("see docs now", &[(4, 8)], |source, selections| link(source, selections, Some("a.md")));
    assert_eq!(text, "see [docs](a.md) now");
}

#[test]
fn headings_set_replace_and_clear_the_level_of_every_selected_line() {
    let lines = |level| move |source: &Text, selections: &Selections| heading(source, selections, level);
    assert_eq!(apply("one\ntwo", &[(0, 5)], lines(2)).0, "## one\n## two");
    assert_eq!(apply("### one", &[(4, 7)], lines(1)), ("# one".to_owned(), vec![(2, 5)]));
    assert_eq!(apply("## one", &[(3, 6)], lines(0)), ("one".to_owned(), vec![(0, 3)]));
    assert_eq!(apply("one\n\ntwo", &[(0, 8)], lines(1)).0, "# one\n\n# two", "blank lines stay blank");
    assert_eq!(apply("#tag", &[(0, 4)], lines(1)).0, "# #tag", "a hash without a space is text");
}

#[test]
fn the_toolbar_reads_the_level_of_the_selected_line() {
    let selections = Selections::single(Selection::new(8, 11));
    assert_eq!(heading_level(&Text::from("one\n### two"), &selections), 3);
    assert_eq!(heading_level(&Text::from("plain"), &Selections::single(Selection::new(0, 3))), 0);
}

#[test]
fn lists_toggle_and_change_kind_keeping_the_indent() {
    let kind = |kind| move |source: &Text, selections: &Selections| list(source, selections, kind);
    assert_eq!(apply("one\ntwo", &[(0, 7)], kind(ListKind::Bullet)).0, "- one\n- two");
    assert_eq!(apply("- one\n- two", &[(0, 11)], kind(ListKind::Bullet)).0, "one\ntwo");
    assert_eq!(apply("- one\n- two", &[(0, 11)], kind(ListKind::Numbered)).0, "1. one\n2. two");
    assert_eq!(apply("1. one", &[(0, 6)], kind(ListKind::Check)).0, "- [ ] one");
    assert_eq!(apply("- [x] one", &[(0, 9)], kind(ListKind::Check)).0, "one");
    assert_eq!(apply("- [x] one", &[(0, 9)], kind(ListKind::Bullet)).0, "- one", "a task is no plain bullet");
    assert_eq!(apply("  one", &[(2, 5)], kind(ListKind::Bullet)).0, "  - one");
    assert_eq!(apply("", &[(0, 0)], kind(ListKind::Bullet)).0, "- ");
}

struct Fixture {
    registry: Registry,
    session: Session,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new(name: &str, text: &str) -> Self {
        zen_i18n::register(crate::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(name);
        std::fs::write(&file, text).unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        Self { registry, session, _dir: dir }
    }

    fn run(&mut self, id: &str, params: Value) -> Result<Value, String> {
        self.registry.dispatch(&mut self.session, id, params).map_err(|error| error.to_string())
    }

    fn text(&self) -> String {
        self.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
    }

    fn select(&mut self, anchor: usize, head: usize) {
        self.run("editor.selection.set", json!({ "selections": [{ "anchor": anchor, "head": head }] })).unwrap();
    }
}

#[test]
fn the_commands_edit_the_active_markdown_file_in_one_undo_step() {
    let mut fixture = Fixture::new("a.md", "a word here\n");
    fixture.select(2, 6);
    let answer = fixture.run("markdown.format.bold", json!({})).unwrap();
    assert_eq!(answer, json!({ "changed": true, "selection": [4, 8] }));
    assert_eq!(fixture.text(), "a **word** here\n");
    fixture.run("editor.edit.undo", json!({})).unwrap();
    assert_eq!(fixture.text(), "a word here\n");

    fixture.run("markdown.format.heading", json!({ "level": 2 })).unwrap();
    assert_eq!(fixture.text(), "## a word here\n");
    assert!(fixture.run("markdown.format.heading", json!({ "level": 7 })).is_err());
    fixture.run("markdown.format.list", json!({ "kind": "check" })).unwrap();
    assert!(fixture.text().starts_with("- [ ] ## a word"), "{}", fixture.text());
    fixture.select(6, 8);
    fixture.run("markdown.format.link", json!({ "url": "b.md" })).unwrap();
    assert!(fixture.text().contains("[##](b.md)"), "{}", fixture.text());
}

#[test]
fn outside_markdown_the_commands_refuse_and_are_not_available() {
    let mut fixture = Fixture::new("a.txt", "a word here\n");
    assert!(!fixture.registry.is_available(&fixture.session, "markdown.format.bold"));
    assert!(fixture.run("markdown.format.bold", json!({})).is_err());
    assert_eq!(fixture.text(), "a word here\n");
}
