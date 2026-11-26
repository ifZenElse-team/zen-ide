use std::path::PathBuf;

use serde_json::{Value, json};
use zen_syntax::Span;

use super::*;
use crate::brackets::Highlighters;

struct Code;

impl Highlighter for Code {
    fn handles(&self, name: &str) -> bool {
        name.ends_with(".go")
    }

    fn highlight(&self, text: &str) -> Vec<Span> {
        let mut spans = Vec::new();
        let bytes = text.as_bytes();
        let mut at = 0;
        while at < bytes.len() {
            if text[at..].starts_with("//") {
                let end = text[at..].find('\n').map_or(text.len(), |end| at + end);
                spans.push(Span { range: at..end, kind: HighlightKind::Comment });
                at = end;
            } else if bytes[at] == b'"' {
                let end = text[at + 1..].find('"').map_or(text.len(), |end| at + end + 2);
                spans.push(Span { range: at..end, kind: HighlightKind::String });
                at = end;
            } else if bytes[at].is_ascii_alphabetic() && (at == 0 || !bytes[at - 1].is_ascii_alphanumeric()) {
                let end = text[at..].find(|c: char| !c.is_ascii_alphanumeric()).map_or(text.len(), |end| at + end);
                if ["func", "package", "return", "var", "type"].contains(&&text[at..end]) {
                    spans.push(Span { range: at..end, kind: HighlightKind::Keyword });
                }
                at = end;
            } else {
                at += 1;
            }
        }
        spans
    }
}

const MAIN: &str = "package main\n\n// Serve answers the reqeusts.\nfunc parseUrlHadnler() string {\n\tmesage := \"wrold\"\n\treturn fmt.Sprnitf(mesage)\n}\n";

struct Editor {
    registry: Registry,
    session: Session,
    file: PathBuf,
    _dir: tempfile::TempDir,
}

