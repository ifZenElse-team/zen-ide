use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_modules::folding::{self, FoldingGroups};

type Expected = Vec<(&'static str, Vec<(&'static str, Option<&'static str>)>)>;

const GO: &str = r#"// Copyright 2026 Fictional Holder Ltd, an invented name.
// Licensed under the Apache License, Version 2.0.

// Package server serves the example.
package server

import (
	"errors"
	"fmt"
	"net/http"
)

// Server answers requests.
// It is safe to use from several goroutines.
type Server struct {
	name string
}

type Empty struct {
}

type Handler interface {
}

// Start runs the server.
func (s *Server) Start(port int) error {
	if err := listen(port); err != nil {
		return fmt.Errorf("start %s on %d: %w", s.name, port, err)
	}
	switch port {
	case 80:
		return errors.New("privileged")
	case 8080, 8081:
		s.name = "dev"
		s.name += "!"
	default:
		fmt.Printf("port %d\n", port)
	}
	return nil
}

func (s *Server) Name() string {
	return s.name
}

func noop() {
}

// region Handlers
func handle(w http.ResponseWriter) {
	fmt.Fprintf(w, "hello %s", "world")
}
// endregion

func listen(int) error {
	return nil
}
"#;

const RUST: &str = r#"// Copyright 2026 Fictional Holder Ltd, an invented name.
// Licensed under the MIT License.

use std::fmt;
use std::io::{
    self,
    Write,
};

/// A server.
/// It answers requests.
pub struct Server {
    name: String,
}

impl Server {
    /// Starts it.
    /// On the port given.
    pub fn start(&self, port: u16) -> io::Result<()> {
        writeln!(io::stdout(), "{}", port)?;
        Ok(())
    }
}

// region Helpers
fn helper(
    a: u8,
) -> u8 {
    a
}
// endregion

impl fmt::Display for Server {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}
"#;

fn every_option() -> Vec<&'static str> {
    zen_workbench::folding::GENERAL
        .options
        .iter()
        .chain(zen_lang_go::module::FOLDING.options)
        .map(|option| option.key)
        .collect()
}

fn open_session(on: &[&str]) -> (Registry, Session) {
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.state_mut::<zen_workbench::brackets::Highlighters>().set_source(Arc::new(|name: &str| {
        if name.ends_with(".go") {
            Some(Arc::new(zen_lang_go::GoHighlighter) as _)
        } else if name.ends_with(".rs") {
            Some(Arc::new(zen_lang_rust::RustHighlighter) as _)
        } else {
            None
        }
    }));
    session.set_state(FoldingGroups(vec![zen_lang_go::module::FOLDING]));
    for key in every_option() {
        session.settings_mut().set(key, on.contains(&key)).unwrap();
    }
    (registry, session)
}

fn write(folder: &Path, name: &str, text: &str) -> PathBuf {
    let path = folder.join(name);
    std::fs::write(&path, text).unwrap();
    path
}

fn folded(registry: &Registry, session: &mut Session, text: &str) -> Vec<(String, Option<String>, String)> {
    let folds: Value = registry.dispatch(session, "editor.fold.list", json!({})).unwrap();
    folds
        .as_array()
        .unwrap()
        .iter()
        .filter(|fold| fold["folded"] == true)
        .map(|fold| {
            let (start, end) = (fold["start"].as_u64().unwrap() as usize, fold["end"].as_u64().unwrap() as usize);
            let hidden: String = text.chars().skip(start).take(end - start).collect();
            (hidden, fold["placeholder"].as_str().map(str::to_owned), fold["option"].as_str().unwrap_or("").to_owned())
        })
        .collect()
}

fn opened_with(name: &str, text: &str, on: &[&str]) -> Vec<(String, Option<String>, String)> {
    let folder = tempfile::tempdir().unwrap();
    let path = write(folder.path(), name, text);
    let (registry, mut session) = open_session(on);
    registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
    folded(&registry, &mut session, text)
}

fn shown(folds: &[(String, Option<String>, String)]) -> Vec<(&str, Option<&str>)> {
    folds.iter().map(|(hidden, placeholder, _)| (hidden.as_str(), placeholder.as_deref())).collect()
}

