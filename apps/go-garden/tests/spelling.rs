use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_syntax::Highlighter;
use zen_workbench::brackets::Highlighters;
use zen_workbench::spelling::{self, Spelling};

const GO: &str = r#"package main

import "fmt"

// parseUrlHadnler reads the adress of a reqeust, see https://exampel.com/hadnler or mail
// hadnler@exampel.com; the HTTP JSON id 0xDEADBEEF and 3fa9c0d2 are skipped, so is ok.
func parseUrlHadnler(raw string) (string, error) {
	mesage := "cannot parse the wrold: %v\n"
	if raw == "" {
		return "", fmt.Errorf(mesage, raw)
	}
	return raw, nil
}

type requestHandler struct {
	MAX_RETRY_CUONT int
}
"#;

const RUST: &str = r#"//! Readers of the confgiuration.

/// Parses a URL with a hadnler.
pub fn parse_url_hadnler(raw: &str) -> String {
    let mesage = format!("cannot parse the wrold: {raw}");
    println!("{mesage}");
    mesage
}

struct ReqeustReader;
"#;

fn dispatch(registry: &Registry, session: &mut Session, id: &str, params: Value) -> Value {
    registry.dispatch(session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
}

fn open(files: &[(&str, &str)]) -> (Registry, Session, Vec<PathBuf>, tempfile::TempDir) {
    zen_i18n::register(zen_workbench::LOCALES);
    let dir = tempfile::tempdir().unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    dispatch(&registry, &mut session, "file.project.open", json!({ "path": dir.path() }));
    let go: Arc<dyn Highlighter> = Arc::new(zen_lang_go::GoHighlighter);
    let rust: Arc<dyn Highlighter> = Arc::new(zen_lang_rust::RustHighlighter);
    session.state_mut::<Highlighters>().set_source(Arc::new(move |name| {
        [&go, &rust].into_iter().find(|highlighter| highlighter.handles(name)).cloned()
    }));
    session.state_mut::<Spelling>().activate();
    let mut paths = Vec::new();
    for (name, text) in files {
        let path = dir.path().join(name);
        std::fs::write(&path, text).unwrap();
        dispatch(&registry, &mut session, "file.document.open", json!({ "path": path }));
        paths.push(path);
    }
    (registry, session, paths, dir)
}

fn typos(registry: &Registry, session: &mut Session, path: &Path) -> Vec<(String, String, u64)> {
    let answer = dispatch(registry, session, "spelling.typos.list", json!({ "path": path }));
    assert_eq!(answer["checked"], true, "{answer}");
    answer["typos"]
        .as_array()
        .unwrap()
        .iter()
        .map(|typo| {
            (
                typo["word"].as_str().unwrap().to_owned(),
                typo["context"].as_str().unwrap().to_owned(),
                typo["line"].as_u64().unwrap(),
            )
        })
        .collect()
}

fn expected(list: &[(&str, &str, u64)]) -> Vec<(String, String, u64)> {
    list.iter().map(|(word, context, line)| ((*word).to_owned(), (*context).to_owned(), *line)).collect()
}

#[test]
fn go_comments_literals_and_declared_names_are_checked_and_the_rest_skipped() {
    let (registry, mut session, paths, _dir) = open(&[("main.go", GO)]);
    assert_eq!(
        typos(&registry, &mut session, &paths[0]),
        expected(&[
            ("Hadnler", "comment", 5),
            ("adress", "comment", 5),
            ("reqeust", "comment", 5),
            ("Hadnler", "code", 7),
            ("mesage", "code", 8),
            ("wrold", "literal", 8),
        ]),
        "only Hadnler of parseUrlHadnler; the URL, the e-mail address, words in capitals, hex numbers, short \
         words, the used Errorf and the format verb are skipped"
    );
}

#[test]
fn rust_comments_literals_and_declared_names_are_checked() {
    let (registry, mut session, paths, _dir) = open(&[("lib.rs", RUST)]);
    assert_eq!(
        typos(&registry, &mut session, &paths[0]),
        expected(&[
            ("confgiuration", "comment", 1),
            ("hadnler", "comment", 3),
            ("hadnler", "code", 4),
            ("mesage", "code", 5),
            ("wrold", "literal", 5),
            ("mesage", "literal", 6),
            ("Reqeust", "code", 10),
        ])
    );
}

#[test]
fn f2_steps_through_the_typos_and_the_problems_lists_keep_them_apart_from_errors() {
    zen_i18n::set_language(Some("en"));
    let (registry, mut session, paths, _dir) = open(&[("main.go", GO)]);
    let starts: Vec<u64> =
        dispatch(&registry, &mut session, "spelling.typos.list", json!({ "path": paths[0] }))["typos"]
            .as_array()
            .unwrap()
            .iter()
            .map(|typo| typo["start"].as_u64().unwrap())
            .collect();
    let next: Vec<u64> = (0..starts.len() + 1)
        .map(|_| dispatch(&registry, &mut session, "editor.problem.next", json!({}))["start"].as_u64().unwrap())
        .collect();
    assert_eq!(next[..starts.len()], starts[..]);
    assert_eq!(next[starts.len()], starts[0], "F2 goes round to the first");
    let previous = dispatch(&registry, &mut session, "editor.problem.previous", json!({}));
    assert_eq!(previous["start"].as_u64(), starts.last().copied(), "⇧F2 goes round to the last");

    let file = dispatch(&registry, &mut session, "problems.file.list", json!({ "path": paths[0] }));
    let listed = zen_workbench::language::listed_problems(&file);
    assert_eq!(listed.len(), starts.len());
    assert!(listed.iter().all(|problem| problem["severity"] == "typo"), "{file}");
    assert_eq!(listed[3]["message"], "Possible misspelling: 'Hadnler'");
    let project = dispatch(&registry, &mut session, "problems.project.list", json!({}));
    assert_eq!(project, json!([]), "Project Errors has no typos");
}

#[test]
fn the_three_process_settings_and_the_inspection_switch() {
    let (registry, mut session, paths, _dir) = open(&[("main.go", GO), ("lib.rs", RUST)]);
    let contexts = |registry: &Registry, session: &mut Session, path: &Path| {
        let mut found: Vec<String> = typos(registry, session, path).into_iter().map(|typo| typo.1).collect();
        found.dedup();
        found
    };
    for (key, left) in [
        (spelling::PROCESS_CODE, ["comment", "literal"]),
        (spelling::PROCESS_LITERALS, ["code", "comment"]),
        (spelling::PROCESS_COMMENTS, ["code", "literal"]),
    ] {
        session.settings_mut().set(key, false).unwrap();
        for path in &paths {
            let mut found = contexts(&registry, &mut session, path);
            found.sort();
            found.dedup();
            assert_eq!(found, left, "{key} off in {}", path.display());
        }
        session.settings_mut().set(key, true).unwrap();
    }
    session.settings_mut().set(spelling::ENABLED, false).unwrap();
    let answer = dispatch(&registry, &mut session, "spelling.typos.list", json!({ "path": paths[0] }));
    assert_eq!(answer, json!({ "checked": false, "typos": [] }));
    let file = dispatch(&registry, &mut session, "problems.file.list", json!({ "path": paths[0] }));
    assert!(zen_workbench::language::listed_problems(&file).is_empty(), "{file}");
}

fn large_go(lines: usize) -> String {
    let mut text = String::from("package main\n\nimport \"fmt\"\n\n");
    let mut index = 0;
    while text.lines().count() < lines {
        let typo = if index % 5 == 0 { "hadnler" } else { "handler" };
        text.push_str(&format!(
            "// process{index} runs the {typo} for the request and returns its answer.\nfunc process{index}(value int) string {{\n\tanswer := fmt.Sprintf(\"value %d of the {typo}\", value)\n\tif value > 10 {{\n\t\treturn answer + \" is large\"\n\t}}\n\treturn answer\n}}\n\n"
        ));
        index += 1;
    }
    text
}

#[test]
#[ignore = "a measurement"]
#[allow(clippy::print_stdout, reason = "a measurement, read on the terminal")]
fn typing_in_a_large_file_does_not_wait_for_the_spelling_check() {
    let text = large_go(10_000);
    let (registry, mut session, paths, _dir) = open(&[("main.go", &text)]);
    let started = std::time::Instant::now();
    let first = typos(&registry, &mut session, &paths[0]);
    let whole = started.elapsed();
    assert_eq!(first.len(), 2 * text.matches("// process").count().div_ceil(5));
    let measure = |session: &mut Session, active: bool| {
        session.state_mut::<Spelling>().activate();
        if !active {
            session.settings_mut().set(spelling::ENABLED, false).unwrap();
        }
        let mut times = Vec::new();
        for key in 0..200 {
            let edit = if key % 2 == 0 { json!({ "text": "x" }) } else { json!({}) };
            let id = if key % 2 == 0 { "editor.edit.insert" } else { "editor.edit.deleteBackward" };
            dispatch(&registry, session, "editor.selection.set", json!({ "selections": [{ "head": text.len() / 2 }] }));
            let _ = registry.dispatch(session, id, edit);
            let started = std::time::Instant::now();
            dispatch(&registry, session, "problems.file.list", json!({ "path": paths[0] }));
            times.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        session.settings_mut().set(spelling::ENABLED, true).unwrap();
        times.sort_by(f64::total_cmp);
        (times[times.len() / 2], times[times.len() * 95 / 100], times[times.len() - 1])
    };
    let mut still = Vec::new();
    for _ in 0..50 {
        let started = std::time::Instant::now();
        dispatch(&registry, &mut session, "problems.file.list", json!({ "path": paths[0] }));
        still.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    still.sort_by(f64::total_cmp);
    println!("problems of a frame without a keystroke, p50 {:.2} ms", still[still.len() / 2]);
    let on = measure(&mut session, true);
    let off = measure(&mut session, false);
    println!(
        "10 000 lines, {} typos; the whole check in the background: {:.1} ms",
        first.len(),
        whole.as_secs_f64() * 1000.0
    );
    println!("problems of a keystroke, p50 / p95 / max ms: spelling on {on:.2?}, off {off:.2?}");
    assert!(on.1 < 16.0, "the window's thread never waits for the check: p95 {:.2} ms", on.1);
}

#[test]
#[ignore = "a measurement"]
#[allow(clippy::print_stdout, reason = "a measurement, read on the terminal")]
fn spelling_of_this_repository() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap();
    let listed =
        std::process::Command::new("git").args(["ls-files", "*.rs", "*.md"]).current_dir(&root).output().unwrap();
    let files: Vec<PathBuf> = String::from_utf8(listed.stdout).unwrap().lines().map(|line| root.join(line)).collect();
    let (registry, mut session, _, _dir) = open(&[]);
    let markdown = ["release", "debug"]
        .iter()
        .map(|profile| root.join(format!("target/wasm32-unknown-unknown/{profile}/zen_plugin_markdown.wasm")))
        .find(|path| path.exists())
        .expect("the Markdown plug-in, built by cargo build --target wasm32-unknown-unknown -p zen-plugin-markdown");
    let markdown: Arc<dyn Highlighter> = Arc::new(
        zen_plugins::WasmHighlighter::load("markdown", &std::fs::read(markdown).unwrap(), &["md".to_owned()]).unwrap(),
    );
    let go: Arc<dyn Highlighter> = Arc::new(zen_lang_go::GoHighlighter);
    let rust: Arc<dyn Highlighter> = Arc::new(zen_lang_rust::RustHighlighter);
    session.state_mut::<Highlighters>().set_source(Arc::new(move |name| {
        [&go, &rust, &markdown].into_iter().find(|highlighter| highlighter.handles(name)).cloned()
    }));
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut contexts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut found_in = 0;
    let mut listing = Vec::new();
    for path in &files {
        dispatch(&registry, &mut session, "file.document.open", json!({ "path": path }));
        let answer =
            dispatch(&registry, &mut session, "spelling.typos.list", json!({ "path": path, "waitMs": 60_000 }));
        assert_eq!(answer["checked"], true, "{}", path.display());
        let typos = answer["typos"].as_array().unwrap();
        found_in += usize::from(!typos.is_empty());
        for typo in typos {
            let (word, context) = (typo["word"].as_str().unwrap(), typo["context"].as_str().unwrap());
            *counts.entry(word.to_ascii_lowercase()).or_default() += 1;
            *contexts.entry(context.to_owned()).or_default() += 1;
            listing.push(format!(
                "{word}\t{}:{}\t{context}",
                path.strip_prefix(&root).unwrap().display(),
                typo["line"]
            ));
        }
        dispatch(&registry, &mut session, "file.document.close", json!({ "path": path }));
    }
    if let Ok(out) = std::env::var("ZEN_SPELLING_TYPOS") {
        std::fs::write(out, listing.join("\n")).unwrap();
    }
    let mut ranked: Vec<(&String, &usize)> = counts.iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!(
        "{} files, {found_in} with typos: {} typos, {} different words, by context {contexts:?}",
        files.len(),
        listing.len(),
        counts.len()
    );
    for (word, count) in ranked.iter().take(40) {
        println!("{count:>6} {word}");
    }
}