impl Editor {
    fn new(name: &str, text: &str) -> Editor {
        zen_i18n::register(crate::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(name);
        std::fs::write(&file, text).unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        let code: Arc<dyn Highlighter> = Arc::new(Code);
        session
            .state_mut::<Highlighters>()
            .set_source(Arc::new(move |name| code.handles(name).then(|| Arc::clone(&code))));
        session.state_mut::<Spelling>().activate();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
        Editor { registry, session, file, _dir: dir }
    }

    fn dispatch(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn typos(&mut self) -> Value {
        let answer = self.dispatch("spelling.typos.list", json!({ "path": self.file }));
        assert_eq!(answer["checked"], true, "{answer}");
        answer["typos"].clone()
    }

    fn words(&mut self) -> Vec<(String, String)> {
        self.typos()
            .as_array()
            .unwrap()
            .iter()
            .map(|typo| (typo["word"].as_str().unwrap().to_owned(), typo["context"].as_str().unwrap().to_owned()))
            .collect()
    }

    fn set(&mut self, key: &str, on: bool) {
        self.session.settings_mut().set(key, on).unwrap();
    }
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(word, context)| ((*word).to_owned(), (*context).to_owned())).collect()
}

#[test]
fn comments_literals_and_declared_names_are_checked() {
    let mut editor = Editor::new("main.go", MAIN);
    assert_eq!(
        editor.words(),
        pairs(&[("reqeusts", "comment"), ("Hadnler", "code"), ("mesage", "code"), ("wrold", "literal")]),
        "Sprnitf is used, not declared here, so it is not checked"
    );
    let typos = editor.typos();
    let hadnler = &typos[1];
    let start = MAIN.find("Hadnler").unwrap();
    assert_eq!((hadnler["start"].as_u64(), hadnler["end"].as_u64()), (Some(start as u64), Some(start as u64 + 7)));
    assert_eq!((hadnler["line"].as_u64(), hadnler["column"].as_u64()), (Some(4), Some(14)));
}

#[test]
fn typos_are_problems_of_their_own_severity_and_not_project_errors() {
    zen_i18n::set_language(Some("en"));
    let mut editor = Editor::new("main.go", MAIN);
    editor.typos();
    let listed = editor.dispatch("problems.file.list", json!({ "path": editor.file }));
    let problems = crate::language::listed_problems(&listed);
    assert_eq!(problems.len(), 4, "{listed}");
    for problem in &problems {
        assert_eq!(problem["severity"], "typo");
        assert_eq!(problem["source"], "spelling");
        assert_eq!(problem["code"], "typo");
    }
    assert_eq!(problems[1]["message"], "Possible misspelling: 'Hadnler'");
    let project = editor.dispatch("problems.project.list", json!({}));
    assert!(
        crate::language::listed_problems(&project).iter().all(|file| file["problems"]
            .as_array()
            .is_none_or(|list| list.iter().all(|problem| problem["severity"] != "typo"))),
        "{project}"
    );
    let ru = zen_i18n::with_language("ru", || editor.dispatch("problems.file.list", json!({ "path": editor.file })));
    assert_eq!(crate::language::listed_problems(&ru)[1]["message"], "Возможная опечатка: «Hadnler»");
}

#[test]
fn f2_and_shift_f2_step_through_the_typos() {
    let mut editor = Editor::new("main.go", MAIN);
    editor.typos();
    let starts: Vec<u64> =
        editor.typos().as_array().unwrap().iter().map(|typo| typo["start"].as_u64().unwrap()).collect();
    let mut visited = Vec::new();
    for _ in 0..5 {
        let problem = editor.dispatch("editor.problem.next", json!({}));
        assert_eq!(problem["severity"], "typo");
        visited.push(problem["start"].as_u64().unwrap());
    }
    assert_eq!(visited, [starts[0], starts[1], starts[2], starts[3], starts[0]], "round to the first");
    let back = editor.dispatch("editor.problem.previous", json!({}));
    assert_eq!(back["start"].as_u64(), Some(starts[3]), "back round to the last");
    let carets = editor.dispatch("editor.selection.get", json!({}));
    assert_eq!(carets[0]["head"].as_u64(), Some(starts[3]), "the caret is on the typo");
}

#[test]
fn the_settings_choose_what_is_checked() {
    let mut editor = Editor::new("main.go", MAIN);
    editor.set(PROCESS_CODE, false);
    assert_eq!(editor.words(), pairs(&[("reqeusts", "comment"), ("wrold", "literal")]));
    editor.set(PROCESS_CODE, true);
    editor.set(PROCESS_LITERALS, false);
    assert_eq!(editor.words(), pairs(&[("reqeusts", "comment"), ("Hadnler", "code"), ("mesage", "code")]));
    editor.set(PROCESS_LITERALS, true);
    editor.set(PROCESS_COMMENTS, false);
    assert_eq!(editor.words(), pairs(&[("Hadnler", "code"), ("mesage", "code"), ("wrold", "literal")]));
    editor.set(ENABLED, false);
    let answer = editor.dispatch("spelling.typos.list", json!({ "path": editor.file }));
    assert_eq!(answer, json!({ "checked": false, "typos": [] }));
    let listed = editor.dispatch("problems.file.list", json!({ "path": editor.file }));
    assert!(crate::language::listed_problems(&listed).is_empty(), "{listed}");
}

#[test]
fn shown_typos_move_with_the_edits_until_the_next_check() {
    let mut editor = Editor::new("main.go", MAIN);
    editor.typos();
    editor.dispatch("editor.selection.set", json!({ "selections": [{ "head": 0 }] }));
    editor.dispatch("editor.edit.insert", json!({ "text": "// Hello.\n" }));
    let listed = editor.dispatch("problems.file.list", json!({ "path": editor.file }));
    let text = editor.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
    let chars: Vec<char> = text.chars().collect();
    for problem in crate::language::listed_problems(&listed) {
        let (start, end) = (problem["start"].as_u64().unwrap() as usize, problem["end"].as_u64().unwrap() as usize);
        let word: String = chars[start..end].iter().collect();
        assert!(problem["message"].as_str().unwrap().contains(&word), "{word} under {problem}");
    }
    assert_eq!(editor.words()[1].0, "Hadnler");
}

#[test]
fn prose_files_are_checked_word_by_word() {
    let mut editor = Editor::new("notes.md", "# Teh title\n\nSee https://exampel.com and the parseUrlHadnler.\n");
    assert_eq!(editor.words(), pairs(&[("Teh", "text"), ("Hadnler", "text")]));
}

#[test]
fn a_window_that_has_not_turned_it_on_checks_nothing() {
    let mut editor = Editor::new("main.go", MAIN);
    editor.session.state_mut::<Spelling>().active = false;
    let answer = editor.dispatch("spelling.typos.list", json!({ "path": editor.file, "waitMs": 10 }));
    assert_eq!(answer["checked"], false);
}

#[test]
fn words_are_judged_one_by_one() {
    let mut editor = Editor::new("main.go", MAIN);
    let answer =
        editor.dispatch("spelling.words.check", json!({ "words": ["handler", "Hadnler", "files", "colour", "teh"] }));
    assert_eq!(answer, json!(["Hadnler", "teh"]));
}

#[test]
fn without_declarations_the_keyword_before_a_name_declares_it() {
    let text = "func open() {}\nlet mut count = 1\nvalue := 2\ncall(arg)\ntype Name struct\nconst Limit = 3\n\
                fn draw(frame: &mut eframe::Frame, raw: *const u8, mut width: u32)";
    let declared: Vec<&str> =
        ["open", "count", "value", "call", "arg", "Name", "struct", "Limit", "eframe", "u8", "width"]
            .into_iter()
            .filter(|name| {
                let start = text.find(name).unwrap();
                declared_by_keyword(text, start..start + name.len())
            })
            .collect();
    assert_eq!(declared, ["open", "count", "value", "Name", "Limit", "width"], "&mut and *const name no new name");
}

#[test]
fn declarations_from_the_language_server_decide_which_names_are_checked() {
    let text = "func parseUrlHadnler() { fmt.Sprnitf(mesage) }";
    let spans = Code.highlight(text);
    let options = Options { code: true, literals: true, comments: true };
    let used = text.find("mesage").unwrap();
    let declared = vec![Range { start: used, end: used + 6 }];
    let regions = regions(text, &spans, Some(&declared), false, options);
    let names: Vec<&str> = regions.iter().map(|region| &text[region.range.clone()]).collect();
    assert_eq!(names, ["mesage"], "only what the server says is declared");
    let regions = regions_without(text, &spans, options);
    let names: Vec<&str> = regions.iter().map(|region| &text[region.range.clone()]).collect();
    assert_eq!(names, ["parseUrlHadnler"]);
}

fn regions_without(text: &str, spans: &[Span], options: Options) -> Vec<Region> {
    regions(text, spans, None, false, options)
}

#[test]
fn character_ranges_become_byte_ranges() {
    let text = "é = naïve";
    assert_eq!(byte_ranges(text, &[(4, 9), (0, 1)]), [0..2, 5..11]);
}

#[test]
fn the_link_targets_of_prose_are_not_checked() {
    let text = "See [the guide](docs/guide-exampel.md) of teh IDE.";
    let target = text.find("docs").unwrap()..text.find(')').unwrap();
    let spans = [Span { range: target, kind: HighlightKind::String }];
    let options = Options { code: true, literals: true, comments: true };
    let checked: Vec<&str> =
        regions(text, &spans, None, true, options).iter().map(|region| &text[region.range.clone()]).collect();
    assert_eq!(checked, ["See [the guide](", ") of teh IDE."]);
}

struct Counted(std::sync::Mutex<Vec<std::thread::ThreadId>>);

impl Highlighter for Counted {
    fn handles(&self, name: &str) -> bool {
        name.ends_with(".go")
    }