#[test]
fn each_go_option_folds_its_regions_of_a_go_file_when_it_first_opens() {
    let expected: Expected = vec![
        (
            folding::FILE_HEADER,
            vec![(
                "// Copyright 2026 Fictional Holder Ltd, an invented name.\n// Licensed under the Apache License, Version 2.0.",
                None,
            )],
        ),
        (folding::IMPORTS, vec![("(\n\t\"errors\"\n\t\"fmt\"\n\t\"net/http\"\n)", None)]),
        (folding::DOC_COMMENTS, vec![(" answers requests.\n// It is safe to use from several goroutines.", None)]),
        (
            folding::METHOD_BODIES,
            vec![
                (
                    &GO[GO.find("error {\n").unwrap() + 7
                        ..GO.find("\treturn nil\n}\n\nfunc (s *Server) Name").unwrap() + 12],
                    None,
                ),
                ("\n\tfmt.Fprintf(w, \"hello %s\", \"world\")\n", None),
            ],
        ),
        (
            folding::CUSTOM_REGIONS,
            vec![(&GO[GO.find("// region").unwrap()..GO.find("// endregion").unwrap() + 12], Some("Handlers"))],
        ),
        (
            zen_lang_go::module::IF_ERRORS,
            vec![(
                "\n\t\treturn fmt.Errorf(\"start %s on %d: %w\", s.name, port, err)\n\t",
                Some(" return fmt.Errorf(\"start %s on %d: %w\", s.name, port, err) "),
            )],
        ),
        (
            zen_lang_go::module::ONE_LINE_RETURNS,
            vec![("\n\treturn s.name\n", Some(" return s.name ")), ("\n\treturn nil\n", Some(" return nil "))],
        ),
        (
            zen_lang_go::module::ONE_LINE_CASES,
            vec![
                ("\n\t\treturn errors.New(\"privileged\")", Some(" return errors.New(\"privileged\")")),
                ("\n\t\tfmt.Printf(\"port %d\\n\", port)", Some(" fmt.Printf(\"port %d\\n\", port)")),
            ],
        ),
        (zen_lang_go::module::EMPTY_FUNCTIONS, vec![("\n", Some(""))]),
        (zen_lang_go::module::EMPTY_TYPES, vec![("\n", Some("")), ("\n", Some(""))]),
        (
            zen_lang_go::module::FORMATTED_STRINGS,
            vec![
                ("\"start %s on %d: %w\", s.name, port, err", Some("\"start {s.name} on {port}: {err}\"")),
                ("\"port %d\\n\", port", Some("\"port {port}\\n\"")),
                ("\"hello %s\", \"world\"", Some("\"hello {\"world\"}\"")),
            ],
        ),
    ];
    assert_eq!(expected.len(), every_option().len(), "every option is checked");
    for (option, regions) in expected {
        let folds = opened_with("server.go", GO, &[option]);
        assert_eq!(shown(&folds), regions, "{option}");
        assert!(folds.iter().all(|(.., tag)| tag == option), "{option}: {folds:?}");
    }
    assert_eq!(opened_with("server.go", GO, &[]), [], "with every option off nothing starts folded");
}

#[test]
fn each_general_option_folds_its_regions_of_a_rust_file_when_it_first_opens() {
    let expected: Expected = vec![
        (
            folding::FILE_HEADER,
            vec![(
                "// Copyright 2026 Fictional Holder Ltd, an invented name.\n// Licensed under the MIT License.",
                None,
            )],
        ),
        (folding::IMPORTS, vec![("std::fmt;\nuse std::io::{\n    self,\n    Write,\n};", None)]),
        (
            folding::DOC_COMMENTS,
            vec![
                ("/// A server.\n/// It answers requests.", None),
                ("/// Starts it.\n    /// On the port given.", None),
            ],
        ),
        (
            folding::METHOD_BODIES,
            vec![
                ("\n        writeln!(io::stdout(), \"{}\", port)?;\n        Ok(())\n    ", None),
                ("\n    a\n", None),
                ("\n        write!(f, \"{}\", self.name)\n    ", None),
            ],
        ),
        (
            folding::CUSTOM_REGIONS,
            vec![(&RUST[RUST.find("// region").unwrap()..RUST.find("// endregion").unwrap() + 12], Some("Helpers"))],
        ),
    ];
    for (option, regions) in expected {
        let folds = opened_with("server.rs", RUST, &[option]);
        assert_eq!(shown(&folds), regions, "{option}");
    }
    let defaults = {
        let folder = tempfile::tempdir().unwrap();
        let path = write(folder.path(), "server.rs", RUST);
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
        folded(&registry, &mut session, RUST).into_iter().map(|(.., option)| option).collect::<Vec<_>>()
    };
    assert_eq!(defaults, [folding::IMPORTS], "by default only the imports");
}

