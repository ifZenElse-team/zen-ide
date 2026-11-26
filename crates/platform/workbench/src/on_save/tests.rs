use std::sync::Arc;

use serde_json::{Value, json};
use zen_syntax::{Highlighter, Span};

use super::*;
use crate::brackets::Highlighters;

fn all(keep_caret_lines: bool) -> Options {
    Options { remove_trailing_spaces: Removal::All, keep_caret_lines, ..Options::default() }
}

fn cleaned(text: &str, saved: &str, options: &Options, carets: &[usize]) -> String {
    let significant = |_: usize| false;
    let edits = replacements(text, options, &Cleanup { saved, carets, significant: &significant });
    assert!(edits.windows(2).all(|pair| pair[0].0.end <= pair[1].0.start), "sorted, not overlapping: {edits:?}");
    let mut result = text.to_owned();
    for (range, inserted) in edits.into_iter().rev() {
        result.replace_range(range, &inserted);
    }
    result
}

#[test]
fn modified_lines_are_the_lines_changed_since_the_save() {
    let saved = "a  \nb\nc  \n";
    let text = "a  \nb \t \nc  \nd  \n";
    let options = Options { keep_caret_lines: false, ..Options::default() };
    assert_eq!(options.remove_trailing_spaces, Removal::Modified, "the default");
    assert_eq!(cleaned(text, saved, &options, &[0]), "a  \nb\nc  \nd\n");
    assert_eq!(cleaned(saved, saved, &options, &[0]), saved, "nothing changed, nothing goes");
    assert_eq!(cleaned("a  \nc  \n", saved, &options, &[0]), "a  \nc  \n", "a deleted line changes no other line");
}

#[test]
fn every_caret_line_keeps_its_spaces() {
    let text = "a  \nb  \nc  \n";
    let carets = [text.find('b').unwrap() + 1, text.len() - 1];
    assert_eq!(cleaned(text, "", &all(true), &carets), "a\nb  \nc  \n");
    assert_eq!(cleaned(text, "", &all(false), &carets), "a\nb\nc\n");
    let modified = Options { keep_caret_lines: true, ..Options::default() };
    assert_eq!(cleaned(text, "", &modified, &carets[..1]), "a\nb  \nc\n");
}

#[test]
fn line_separators_stay_as_they_are() {
    assert_eq!(cleaned("a  \r\nb\t\r\nc ", "", &all(false), &[]), "a\r\nb\r\nc");
    assert_eq!(cleaned("a  \rb \r", "", &all(false), &[]), "a\rb\r");
    let ensure = Options { ensure_line_break: true, ..all(false) };
    assert_eq!(cleaned("a\r\nb  ", "", &ensure, &[]), "a\r\nb\r\n");
    assert_eq!(cleaned("a\rb", "", &ensure, &[]), "a\rb\r");
    assert_eq!(cleaned("a  ", "", &ensure, &[]), "a\n");
    assert_eq!(cleaned("a\n", "", &ensure, &[]), "a\n");
    assert_eq!(cleaned("", "", &ensure, &[]), "", "an empty file stays empty");
    assert_eq!(cleaned("   ", "", &ensure, &[]), "", "and so does one that becomes empty");
}

#[test]
fn a_blank_line_between_cr_and_lf_keeps_one_space_so_the_separators_never_merge() {
    assert_eq!(cleaned("a\r  \nb", "", &all(false), &[]), "a\r \nb");
    assert_eq!(cleaned("a\r\t \nb", "", &all(false), &[]), "a\r\t\nb", "the first of its spaces");
    assert_eq!(cleaned("a\r \nb", "", &all(false), &[]), "a\r \nb", "one space is already the least");
    assert_eq!(cleaned("a\r  \r\nb\n  \nc\r  \rd\r  ", "", &all(false), &[]), "a\r\r\nb\n\nc\r\rd\r");
    let modified = Options { keep_caret_lines: false, ..Options::default() };
    assert_eq!(cleaned("a\r  \nb", "a\rb", &modified, &[]), "a\r \nb");
    let blank = Options { remove_trailing_blank_lines: true, ensure_line_break: true, ..all(false) };
    assert_eq!(cleaned("a\r  \n\r\n", "", &blank, &[]), "a\r", "trailing blank lines go whole");
    assert_eq!(cleaned("a\r  \nb\r  \n", "", &blank, &[]), "a\r \nb\r");
    let caret = Options { keep_caret_lines: true, ..blank };
    assert_eq!(cleaned("a\r  \n \n", "", &caret, &[7]), "a\r \n\n", "kept down to the caret line");
}