    fn highlight(&self, text: &str) -> Vec<Span> {
        self.0.lock().unwrap().push(std::thread::current().id());
        Code.highlight(text)
    }
}

#[test]
fn a_highlighter_that_is_not_incremental_is_never_called_off_the_session_thread() {
    let mut editor = Editor::new("main.go", MAIN);
    let counted = Arc::new(Counted(std::sync::Mutex::new(Vec::new())));
    let source: Arc<dyn Highlighter> = counted.clone();
    editor.session.state_mut::<Highlighters>().set_source(Arc::new(move |_| Some(Arc::clone(&source))));
    assert_eq!(editor.words()[0], ("reqeusts".to_owned(), "comment".to_owned()));
    editor.dispatch("editor.selection.set", json!({ "selections": [{ "head": MAIN.len() }] }));
    editor.dispatch("editor.edit.insert", json!({ "text": "// teh end\n" }));
    assert_eq!(editor.words().last().unwrap().0, "teh", "the edited text is checked again");
    let calls = counted.0.lock().unwrap();
    assert!(!calls.is_empty());
    assert!(
        calls.iter().all(|thread| *thread == std::thread::current().id()),
        "the check reuses the window's highlighting, so a plug-in's lock is never held by the spelling thread"
    );
}

struct Dictionaries {
    editor: Editor,
    data: tempfile::TempDir,
}

impl Dictionaries {
    fn new() -> Dictionaries {
        let mut editor = Editor::new("main.go", MAIN);
        let data = tempfile::tempdir().unwrap();
        editor
            .session
            .state_mut::<Spelling>()
            .set_application_dictionary(data.path().join("dictionaries/application.dic"));
        Dictionaries { editor, data }
    }