#[test]
fn folds_the_user_changed_are_kept_when_the_file_opens_again() {
    let folder = tempfile::tempdir().unwrap();
    let path = write(folder.path(), "server.go", GO);
    let (registry, mut session) = open_session(&[folding::IMPORTS, zen_lang_go::module::IF_ERRORS]);
    registry.dispatch(&mut session, "file.document.open", json!({ "path": &path })).unwrap();
    let options =
        |folds: Vec<(String, Option<String>, String)>| folds.into_iter().map(|(.., option)| option).collect::<Vec<_>>();
    assert_eq!(options(folded(&registry, &mut session, GO)), [folding::IMPORTS, zen_lang_go::module::IF_ERRORS]);
    let list: Value = registry.dispatch(&mut session, "editor.fold.list", json!({})).unwrap();
    let start_of =
        |option: &str| list.as_array().unwrap().iter().find(|fold| fold["option"] == option).unwrap()["start"].clone();
    registry.dispatch(&mut session, "editor.fold.toggle", json!({ "start": start_of(folding::IMPORTS) })).unwrap();
    registry
        .dispatch(
            &mut session,
            "editor.fold.toggle",
            json!({ "start": start_of(zen_lang_go::module::ONE_LINE_RETURNS) }),
        )
        .unwrap();
    registry.dispatch(&mut session, "file.document.close", json!({})).unwrap();
    for key in every_option() {
        session.settings_mut().set(key, true).unwrap();
    }
    registry.dispatch(&mut session, "file.document.open", json!({ "path": &path })).unwrap();
    assert_eq!(
        options(folded(&registry, &mut session, GO)),
        [zen_lang_go::module::IF_ERRORS, zen_lang_go::module::ONE_LINE_RETURNS],
        "the imports stay unfolded, the body folded by hand stays folded, and the options turned on since do not apply"
    );
    let remembered: Value = registry.dispatch(&mut session, "editor.fold.remembered", json!({})).unwrap();
    let (registry, mut restarted) = open_session(&[]);
    registry.dispatch(&mut restarted, "editor.fold.remember", json!({ "files": remembered })).unwrap();
    registry.dispatch(&mut restarted, "file.document.open", json!({ "path": &path })).unwrap();
    assert_eq!(
        options(folded(&registry, &mut restarted, GO)),
        [zen_lang_go::module::IF_ERRORS, zen_lang_go::module::ONE_LINE_RETURNS],
        "and so after a restart, from the workspace"
    );
}

#[test]
fn the_options_command_lists_go_after_general_with_their_values() {
    let (registry, mut session) = open_session(&[zen_lang_go::module::FORMATTED_STRINGS]);
    zen_i18n::register(zen_lang_go::LOCALES);
    zen_i18n::set_language(Some("en"));
    let listed: Value = registry.dispatch(&mut session, "editor.folding.options", json!({})).unwrap();
    let groups = listed["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    let go = &groups[1];
    assert_eq!(go["title"], "Go");
    let labels: Vec<&str> =
        go["options"].as_array().unwrap().iter().map(|option| option["label"].as_str().unwrap()).collect();
    assert_eq!(
        labels,
        [
            "One-line 'if' blocks for error handling",
            "One-line functions with a single 'return'",
            "One-line case clauses",
            "Empty functions",
            "Empty struct or interface type definitions",
            "Formatted strings",
        ]
    );
    let on: Vec<&str> = groups
        .iter()
        .flat_map(|group| group["options"].as_array().unwrap())
        .filter(|option| option["value"] == true)
        .map(|option| option["key"].as_str().unwrap())
        .collect();
    assert_eq!(on, [zen_lang_go::module::FORMATTED_STRINGS]);
}