#[test]
fn trailing_blank_lines_go_down_to_the_last_line_with_text_or_a_caret() {
    let blank = Options {
        remove_trailing_blank_lines: true,
        remove_trailing_spaces: Removal::Off,
        keep_caret_lines: true,
        ..Options::default()
    };
    assert_eq!(cleaned("a\n\n  \n\n", "", &blank, &[0]), "a\n");
    assert_eq!(cleaned("a\r\n\r\n \r\n", "", &blank, &[0]), "a\r\n");
    assert_eq!(cleaned("a\n\n", "", &blank, &[0]), "a\n");
    assert_eq!(cleaned("a", "", &blank, &[0]), "a");
    assert_eq!(cleaned("\n \n", "", &blank, &[]), "");
    let text = "a\n\n  \n\n";
    let caret = [text.find("  ").unwrap()];
    assert_eq!(cleaned(text, "", &blank, &caret), "a\n\n  \n", "the caret line stays");
    let loose = Options { keep_caret_lines: false, ..blank };
    assert_eq!(cleaned(text, "", &loose, &caret), "a\n");
    let everything = Options { remove_trailing_blank_lines: true, ensure_line_break: true, ..all(false) };
    assert_eq!(cleaned("a  \n\n\t\n", "", &everything, &[]), "a\n");
    assert_eq!(cleaned("a  \nb  ", "", &everything, &[]), "a\nb\n");
}

#[test]
fn significant_spaces_stay() {
    let text = "keep  \ndrop  \n";
    let significant = |from: usize| from == 4;
    let edits = replacements(text, &all(false), &Cleanup { saved: "", carets: &[], significant: &significant });
    assert_eq!(edits, [(11..13, String::new())]);
    let markdown = "one  \ntwo \n  \nthree\t\n";
    assert!(markdown_break(markdown, 3), "two spaces after text are a hard line break");
    assert!(!markdown_break(markdown, 9), "one space is not");
    assert!(!markdown_break(markdown, 11), "nor are spaces on a blank line");
    assert!(is_markdown(Path::new("README.md")) && is_markdown(Path::new("a.MARKDOWN")));
    assert!(!is_markdown(Path::new("main.go")));
}

#[test]
fn offsets_convert_between_characters_and_bytes() {
    let text = "ä b\nц  ";
    assert_eq!(byte_offsets(text, &[6, 0, 3, 99]), [8, 0, 4, 9]);
    assert_eq!(char_offsets(text, &[8, 0, 4]), [6, 0, 3]);
}

struct RawStrings;

impl Highlighter for RawStrings {
    fn handles(&self, file_name: &str) -> bool {
        file_name.ends_with(".go")
    }

    fn highlight(&self, text: &str) -> Vec<Span> {
        let quotes: Vec<usize> = text.match_indices('`').map(|(at, _)| at).collect();
        quotes
            .chunks(2)
            .filter(|pair| pair.len() == 2)
            .map(|pair| Span { range: pair[0]..pair[1] + 1, kind: HighlightKind::String })
            .collect()
    }
}

struct Fixture {
    registry: Registry,
    session: Session,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        for (name, text) in files {
            let path = dir.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(Workbench::default());
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let strings: Arc<dyn Highlighter> = Arc::new(RawStrings);
        session
            .state_mut::<Highlighters>()
            .set_source(Arc::new(move |name| RawStrings.handles(name).then(|| Arc::clone(&strings))));
        Self { dir, registry, session }
    }