    fn application(&self) -> PathBuf {
        self.data.path().join("dictionaries/application.dic")
    }

    fn project(&self) -> PathBuf {
        self.editor.file.parent().unwrap().join(".idea/dictionaries/project.xml")
    }

    fn words(&mut self) -> Vec<String> {
        self.editor.words().into_iter().map(|(word, _)| word).collect()
    }
}

fn fixes_of(problem: &Value) -> Vec<(String, String, Value)> {
    problem["fixes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|fix| {
            if fix["kind"] == "heading" {
                return (fix["title"].as_str().unwrap().to_owned(), String::new(), Value::Null);
            }
            assert_eq!(fix["kind"], "action", "{fix}");
            (
                fix["title"].as_str().unwrap().to_owned(),
                fix["args"]["action"].as_str().unwrap().to_owned(),
                fix["args"]["params"].clone(),
            )
        })
        .collect()
}

#[test]
fn a_typo_offers_its_suggestions_or_a_rename_and_save_to_dictionary() {
    zen_i18n::set_language(Some("en"));
    let mut editor = Editor::new("main.go", MAIN);
    editor.typos();
    let comment = MAIN.find("reqeusts").unwrap();
    let listed = editor.dispatch("problems.fixes.list", json!({ "path": editor.file, "offset": comment + 2 }));
    assert_eq!(listed[0]["fixes"][0]["kind"], "heading");
    let fixes = fixes_of(&listed[0]);
    assert_eq!(fixes[0].0, "Fix typo");
    assert_eq!(fixes[1].0, "requests", "the closest word first: {fixes:?}");
    assert_eq!(fixes[1].1, "spelling.typo.change");
    assert_eq!(fixes[1].2, json!({ "path": editor.file, "offset": comment, "to": "requests" }));
    let save = fixes.last().unwrap();
    assert_eq!(save.0, "Save 'reqeusts' to dictionary");
    assert_eq!(save.1, SAVE_TO, "Use single dictionary is off by default, so it asks which");
    assert_eq!(save.2, json!({ "word": "reqeusts", "path": editor.file }));
    let file = editor.dispatch("problems.file.list", json!({ "path": editor.file }));
    let typo = file.as_array().unwrap().iter().find(|problem| problem["start"] == comment).unwrap();
    assert_eq!(fixes_of(typo).len(), 1, "the file's list leaves the suggestions to ⌥⏎: {typo}");
    let name = MAIN.find("Hadnler").unwrap();
    let listed = editor.dispatch("problems.fixes.list", json!({ "path": editor.file, "offset": name }));
    let fixes = fixes_of(&listed[0]);
    assert_eq!(fixes[0].0, "Typo: Rename to 'parseUrlHandler'", "a declared name is renamed to the best name");
    assert_eq!(fixes[0].1, "refactor.rename.apply");
    assert_eq!(fixes[0].2["newName"], "parseUrlHandler");
    assert_eq!(fixes.len(), 2);
    let ru = zen_i18n::with_language("ru", || {
        editor.dispatch("problems.fixes.list", json!({ "path": editor.file, "offset": name }))
    });
    let titles: Vec<String> = fixes_of(&ru[0]).into_iter().map(|(title, _, _)| title).collect();
    assert_eq!(titles, ["Опечатка: переименовать в «parseUrlHandler»", "Сохранить «Hadnler» в словарь"]);
    editor.set(SINGLE_DICTIONARY, true);
    let listed = editor.dispatch("problems.fixes.list", json!({ "path": editor.file, "offset": comment }));
    let save = fixes_of(&listed[0]).pop().unwrap();
    assert_eq!((save.0.as_str(), save.1.as_str()), ("Save 'reqeusts' to dictionary", "spelling.word.save"));
    let elsewhere = MAIN.find("Serve").unwrap();
    let listed = editor.dispatch("problems.fixes.list", json!({ "path": editor.file, "offset": elsewhere }));
    assert_eq!(listed, json!([]), "a typo's fixes are offered on the word, not anywhere on its line");
}