    fn run(&mut self, id: &str, params: Value) -> Result<Value, zen_core::DispatchError> {
        self.registry.dispatch(&mut self.session, id, params)
    }

    fn open(&mut self, name: &str) {
        let path = self.dir.path().join(name);
        self.run("file.document.open", json!({ "path": path })).unwrap();
    }

    fn caret(&mut self, line: usize, column: usize) {
        self.run("editor.selection.set", json!({ "selections": [{ "head": { "line": line, "column": column } }] }))
            .unwrap();
    }

    fn type_at(&mut self, line: usize, column: usize, text: &str) {
        self.caret(line, column);
        self.run("editor.edit.insert", json!({ "text": text })).unwrap();
    }

    fn text(&self) -> String {
        self.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
    }

    fn on_disk(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.path().join(name)).unwrap()
    }

    fn set(&mut self, key: &str, value: impl Serialize) {
        self.session.settings_mut().set(key, value).unwrap();
    }
}

#[test]
fn saving_strips_the_modified_lines_but_the_caret_line_as_one_undo_step() {
    let original = "package main  \n\nfunc a() {}\nfunc b() {}\n";
    let mut fixture = Fixture::new(&[("main.go", original)]);
    fixture.open("main.go");
    fixture.type_at(2, 11, " // a  ");
    fixture.type_at(3, 11, " // b  ");
    fixture.run("file.document.save", json!({})).unwrap();
    let saved = "package main  \n\nfunc a() {} // a\nfunc b() {} // b  \n";
    assert_eq!(fixture.on_disk("main.go"), saved, "untouched lines and the caret line keep their spaces");
    assert_eq!(fixture.text(), saved);
    let caret = fixture.session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary().head;
    assert_eq!(caret, saved.rfind("  \n").unwrap() + 2, "the caret stays at the end of its line");
    fixture.run("editor.edit.undo", json!({})).unwrap();
    assert_eq!(fixture.text(), "package main  \n\nfunc a() {} // a  \nfunc b() {} // b  \n", "one undo step");
    fixture.run("editor.edit.redo", json!({})).unwrap();
    assert_eq!(fixture.text(), saved);
}

#[test]
fn saving_with_every_option_on_cleans_the_whole_file_keeping_crlf() {
    let mut fixture = Fixture::new(&[("notes.txt", "one  \r\ntwo\t\r\n\r\n  \r\n")]);
    fixture.set(REMOVE_TRAILING_SPACES_ON, TrailingSpaces::All);
    fixture.set(KEEP_CARET_LINES, false);
    fixture.set(REMOVE_TRAILING_BLANK_LINES, true);
    fixture.set(ENSURE_LINE_BREAK, true);
    fixture.open("notes.txt");
    fixture.type_at(0, 0, "#");
    fixture.run("file.document.saveAll", json!({})).unwrap();
    assert_eq!(fixture.on_disk("notes.txt"), "#one\r\ntwo\r\n");

    let mut fixture = Fixture::new(&[("last.txt", "a\r\nb")]);
    fixture.set(ENSURE_LINE_BREAK, true);
    fixture.open("last.txt");
    fixture.type_at(1, 1, "c");
    fixture.run("file.document.save", json!({})).unwrap();
    assert_eq!(fixture.on_disk("last.txt"), "a\r\nbc\r\n");
    let document = fixture.session.state::<Workbench>().unwrap().active_document().unwrap();
    assert_eq!(document.selections().primary().head, 5, "the caret stays before the line break added");
}

#[test]
fn a_file_with_mixed_line_separators_keeps_each_of_them() {
    let mut fixture = Fixture::new(&[("mixed.txt", "one  \r\ntwo\t\rthree \nfour")]);
    fixture.set(REMOVE_TRAILING_SPACES_ON, TrailingSpaces::All);
    fixture.set(KEEP_CARET_LINES, false);
    fixture.set(ENSURE_LINE_BREAK, true);
    fixture.open("mixed.txt");
    fixture.type_at(0, 0, "#");
    fixture.run("file.document.save", json!({})).unwrap();
    assert_eq!(fixture.on_disk("mixed.txt"), "#one\r\ntwo\rthree\nfour\r\n");
    fixture.run("editor.edit.undo", json!({})).unwrap();
    assert_eq!(fixture.text(), "#one  \r\ntwo\t\rthree \nfour");
}