#[test]
fn a_typo_is_changed_to_a_suggestion_and_undone() {
    let mut editor = Editor::new("main.go", MAIN);
    editor.typos();
    let comment = MAIN.find("reqeusts").unwrap();
    let typo = editor.dispatch("spelling.typo.get", json!({ "path": editor.file, "offset": comment + 3 }));
    assert_eq!(typo["word"], "reqeusts");
    assert_eq!(typo["context"], "comment");
    assert_eq!(typo["suggestions"][0], "requests", "{typo}");
    assert!(typo["name"].is_null());
    let changed =
        editor.dispatch("spelling.typo.change", json!({ "path": editor.file, "offset": comment, "to": "requests" }));
    assert_eq!(changed["word"], "reqeusts");
    let text =
        |editor: &Editor| editor.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
    assert_eq!(text(&editor), MAIN.replace("reqeusts", "requests"));
    assert_eq!(editor.words()[0].0, "Hadnler", "the changed word is no longer a typo");
    assert_eq!(editor.dispatch("editor.edit.undo", json!({})), true);
    assert_eq!(text(&editor), MAIN, "one ⌘Z brings the typo back");
    let error = editor
        .registry
        .dispatch(&mut editor.session, "spelling.typo.change", json!({ "path": editor.file, "offset": 1, "to": "x" }))
        .unwrap_err();
    assert!(error.to_string().contains("There is no typo there"), "{error}");
    assert_eq!(editor.dispatch("spelling.suggestions.list", json!({ "word": "Wrold", "limit": 2 }))[0], "World");
}

#[test]
fn a_typo_in_a_declared_name_offers_the_whole_name_to_rename_to() {
    let mut editor = Editor::new("main.go", MAIN);
    let name = MAIN.find("parseUrlHadnler").unwrap();
    let typo = editor.dispatch("spelling.typo.get", json!({ "path": editor.file, "offset": name + 9 }));
    assert_eq!(typo["word"], "Hadnler");
    assert_eq!(typo["name"]["name"], "parseUrlHadnler");
    assert_eq!(typo["name"]["start"], name);
    assert_eq!(typo["name"]["end"], name + "parseUrlHadnler".len());
    assert_eq!(typo["name"]["renames"][0], "parseUrlHandler", "{typo}");
    assert_eq!(typo["suggestions"][0], "Handler");
}

#[test]
fn a_word_saved_to_the_application_dictionary_is_no_typo_until_undone() {
    let mut fixture = Dictionaries::new();
    assert_eq!(fixture.words(), ["reqeusts", "Hadnler", "mesage", "wrold"]);
    let file = fixture.editor.file.clone();
    let saved = fixture.editor.dispatch("spelling.word.save", json!({ "word": "Hadnler", "path": file }));
    assert_eq!(
        saved,
        json!({ "word": "hadnler", "level": "application", "file": fixture.application(), "added": true })
    );
    assert_eq!(std::fs::read_to_string(fixture.application()).unwrap(), "hadnler\n");
    assert_eq!(fixture.words(), ["reqeusts", "mesage", "wrold"], "Hadnler is known now");
    assert_eq!(
        fixture.editor.dispatch("spelling.words.check", json!({ "words": ["hadnlers", "wrold"] })),
        json!(["wrold"])
    );
    assert_eq!(fixture.editor.dispatch("editor.edit.undo", json!({})), true, "⌘Z takes the word out again");
    assert_eq!(std::fs::read_to_string(fixture.application()).unwrap(), "");
    assert_eq!(fixture.words(), ["reqeusts", "Hadnler", "mesage", "wrold"]);
    let text = fixture.editor.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
    assert_eq!(text, MAIN, "the undo left the text alone");
    fixture.editor.dispatch("spelling.word.save", json!({ "word": "mesage", "path": file }));
    fixture
        .editor
        .session
        .state_mut::<Workbench>()
        .active_document_mut()
        .unwrap()
        .replace_ranges(vec![(0..0, "\n".to_owned())]);
    fixture.editor.dispatch("editor.edit.undo", json!({}));
    assert_eq!(std::fs::read_to_string(fixture.application()).unwrap(), "mesage\n", "after an edit ⌘Z undoes the edit");
    let again = fixture.editor.dispatch("spelling.word.save", json!({ "word": "MESAGE" }));
    assert_eq!(again["added"], false);
    assert_eq!(
        fixture.editor.dispatch("spelling.word.remove", json!({ "word": "Mesage" })),
        json!([fixture.application()])
    );
    assert!(fixture.words().contains(&"mesage".to_owned()));
    let error = fixture.editor.registry.dispatch(
        &mut fixture.editor.session,
        "spelling.word.save",
        json!({ "word": "two words" }),
    );
    assert!(error.is_err());
}

#[test]
fn the_project_dictionary_is_read_and_written_in_the_idea_format() {
    let mut fixture = Dictionaries::new();
    let folder = fixture.project().parent().unwrap().to_owned();
    std::fs::create_dir_all(&folder).unwrap();
    let ada = "<component name=\"ProjectDictionaryState\">\n  <dictionary name=\"ada\">\n    <words>\n      <w>wrold</w>\n    </words>\n  </dictionary>\n</component>";
    std::fs::write(folder.join("ada.xml"), ada).unwrap();
    std::fs::write(folder.join("notes.txt"), "mesage").unwrap();
    let state = fixture.editor.dispatch("spelling.dictionaries.get", json!({}));
    assert_eq!(state["project"]["words"], json!(["wrold"]), "{state}");
    assert_eq!(
        fixture.words(),
        ["reqeusts", "Hadnler", "mesage"],
        "a user dictionary in the project's .idea folder counts"
    );
    let file = fixture.editor.file.clone();
    let saved =
        fixture.editor.dispatch("spelling.word.save", json!({ "word": "reqeusts", "level": "project", "path": file }));
    assert_eq!(saved["file"], json!(fixture.project()));
    assert_eq!(
        std::fs::read_to_string(fixture.project()).unwrap(),
        "<component name=\"ProjectDictionaryState\">\n  <dictionary name=\"project\">\n    <words>\n      <w>reqeusts</w>\n    </words>\n  </dictionary>\n</component>\n"
    );
    fixture.editor.set(SINGLE_DICTIONARY, true);
    fixture.editor.session.settings_mut().set(SAVING_DICTIONARY, "project").unwrap();
    fixture.editor.dispatch("spelling.word.save", json!({ "word": "Hadnler" }));
    assert_eq!(
        zen_spell::idea_dictionary::words(&std::fs::read_to_string(fixture.project()).unwrap()).unwrap(),
        ["hadnler", "reqeusts"]
    );
    assert_eq!(fixture.words(), ["mesage"]);
    let removed = fixture.editor.dispatch("spelling.word.remove", json!({ "word": "wrold" }));
    assert_eq!(removed, json!([folder.join("ada.xml")]));
    assert_eq!(
        std::fs::read_to_string(folder.join("ada.xml")).unwrap(),
        ada.replace("    <words>\n      <w>wrold</w>\n    </words>\n", "")
    );
    assert_eq!(fixture.editor.dispatch("editor.edit.undo", json!({})), true, "the word saved from the file goes");
    assert_eq!(
        zen_spell::idea_dictionary::words(&std::fs::read_to_string(fixture.project()).unwrap()).unwrap(),
        ["hadnler"]
    );
    assert!(!fixture.application().exists(), "nothing went to the application dictionary");
}