#[test]
fn switched_off_nothing_changes() {
    let mut fixture = Fixture::new(&[("a.txt", "a  \nb  ")]);
    fixture.set(REMOVE_TRAILING_SPACES, false);
    fixture.open("a.txt");
    fixture.type_at(0, 0, "x");
    fixture.caret(1, 0);
    fixture.run("file.document.save", json!({})).unwrap();
    assert_eq!(fixture.on_disk("a.txt"), "xa  \nb  ");
}

#[test]
fn spaces_inside_multi_line_strings_and_markdown_hard_breaks_stay() {
    let code = "package main\n\nvar s = `first  \n  \nsecond  `\nvar t = 1  \n";
    let markdown = "Line one  \nLine two \n";
    let mut fixture = Fixture::new(&[("main.go", code), ("README.md", markdown)]);
    fixture.set(REMOVE_TRAILING_SPACES_ON, TrailingSpaces::All);
    fixture.open("main.go");
    fixture.type_at(0, 0, "\n");
    fixture.caret(0, 0);
    fixture.run("file.document.save", json!({})).unwrap();
    assert_eq!(fixture.on_disk("main.go"), "\npackage main\n\nvar s = `first  \n  \nsecond  `\nvar t = 1\n");
    fixture.open("README.md");
    fixture.type_at(1, 0, "> ");
    fixture.caret(0, 0);
    fixture.run("file.document.save", json!({})).unwrap();
    assert_eq!(fixture.on_disk("README.md"), "Line one  \n> Line two\n");
}

#[test]
fn read_only_files_are_left_alone() {
    let mut fixture = Fixture::new(&[("locked.txt", "a  \n")]);
    fixture.set(KEEP_CARET_LINES, false);
    fixture.open("locked.txt");
    fixture.type_at(0, 1, "b");
    let path = fixture.dir.path().join("locked.txt");
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).unwrap();
    assert!(!before_save(&mut fixture.session, &path), "a read-only file keeps its spaces");
    assert_eq!(fixture.text(), "ab  \n");
    let saved = fixture.session.state::<Workbench>().unwrap().active_document().unwrap().saved.to_string();
    assert_eq!(saved, "a  \n", "and nothing was written");
}

#[test]
fn editorconfig_overrides_the_options_for_matching_files() {
    let editorconfig = "root = true\n[*.txt]\ntrim_trailing_whitespace = false\ninsert_final_newline = true\n\
                        [*.go]\ntrim_trailing_whitespace = true\ninsert_final_newline = false\n";
    let mut fixture =
        Fixture::new(&[(".editorconfig", editorconfig), ("keep.txt", "a  \nb  "), ("strip.go", "a  \nb  ")]);
    fixture.set(REMOVE_TRAILING_SPACES, false);
    fixture.set(ENSURE_LINE_BREAK, true);
    fixture.set(KEEP_CARET_LINES, false);
    fixture.open("keep.txt");
    fixture.type_at(0, 0, "x");
    fixture.run("file.document.save", json!({})).unwrap();
    assert_eq!(
        fixture.on_disk("keep.txt"),
        "xa  \nb  \n",
        "trim_trailing_whitespace = false, insert_final_newline = true"
    );
    fixture.open("strip.go");
    fixture.type_at(0, 0, "x");
    fixture.run("file.document.save", json!({})).unwrap();
    assert_eq!(fixture.on_disk("strip.go"), "xa\nb", "trim_trailing_whitespace = true strips all lines");

    let options = fixture.run("editor.onSave.options", json!({ "path": "strip.go" })).unwrap();
    assert_eq!(options["removeTrailingSpaces"], "all", "{options}");
    assert_eq!(
        (options["trimTrailingWhitespace"].clone(), options["insertFinalNewline"].clone()),
        (json!(true), json!(false))
    );
    assert_eq!(options["ensureLineBreak"], false);
    let options = fixture.run("editor.onSave.options", json!({ "path": "other.rs" })).unwrap();
    assert_eq!(options["removeTrailingSpaces"], "off", "no section for it: the settings");
    assert_eq!(options["trimTrailingWhitespace"], Value::Null);
    assert_eq!(options["ensureLineBreak"], true);
    let active = fixture.run("editor.onSave.options", json!({})).unwrap();
    assert_eq!(active["path"], json!(fixture.dir.path().join("strip.go")));
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join(".editorconfig"), "root = true\n[*]\ntrim_trailing_whitespace = true\n")
        .unwrap();
    let options = fixture.run("editor.onSave.options", json!({ "path": outside.path().join("a.txt") })).unwrap();
    assert_eq!(options["trimTrailingWhitespace"], Value::Null, "outside the project: no EditorConfig");
}

struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

#[derive(Clone)]
struct GeneratedLine {
    content: String,
    separator: &'static str,
    new: bool,
}

fn unique_blank(id: usize) -> String {
    format!(
        "\t{}",
        (0..usize::BITS - id.leading_zeros())
            .map(|bit| if id >> bit & 1 == 1 { '\t' } else { ' ' })
            .collect::<String>()
    )
}

fn generated_content(random: &mut Random, id: usize) -> String {
    const WORDS: &[&str] =
        &["fn", "x := `raw", "\"str", "ä", "ц", "🙂", "a\tb", "日本", "\u{a0}", "\u{3000}", "#", "*"];
    const TRAILING: &[&str] = &["", "", " ", "  ", "\t", " \t ", "   ", "\u{a0} ", "\u{3000}\t"];
    if random.below(6) == 0 {
        return if random.below(2) == 0 { String::new() } else { unique_blank(id) };
    }
    let mut content = format!("{}id{id}", "\t".repeat(random.below(3)));
    for _ in 0..random.below(4) {
        content.push(' ');
        content.push_str(random.pick(WORDS));
    }
    content.push_str(random.pick(TRAILING));
    content
}

fn new_content(random: &mut Random, id: usize) -> String {
    Some(generated_content(random, id)).filter(|content| !content.is_empty()).unwrap_or_else(|| unique_blank(id))
}

fn separator(random: &mut Random, style: usize) -> &'static str {
    match style {
        0 => "\n",
        1 => "\r\n",
        2 => "\r",
        _ => ["\n", "\r\n", "\r"][random.below(3)],
    }
}

fn joined(lines: &mut [GeneratedLine]) -> String {
    for index in 1..lines.len() {
        if lines[index - 1].separator == "\r" && lines[index].content.is_empty() && lines[index].separator == "\n" {
            lines[index].separator = "\r\n";
        }
    }
    lines.iter().map(|line| format!("{}{}", line.content, line.separator)).collect()
}

fn bounds(text: &str) -> Vec<(usize, usize, usize)> {
    lines(text).into_iter().map(|line| (line.start, line.content_end, line.end)).collect()
}

fn expected(
    text: &str,
    generated: &[GeneratedLine],
    options: &Options,
    caret_lines: &[usize],
    significant_lines: &[usize],
) -> String {
    let kept = |index: usize| options.keep_caret_lines && caret_lines.contains(&index);
    let blank = |line: &GeneratedLine| line.content.trim_end_matches([' ', '\t']).is_empty();
    let mut last = generated.len();
    if options.remove_trailing_blank_lines {
        let filled = generated.iter().rposition(|line| !blank(line));
        let caret = (0..generated.len()).rev().find(|&index| kept(index));
        last = filled.into_iter().chain(caret).max().map_or(0, |index| index + 1);
    }
    let mut result = String::new();
    for (index, line) in generated[..last].iter().enumerate() {
        let trimmed = line.content.trim_end_matches([' ', '\t']);
        let strip = match options.remove_trailing_spaces {
            Removal::Off => false,
            Removal::All => true,
            Removal::Modified => line.new,
        } && !kept(index)
            && !significant_lines.contains(&index);
        let joins = index > 0 && generated[index - 1].separator == "\r" && line.separator == "\n";
        let trimmed = if trimmed.is_empty() && joins { &line.content[..1] } else { trimmed };
        result.push_str(if strip { trimmed } else { &line.content });
        result.push_str(line.separator);
    }
    if options.ensure_line_break && !result.is_empty() && !result.ends_with(['\n', '\r']) {
        let first = text
            .find(['\n', '\r'])
            .map_or("\n", |at| if text[at..].starts_with("\r\n") { "\r\n" } else { &text[at..=at] });
        result.push_str(first);
    }
    result
}