#[test]
fn a_change_made_outside_is_read_again() {
    let mut fixture = Dictionaries::new();
    assert_eq!(fixture.words().len(), 4);
    std::fs::create_dir_all(fixture.application().parent().unwrap()).unwrap();
    std::fs::write(fixture.application(), "mesage\nreqeusts\n").unwrap();
    let state = fixture.editor.dispatch("spelling.dictionaries.get", json!({}));
    assert_eq!(state["application"]["words"], json!(["mesage", "reqeusts"]));
    assert_eq!(fixture.words(), ["Hadnler", "wrold"]);
}

#[test]
fn custom_dictionaries_are_plain_word_lists_or_hunspell() {
    let mut fixture = Dictionaries::new();
    let folder = fixture.data.path().join("custom");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("words.dic"), "# team words\nmesage\n").unwrap();
    std::fs::write(folder.join("en_ZZ.aff"), "SET UTF-8\nSFX S Y 1\nSFX S 0 s .\n").unwrap();
    std::fs::write(folder.join("en_ZZ.dic"), "1\nreqeust/S\n").unwrap();
    std::fs::write(folder.join("broken.dic"), [0xffu8, 0xfe]).unwrap();
    let paths = [folder.join("words.dic"), folder.join("en_ZZ.dic"), folder.join("missing.dic")];
    fixture.editor.session.settings_mut().set(CUSTOM_DICTIONARIES, &paths).unwrap();
    assert_eq!(fixture.words(), ["Hadnler", "wrold"], "mesage from the list, reqeusts from the Hunspell affix");
    let state = fixture.editor.dispatch("spelling.dictionaries.get", json!({}));
    let custom = state["custom"].as_array().unwrap();
    assert_eq!(custom.len(), 3);
    assert_eq!((custom[0]["format"].as_str(), custom[0]["words"].as_u64()), (Some("plain"), Some(1)));
    assert_eq!((custom[1]["format"].as_str(), custom[1]["words"].as_u64()), (Some("hunspell"), Some(2)));
    assert!(custom[2]["error"].is_string(), "{state}");
    let saved = fixture.editor.dispatch("spelling.word.save", json!({ "word": "wrold" }));
    assert_eq!(saved["file"], json!(fixture.application()), "custom dictionaries are never written");
    assert_eq!(std::fs::read_to_string(folder.join("words.dic")).unwrap(), "# team words\nmesage\n");
}

#[cfg(unix)]
#[test]
fn a_linked_dictionary_folder_is_neither_read_nor_written() {
    let mut fixture = Dictionaries::new();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(
        outside.path().join("project.xml"),
        zen_spell::idea_dictionary::new_file("x", &["wrold".to_owned()]),
    )
    .unwrap();
    let idea = fixture.editor.file.parent().unwrap().join(".idea");
    std::fs::create_dir_all(&idea).unwrap();
    std::os::unix::fs::symlink(outside.path(), idea.join("dictionaries")).unwrap();
    assert_eq!(fixture.words().len(), 4, "the linked folder is not read");
    let error = fixture
        .editor
        .registry
        .dispatch(&mut fixture.editor.session, "spelling.word.save", json!({ "word": "mesage", "level": "project" }))
        .unwrap_err();
    assert!(error.to_string().contains("symbolic link"), "{error}");
    assert_eq!(
        zen_spell::idea_dictionary::words(&std::fs::read_to_string(outside.path().join("project.xml")).unwrap())
            .unwrap(),
        ["wrold"]
    );
}