struct Case {
    saved: String,
    text: String,
    generated: Vec<GeneratedLine>,
    options: Options,
    carets: Vec<usize>,
    caret_lines: Vec<usize>,
    significant_lines: Vec<usize>,
}

fn random_case(random: &mut Random) -> Case {
    let style = random.below(4);
    let mut id = 1;
    let mut original: Vec<GeneratedLine> = (0..random.below(12))
        .map(|_| {
            id += 1;
            GeneratedLine { content: generated_content(random, id), separator: separator(random, style), new: false }
        })
        .collect();
    if let Some(last) = original.last_mut()
        && random.below(3) == 0
    {
        last.separator = "";
    }
    let saved = joined(&mut original);
    let mut generated = original.clone();
    for _ in 0..random.below(6) {
        id += 1;
        let at = random.below(generated.len() + 1);
        match random.below(4) {
            0 if at < generated.len() => {
                generated.remove(at);
            }
            1 if at < generated.len() => {
                let line = &mut generated[at];
                line.content = format!(
                    "{}{}",
                    line.content.trim_end_matches([' ', '\t']),
                    ["x ", " y\t", "z", "  "][random.below(4)]
                );
                line.content = format!("{} id{id}", line.content);
                line.content.push_str(random.pick(&["", " ", "\t\t"]));
                line.new = true;
            }
            2 if at < generated.len() => {
                generated[at].content = new_content(random, id);
                generated[at].new = true;
            }
            _ => {
                let separator =
                    if at == generated.len() && generated.last().is_some_and(|line| line.separator.is_empty()) {
                        ""
                    } else {
                        separator(random, style)
                    };
                if separator.is_empty() {
                    generated.last_mut().unwrap().separator = separator_of(style);
                }
                generated.insert(at, GeneratedLine { content: new_content(random, id), separator, new: true });
            }
        }
    }
    if generated.last().is_none_or(|line| !line.separator.is_empty()) {
        generated.push(GeneratedLine { content: String::new(), separator: "", new: false });
    }
    let text = joined(&mut generated);
    let lines = bounds(&text);
    assert_eq!(
        lines.len(),
        generated.len(),
        "{text:?} {:?}",
        generated.iter().map(|line| (line.content.clone(), line.separator)).collect::<Vec<_>>()
    );
    let mut carets = Vec::new();
    let mut caret_lines = Vec::new();
    for _ in 0..random.below(4) {
        let line = random.below(lines.len());
        let (start, content_end, _) = lines[line];
        let boundaries: Vec<usize> = (start..=content_end).filter(|at| text.is_char_boundary(*at)).collect();
        carets.push(boundaries[random.below(boundaries.len())]);
        caret_lines.push(line);
    }
    let significant_lines: Vec<usize> = (0..lines.len()).filter(|_| random.below(5) == 0).collect();
    let options = Options {
        path: PathBuf::new(),
        remove_trailing_spaces: [Removal::Off, Removal::Modified, Removal::Modified, Removal::All][random.below(4)],
        keep_caret_lines: random.below(3) != 0,
        remove_trailing_blank_lines: random.below(2) == 0,
        ensure_line_break: random.below(2) == 0,
        trim_trailing_whitespace: None,
        insert_final_newline: None,
    };
    Case { saved, text, generated, options, carets, caret_lines, significant_lines }
}

fn separator_of(style: usize) -> &'static str {
    match style {
        1 => "\r\n",
        2 => "\r",
        _ => "\n",
    }
}

#[test]
fn random_documents_and_edits_lose_only_the_trailing_spaces_of_the_right_lines() {
    let mut random = Random(0x9E37_79B9_7F4A_7C15);
    for round in 0..20_000 {
        let case = random_case(&mut random);
        let lines = bounds(&case.text);
        let line_of = |offset: usize| {
            lines.iter().position(|line| offset <= line.1 || offset < line.2).unwrap_or(lines.len() - 1)
        };
        let significant = |from: usize| case.significant_lines.contains(&line_of(from));
        let edits = replacements(
            &case.text,
            &case.options,
            &Cleanup { saved: &case.saved, carets: &case.carets, significant: &significant },
        );
        assert!(edits.windows(2).all(|pair| pair[0].0.end <= pair[1].0.start), "round {round}: {edits:?}");
        let mut result = case.text.clone();
        for (range, inserted) in edits.iter().rev() {
            result.replace_range(range.clone(), inserted);
        }
        let wanted = expected(&case.text, &case.generated, &case.options, &case.caret_lines, &case.significant_lines);
        assert_eq!(
            result, wanted,
            "round {round}: saved {:?}, text {:?}, options {:?}, carets {:?}, significant lines {:?}",
            case.saved, case.text, case.options, case.carets, case.significant_lines
        );
    }
}

#[test]
fn random_saves_through_the_editor_keep_every_caret_line_of_every_split_and_undo_in_one_step() {
    let mut random = Random(0xD1B5_4A32_D192_ED03);
    for round in 0..120 {
        let mut case = random_case(&mut random);
        case.options.remove_trailing_spaces = [Removal::Off, Removal::Modified, Removal::All][random.below(3)];
        let mut fixture = Fixture::new(&[("file.txt", case.text.as_str())]);
        fixture.set(REMOVE_TRAILING_SPACES, case.options.remove_trailing_spaces != Removal::Off);
        let lines = if case.options.remove_trailing_spaces == Removal::All {
            TrailingSpaces::All
        } else {
            TrailingSpaces::Modified
        };
        fixture.set(REMOVE_TRAILING_SPACES_ON, lines);
        fixture.set(KEEP_CARET_LINES, case.options.keep_caret_lines);
        fixture.set(REMOVE_TRAILING_BLANK_LINES, case.options.remove_trailing_blank_lines);
        fixture.set(ENSURE_LINE_BREAK, case.options.ensure_line_break);
        fixture.open("file.txt");
        fixture.run("editor.split.right", json!({})).unwrap();
        let document = fixture.session.state_mut::<Workbench>().active_document_mut().unwrap();
        document.saved = zen_text::Text::from(case.saved.as_str());
        document.dirty = case.text != case.saved;
        std::fs::write(document.path(), &case.saved).unwrap();
        assert_eq!(fixture.text(), case.text, "round {round}");
        let chars = char_offsets(&case.text, &case.carets);
        let (second, first) = chars.split_at(chars.len() / 2);
        let carets = |offsets: &[usize]| json!({ "selections": offsets.iter().map(|head| json!({ "head": head })).collect::<Vec<_>>() });
        let mut caret_lines = case.caret_lines.clone();
        for (index, group) in [second, first].into_iter().enumerate() {
            if index == 1 {
                fixture.run("editor.split.next", json!({})).unwrap();
            }
            if group.is_empty() {
                caret_lines.push(0);
            }
            fixture.run("editor.selection.set", carets(if group.is_empty() { &[0] } else { group })).unwrap();
        }
        fixture.run("file.document.save", json!({})).unwrap();
        let wanted = if case.text == case.saved {
            case.saved.clone()
        } else {
            expected(&case.text, &case.generated, &case.options, &caret_lines, &[])
        };
        let context = format!(
            "round {round}: saved {:?}, text {:?}, options {:?}, carets {chars:?}",
            case.saved, case.text, case.options
        );
        assert_eq!(fixture.on_disk("file.txt"), wanted, "{context}");
        assert_eq!(fixture.text(), wanted, "{context}");
        if wanted != case.text {
            fixture.run("editor.edit.undo", json!({})).unwrap();
            assert_eq!(fixture.text(), case.text, "one undo step: {context}");
            fixture.run("editor.edit.redo", json!({})).unwrap();
            assert_eq!(fixture.text(), wanted, "{context}");
        }
    }
}

#[test]
fn many_replacements_at_once_move_the_text_and_every_place_as_one_at_a_time_does() {
    let text = "αβ one  \n\ttwo\r\nthree 🙂\rfour\n".repeat(8);
    let length = text.chars().count();
    let mut random = Random(0x2545_F491_4F6C_DD1D);
    for round in 0..500 {
        let mut replacements = Vec::new();
        let mut at = 0;
        while replacements.len() < 12 {
            let start = at + random.below(12);
            if start > length {
                break;
            }
            let end = (start + random.below(4)).min(length);
            replacements.push((start..end, random.pick(&["", "x", "  ", "ü\n", "🙂🙂", "\r\n"]).to_owned()));
            at = end;
        }
        let placed = || {
            let mut document = Document::with_text(Path::new("a.txt"), text.clone());
            document.marks = (0..=length).step_by(3).collect();
            document.edit_places = (0..=length).step_by(5).map(|offset| (offset, 0, 0)).collect();
            document.folds = (0..length).step_by(7).map(|start| start..(start + 1 + start % 5).min(length)).collect();
            document.find_excluded = document.folds.iter().rev().cloned().collect();
            document
        };
        let mut batch = placed();
        let mut single = placed();
        let edits: Vec<zen_text::Edit> = replacements
            .iter()
            .rev()
            .map(|(range, text)| zen_text::Edit::replace(range.clone(), text.clone()))
            .collect();
        batch.buffer.edit(edits.clone(), zen_text::EditKind::Other).unwrap();
        batch.applied_all(&edits);
        single.buffer.edit(edits.clone(), zen_text::EditKind::Other).unwrap();
        for edit in &edits {
            single.applied(edit);
        }
        let context = format!("round {round}: {replacements:?}");
        assert_eq!(batch.mirror, batch.buffer.text().to_string(), "{context}");
        assert_eq!(batch.mirror, single.mirror, "{context}");
        assert_eq!(batch.pending, single.pending, "{context}");
        assert_eq!(batch.pending_chars, single.pending_chars, "{context}");
        assert_eq!(batch.marks, single.marks, "{context}");
        assert_eq!(batch.edit_places, single.edit_places, "{context}");
        assert_eq!(batch.folds, single.folds, "{context}");
        assert_eq!(batch.find_excluded, single.find_excluded, "{context}");
    }
}

#[test]
fn saving_a_file_of_100_000_lines_takes_its_trailing_spaces_at_once() {
    let mut saved = String::new();
    let mut text = String::new();
    for line in 0..100_000 {
        saved.push_str(&format!("\tx := `line {line}`  \n"));
        text.push_str(&format!("\tx := `line {line}{}`  \n", if line % 2 == 0 { "x" } else { "" }));
    }
    let mut fixture = Fixture::new(&[("big.go", saved.as_str())]);
    fixture.set(KEEP_CARET_LINES, false);
    fixture.open("big.go");
    let document = fixture.session.state_mut::<Workbench>().active_document_mut().unwrap();
    let length = document.buffer.text().len_chars();
    document.replace_ranges(vec![(0..length, text.clone())]);
    let started = std::time::Instant::now();
    fixture.run("file.document.save", json!({})).unwrap();
    let elapsed = started.elapsed();
    let on_disk = fixture.on_disk("big.go");
    assert_eq!(on_disk.matches("`  \n").count(), 50_000, "the lines not changed keep their spaces");
    assert_eq!(on_disk.matches("x`\n").count(), 50_000, "the changed ones lose them");
    assert!(elapsed < std::time::Duration::from_secs(120), "{elapsed:?}: one pass, not a pass per line");
}
