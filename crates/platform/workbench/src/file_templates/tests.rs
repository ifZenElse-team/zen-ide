use std::collections::HashMap;
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use serde_json::{Value, json};
use zen_core::{Registry, Session};

use super::velocity::{self, references, render};
use super::*;
use crate::Workbench;

fn values(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
}

fn no_includes(_: &str) -> Option<String> {
    None
}

fn plain(text: &str, pairs: &[(&str, &str)]) -> velocity::Rendered {
    zen_i18n::register(crate::LOCALES);
    render(text, &values(pairs), &no_includes)
}

#[test]
fn references_braced_plain_quiet_and_the_dollar_sign() {
    let out =
        plain("${NAME} $NAME-$!QUIET $!{QUIET}${DS}x $5 $ ${NAME}.go $NAME.go", &[("NAME", "Order"), ("DS", "$")]);
    assert_eq!(out.text, "Order Order- $x $5 $ Order.go Order.go");
    assert_eq!(out.missing, ["QUIET"]);
    assert!(out.problems.is_empty(), "{:?}", out.problems);
    let unknown = plain("Hello, $WHO and ${WHO}!", &[]);
    assert_eq!(unknown.text, "Hello, $WHO and ${WHO}!", "a variable without a value stays as written");
    assert_eq!(unknown.missing, ["WHO"]);
}

#[test]
fn set_if_elseif_else_and_lines_with_only_a_directive() {
    let text = "#set( $PACKAGE = \"pkg_${NAME}\" )\n#set($FLAG = true)\npackage $PACKAGE\n  #if ($KIND == \"test\")\n// test\n  #elseif($KIND != \"main\" && !$SKIP || $FORCE)\n// lib\n#else\n// main\n#end\n#if($FLAG)ok#end\n";
    let run = |kind: &str| plain(text, &[("NAME", "orders"), ("KIND", kind)]);
    assert_eq!(run("test").text, "package pkg_orders\n// test\nok\n");
    assert_eq!(run("lib").text, "package pkg_orders\n// lib\nok\n");
    assert_eq!(run("main").text, "package pkg_orders\n// main\nok\n");
    let forced = plain(text, &[("NAME", "x"), ("KIND", "main"), ("FORCE", "yes")]);
    assert!(forced.text.contains("// lib"), "{}", forced.text);
    let words = plain("#if($A eq 'x' and not $B)yes#{else}no#{end}", &[("A", "x")]);
    assert_eq!(words.text, "yes");
    let empty = plain("#if($E)set#else empty#end", &[("E", "")]);
    assert_eq!(empty.text, " empty", "an empty value is false");
}

#[test]
fn parse_takes_an_include_by_its_name_with_or_without_the_extension() {
    let includes: HashMap<String, String> = [
        ("File Header.go".to_owned(), "// Created by ${USER}\n".to_owned()),
        ("Loop".to_owned(), "#parse(\"Loop\")".to_owned()),
    ]
    .into();
    let find = |name: &str| includes.get(name).cloned();
    zen_i18n::register(crate::LOCALES);
    let out = render("#parse(\"File Header.go\")\npackage main\n", &values(&[("USER", "ada")]), &find);
    assert_eq!(out.text, "// Created by ada\npackage main\n");
    assert!(out.problems.is_empty());
    let looped = render("x\n#parse('Loop')\n", &HashMap::new(), &find);
    assert!(looped.problems.iter().any(|problem| problem.message.contains("deeply")), "{:?}", looped.problems);
    let missing = render("a\n  #parse(\"Nope\")\n", &HashMap::new(), &find);
    assert_eq!((missing.problems[0].line, missing.problems[0].column), (2, 3));
    assert!(missing.problems[0].message.contains("Nope"));
}

#[test]
fn comments_raw_text_and_escapes() {
    let out = plain(
        "## a comment line\nkeep ## trailing\n#* block\ncomment *#\nlive #[[$VALUE$ $END$ #if]]# \\$NAME \\#if \\x $NAME\n",
        &[("NAME", "n")],
    );
    assert_eq!(out.text, "keep \nlive $VALUE$ $END$ #if $NAME #if \\x n\n");
    assert!(out.problems.is_empty(), "{:?}", out.problems);
}

#[test]
fn text_that_only_looks_like_velocity_stays_as_it_is() {
    let text = "#!/bin/sh\n# Title\n#include <stdio.h>\ncolor: #fff; price: $5; a#b ${ } $!\n";
    let out = plain(text, &[]);
    assert_eq!(out.text, text);
    assert!(out.problems.is_empty() && out.missing.is_empty(), "{out:?}");
}

#[test]
fn what_the_subset_lacks_is_reported_with_its_place() {
    let cases: &[(&str, (usize, usize), &str)] = &[
        ("a\n#foreach($x in $list)\n$x\n#end", (2, 1), "#foreach"),
        ("x ${NAME.toUpperCase()} y", (1, 3), "Methods"),
        ("x $NAME.trim() y", (1, 3), "Methods"),
        ("#set($A = $B + 1)", (1, 14), "expressions"),
        ("\n\n  #if($A)\nnever closed", (3, 3), "#end"),
        ("ok\n#end", (2, 1), "without"),
        ("#* never closed", (1, 1), "*#"),
        ("#[[ never closed", (1, 1), "]]#"),
        ("#set(\"x\" = 1)", (1, 1), "#set"),
        ("#if $A #end", (1, 1), "("),
        ("#set($A = \"open", (1, 11), "string"),
        ("#if($A < 3)x#end", (1, 8), "condition"),
        ("#macro(m)x#end", (1, 1), "#macro"),
    ];
    for (text, (line, column), says) in cases {
        let out = plain(text, &[("A", "1"), ("B", "2"), ("NAME", "n")]);
        let problem = out.problems.first().unwrap_or_else(|| panic!("{text:?} reports nothing"));
        assert_eq!((problem.line, problem.column), (*line, *column), "{text:?}: {problem:?}");
        assert!(problem.message.contains(says), "{text:?}: {}", problem.message);
    }
}

#[test]
fn the_variables_a_template_reads_are_found_in_order_in_every_branch_and_include() {
    let includes: HashMap<String, String> = [("Header".to_owned(), "// $AUTHOR".to_owned())].into();
    let find = |name: &str| includes.get(name).cloned();
    let text = "#set($LOCAL = \"x\")\n${LOCAL} ${FIRST}\n#if($MODE == \"a\")$ONLY_A#else$ONLY_B#end\n#parse(\"Header\")\n#set($LATE = $FIRST)$LATE $!QUIET";
    assert_eq!(references(text, &find), ["FIRST", "MODE", "ONLY_A", "ONLY_B", "AUTHOR", "QUIET"]);
}

fn alphabet() -> impl Strategy<Value = String> {
    let pieces = prop::sample::select(vec![
        "#", "$", "!", "{", "}", "(", ")", "[[", "]]", "*", "\"", "'", "\\", "\n", " ", "=", "==", "&&", "||", "set",
        "if", "elseif", "else", "end", "parse", "foreach", "NAME", "x", "1", ".", "é", "😀", "##", "#*", "*#", "\t",
    ]);
    prop::collection::vec(pieces, 0..60).prop_map(|pieces| pieces.concat())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2000, ..ProptestConfig::default() })]

    #[test]
    fn any_text_renders_or_reports_and_never_panics(text in alphabet()) {
        let includes: HashMap<String, String> = [("x".to_owned(), text.clone())].into();
        let find = |name: &str| includes.get(name).cloned();
        let out = render(&text, &values(&[("NAME", "n"), ("x", "x")]), &find);
        let lines = text.matches('\n').count() + 1;
        for problem in &out.problems {
            prop_assert!(problem.line >= 1 && problem.column >= 1, "{problem:?}");
            if problem.include.is_none() {
                prop_assert!(problem.line <= lines, "{problem:?} past the end of {text:?}");
            }
        }
        let _ = references(&text, &find);
    }

    #[test]
    fn text_without_velocity_characters_is_copied(text in "[^$#\\\\]{0,80}") {
        let out = render(&text, &HashMap::new(), &no_includes);
        prop_assert_eq!(out.text, text);
        prop_assert!(out.problems.is_empty());
    }

    #[test]
    fn any_unicode_never_panics(text in any::<String>()) {
        let _ = render(&text, &HashMap::new(), &no_includes);
        let _ = references(&text, &no_includes);
    }
}

fn template(name: &str, extension: &str, text: &str) -> FileTemplate {
    FileTemplate {
        name: name.to_owned(),
        extension: extension.to_owned(),
        kind: TemplateKind::File,
        file_name: String::new(),
        text: text.to_owned(),
        reformat: false,
        live_templates: false,
        children: Vec::new(),
    }
}

struct Fixture {
    registry: Registry,
    session: Session,
    project: PathBuf,
    data: PathBuf,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self::with(|_| {})
    }

    fn with(declare: impl FnOnce(&FileTemplates)) -> Self {
        zen_i18n::register(crate::LOCALES);
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("shop");
        let data = dir.path().join("data");
        std::fs::create_dir_all(project.join("internal")).unwrap();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut session = Session::new();
        let templates = FileTemplates::load(Some(data.clone()), "GoGarden");
        declare(&templates);
        session.set_state(templates);
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        let project = session.state::<Workbench>().unwrap().project().unwrap().to_owned();
        Self { registry, session, project, data, _dir: dir }
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn fail(&mut self, id: &str, params: Value) -> String {
        self.registry.dispatch(&mut self.session, id, params).unwrap_err().to_string()
    }

    fn set(&mut self, template: &FileTemplate, scheme: &str) -> Value {
        self.run("file.templates.set", json!({ "template": template, "scheme": scheme }))
    }

    fn active_text(&self) -> String {
        self.session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned()
    }
}

#[test]
fn a_file_is_created_from_a_template_with_the_predefined_variables_and_opened() {
    let mut fixture = Fixture::new();
    let header = FileTemplate {
        kind: TemplateKind::Include,
        ..template("File Header", "go", "// ${PRODUCT_NAME}: ${PROJECT_NAME}/${DIR_PATH}/${FILE_NAME}\n")
    };
    fixture.set(&header, "default");
    let text = "#parse(\"File Header.go\")\n#set($TYPE = \"${NAME}Service\")\npackage ${NAME}\n\n// ${DS}Id$ by ${USER} on ${DATE} ${TIME}, ${YEAR}-${MONTH}-${DAY} ${HOUR}:${MINUTE}:${SECOND} ${MONTH_NAME_SHORT} ${MONTH_NAME_FULL} ${DAY_NAME_SHORT} ${DAY_NAME_FULL}\ntype ${TYPE} struct{}\n";
    fixture.set(&template("Service", "go", text), "default");
    let created = fixture.run(
        "file.templates.create",
        json!({ "template": "Service", "directory": "internal", "name": "billing/orders.go" }),
    );
    let path = fixture.project.join("internal/billing/orders.go");
    assert_eq!(created["path"], json!(path));
    let written = std::fs::read_to_string(&path).unwrap();
    let now = variables::Moment::now();
    let lines: Vec<&str> = written.lines().collect();
    assert_eq!(lines[0], "// GoGarden: shop/internal/billing/orders.go");
    assert_eq!(lines[1], "package orders");
    assert!(lines[3].starts_with(&format!("// $Id$ by {} on {:04}-", variables::user(), now.year)), "{written}");
    assert!(lines[3].contains(&format!(", {:04}-", now.year)), "{written}");
    assert_eq!(lines[4], "type ordersService struct{}");
    assert_eq!(created["text"], json!(written));
    assert_eq!(fixture.active_text(), written, "the new file is open in front");
    let all = fixture.run("file.templates.variables", json!({ "directory": "internal", "name": "orders.go" }));
    let names: Vec<&str> = all.as_array().unwrap().iter().map(|v| v["name"].as_str().unwrap()).collect();
    for name in [
        "NAME",
        "FILE_NAME",
        "DIR_PATH",
        "PACKAGE_NAME",
        "PROJECT_NAME",
        "PRODUCT_NAME",
        "USER",
        "DATE",
        "TIME",
        "YEAR",
        "MONTH",
        "MONTH_NAME_SHORT",
        "MONTH_NAME_FULL",
        "DAY",
        "DAY_NAME_SHORT",
        "DAY_NAME_FULL",
        "HOUR",
        "MINUTE",
        "SECOND",
        "DS",
    ] {
        assert!(names.contains(&name), "{name} is listed");
    }
    assert_eq!(all[2]["value"], "internal");
    assert!(!all[0]["description"].as_str().unwrap().is_empty());
    let again = fixture.fail(
        "file.templates.create",
        json!({ "template": "Service", "directory": "internal", "name": "billing/orders" }),
    );
    assert!(again.contains("exists") || again.contains("Cannot create"), "{again}");
}

#[test]
fn variables_without_a_value_are_asked_for_also_from_the_file_name_pattern() {
    let mut fixture = Fixture::new();
    let mut handler =
        template("Handler", "go", "package ${PACKAGE}\n// ${DESCRIPTION}\n#if($WITH_TESTS == \"yes\")// tested#end\n");
    handler.file_name = "${NAME}_${SUFFIX}".to_owned();
    fixture.set(&handler, "default");
    let listed = fixture.run("file.templates.list", json!({}));
    let info = &listed["templates"][0];
    assert_eq!(info["asked"], json!(["NAME", "SUFFIX", "PACKAGE", "DESCRIPTION", "WITH_TESTS"]));
    let preview = fixture.run(
        "file.templates.render",
        json!({ "template": "Handler", "name": "user", "variables": { "SUFFIX": "http" } }),
    );
    assert_eq!(preview["missing"], json!(["PACKAGE", "DESCRIPTION", "WITH_TESTS"]));
    assert_eq!(preview["path"], json!(fixture.project.join("user_http.go")));
    let error = fixture.fail(
        "file.templates.create",
        json!({ "template": "Handler", "name": "user", "variables": { "SUFFIX": "http" } }),
    );
    assert!(error.contains("PACKAGE, DESCRIPTION, WITH_TESTS") && error.contains("no value"), "{error}");
    assert!(!fixture.project.join("user_http.go").exists(), "nothing is created while a value is missing");
    let no_name = fixture.fail("file.templates.create", json!({ "template": "Handler" }));
    assert!(no_name.contains("NAME"), "{no_name}");
    fixture.run(
        "file.templates.create",
        json!({ "template": "Handler", "name": "user", "variables": { "SUFFIX": "http", "PACKAGE": "api", "DESCRIPTION": "Users.", "WITH_TESTS": "yes" } }),
    );
    let written = std::fs::read_to_string(fixture.project.join("user_http.go")).unwrap();
    assert_eq!(written, "package api\n// Users.\n// tested\n");
    let mut fixed = template("Fixed", "md", "# ${TITLE}\n");
    fixed.file_name = "docs/README".to_owned();
    fixture.set(&fixed, "default");
    let preview = fixture.run("file.templates.render", json!({ "template": "Fixed" }));
    assert_eq!(preview["asked"], json!(["TITLE"]), "a file name without NAME does not ask for one");
    assert_eq!(preview["path"], json!(fixture.project.join("docs/README.md")));
    let mut escape = template("Escape", "txt", "x");
    escape.file_name = "../../${NAME}".to_owned();
    fixture.set(&escape, "default");
    let outside = fixture.fail("file.templates.create", json!({ "template": "Escape", "name": "away" }));
    assert!(outside.contains("../../away"), "{outside}");
}

#[test]
fn a_file_name_pattern_never_leaves_the_chosen_folder() {
    let mut fixture = Fixture::new();
    let cases = [
        ("../${NAME}", json!({})),
        ("${NAME}/../../x", json!({})),
        ("/tmp/${NAME}", json!({})),
        ("${WHERE}", json!({ "WHERE": "../escaped" })),
        ("${WHERE}", json!({ "WHERE": "/tmp/escaped" })),
        ("sub/${WHERE}", json!({ "WHERE": "../../escaped" })),
        ("$!EMPTY", json!({})),
    ];
    for (pattern, variables) in cases {
        let mut tricky = template("Tricky", "txt", "x");
        tricky.file_name = pattern.to_owned();
        fixture.set(&tricky, "default");
        let error = fixture.fail(
            "file.templates.create",
            json!({ "template": "Tricky", "directory": "internal", "name": "n", "variables": variables }),
        );
        assert!(error.contains("file name pattern"), "{pattern}: {error}");
        let preview = fixture.registry.dispatch(
            &mut fixture.session,
            "file.templates.render",
            json!({ "template": "Tricky", "directory": "internal", "name": "n", "variables": variables }),
        );
        assert!(preview.is_err(), "{pattern}: {preview:?}");
    }
    for typed in ["../n", "/tmp/n", "a/../../n"] {
        let error = fixture.fail("file.templates.create", json!({ "template": "Tricky", "name": typed }));
        assert!(error.contains("not a valid name"), "{typed}: {error}");
    }
    let mut nested = template("Nested", "txt", "x");
    nested.file_name = "docs/${NAME}".to_owned();
    fixture.set(&nested, "default");
    let created =
        fixture.run("file.templates.create", json!({ "template": "Nested", "directory": "internal", "name": "a" }));
    assert_eq!(
        created["path"],
        json!(fixture.project.join("internal/docs/a.txt")),
        "a folder under the chosen one is fine"
    );
    assert!(!fixture.project.parent().unwrap().join("escaped.txt").exists());
    assert!(!fixture.project.join("escaped.txt").exists());
}

#[test]
fn values_with_velocity_characters_are_written_as_they_are() {
    let mut fixture = Fixture::new();
    let mut literal =
        template("Literal", "txt", "[${TEXT}] [$TEXT] #if($TEXT)[set]#end\n#set($COPY = \"<$TEXT>\")$COPY\n");
    literal.file_name = "${NAME}_${TAG}".to_owned();
    fixture.set(&literal, "default");
    let value = "$NAME ${USER} #set($X = 1) #parse(\"Loop\") ## not a comment #[[raw]]# \\$DS";
    fixture.run(
        "file.templates.create",
        json!({ "template": "Literal", "name": "a$NAME", "variables": { "TEXT": value, "TAG": "#if(1)${DS}" } }),
    );
    let path = fixture.project.join("a$NAME_#if(1)${DS}.txt");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), format!("[{value}] [{value}] [set]\n<{value}>\n"));
}

#[test]
fn hostile_templates_are_reported_quickly_and_never_crash() {
    zen_i18n::register(crate::LOCALES);
    let started = std::time::Instant::now();
    let nested = "#if($A)".repeat(100_000);
    let out = plain(&nested, &[("A", "1")]);
    assert!(out.problems.iter().any(|problem| problem.message.contains("too deeply")), "{:?}", out.problems.first());
    let negations = format!("#if({}$A)x#end", "!".repeat(100_000));
    assert!(!plain(&negations, &[("A", "1")]).problems.is_empty());
    let brackets = format!("#if({}$A)x#end", "(".repeat(100_000));
    assert!(!plain(&brackets, &[("A", "1")]).problems.is_empty());
    let chained = format!("#if($A{})x#end", " && $A || !$A".repeat(100_000));
    assert!(!plain(&chained, &[("A", "1")]).problems.is_empty());
    let unclosed = "${a b ".repeat(100_000);
    assert!(!plain(&unclosed, &[]).problems.is_empty());
    let one_line = "#set($x = 1)  ##c\n".to_owned() + &"   #set($x = 1)".repeat(100_000);
    assert!(plain(&one_line, &[]).problems.is_empty());
    let reported = "#foreach($x)".repeat(100_000);
    assert_eq!(plain(&reported, &[]).problems.len(), 100, "problems stop at a hundred");
    let doubling = format!("#set($A = \"xxxxxxxx\")\n{}$A", "#set($A = \"$A$A\")\n".repeat(64));
    let out = plain(&doubling, &[]);
    assert!(out.text.len() <= 1 << 20, "{}", out.text.len());
    assert!(out.problems.iter().any(|problem| problem.message.contains("longer than")), "{:?}", out.problems);
    let includes: HashMap<String, String> = [
        ("Fan".to_owned(), "#parse(\"Fan\")".repeat(32) + "$V"),
        ("Wide".to_owned(), "#parse(\"Leaf\")".repeat(100)),
        ("Leaf".to_owned(), "leaf $V\n".to_owned()),
    ]
    .into();
    let find = |name: &str| includes.get(name).cloned();
    let fanned = render("#parse(\"Fan\")", &values(&[("V", "v")]), &find);
    assert!(fanned.problems.iter().any(|problem| problem.message.contains("too deeply")), "{:?}", fanned.problems);
    assert!(fanned.problems.len() <= 100);
    let wide = render("#parse(\"Wide\")", &values(&[("V", "v")]), &find);
    assert_eq!(wide.text.matches("leaf v").count(), 63, "the template and 63 includes are taken in");
    assert!(wide.problems.iter().any(|problem| problem.message.contains("more than 64")), "{:?}", wide.problems);
    assert_eq!(references("#parse(\"Fan\") $W", &find), ["V", "W"]);
    assert!(started.elapsed() < std::time::Duration::from_secs(120), "took {:?}", started.elapsed());
}

#[test]
fn a_template_that_cannot_be_rendered_is_reported_and_creates_nothing() {
    let mut fixture = Fixture::new();
    fixture.set(&template("Broken", "txt", "ok\n#foreach($i in $items)\n$i\n#end\n${NAME.toUpperCase()}\n"), "default");
    let listed = fixture.run("file.templates.list", json!({ "kind": "file" }));
    let problems = listed["templates"][0]["problems"].as_array().unwrap();
    assert_eq!((problems[0]["line"].as_u64(), problems[0]["column"].as_u64()), (Some(2), Some(1)));
    let error = fixture.fail(
        "file.templates.create",
        json!({ "template": "Broken", "name": "b", "variables": { "i": "1", "items": "x" } }),
    );
    assert!(error.contains("2:1") && error.contains("#foreach") && error.contains("5:1"), "{error}");
    assert!(!fixture.project.join("b.txt").exists());
    let unknown = fixture.fail("file.templates.create", json!({ "template": "Nope", "name": "x" }));
    assert!(unknown.contains("Nope"), "{unknown}");
}

#[test]
fn live_templates_and_reformat_shape_the_new_file() {
    let mut fixture = Fixture::new();
    let mut live = template("Config", "yaml", "name: ${NAME}\nitems:\n\t- #[[$ITEM$]]#\n\t- #[[$END$]]#\n");
    live.live_templates = true;
    live.reformat = true;
    fixture.set(&live, "default");
    let created = fixture.run("file.templates.create", json!({ "template": "Config", "name": "app" }));
    assert_eq!(created["liveVariables"], json!(["ITEM"]));
    let unit = crate::code_style::indent_unit(crate::project_settings::code_style_for_path(
        &fixture.session,
        &fixture.project.join("app.yaml"),
    ));
    assert_ne!(unit, "\t");
    assert_eq!(fixture.active_text(), format!("name: app\nitems:\n{unit}- ITEM\n{unit}- \n"));
    let selected = fixture.session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary();
    let start = fixture.active_text().find("ITEM").unwrap();
    assert_eq!((selected.anchor, selected.head), (start, start + 4), "Tab starts on the first variable");
    fixture.run("editor.edit.type", json!({ "text": "db" }));
    fixture.run("editor.edit.tab", json!({}));
    let text = fixture.active_text();
    assert_eq!(text, format!("name: app\nitems:\n{unit}- db\n{unit}- \n"));
    let caret = fixture.session.state::<Workbench>().unwrap().active_document().unwrap().selections().primary().head;
    assert_eq!(caret, text.len() - 1, "then the caret goes to $END$");

    let mut kept = template("Kept", "yaml", "a:\n\tb: 1\n");
    kept.reformat = false;
    fixture.set(&kept, "default");
    fixture.run("file.templates.create", json!({ "template": "Kept", "name": "k", "open": false }));
    assert_eq!(std::fs::read_to_string(fixture.project.join("k.yaml")).unwrap(), "a:\n\tb: 1\n");
}

#[test]
fn templates_are_kept_per_ide_and_for_the_project_scheme_in_idea_file_templates() {
    let mut fixture = Fixture::new();
    assert_eq!(fixture.run("file.templates.list", json!({}))["scheme"], "default");
    fixture.set(&template("Note", "md", "# IDE ${NAME}\n"), "default");
    assert!(fixture.data.join("fileTemplates/Note.md.ft").is_file());
    assert!(fixture.data.join("options/file.template.settings.xml").is_file());
    fixture.set(&template("Note", "md", "# Project ${NAME}\n"), "project");
    let idea = fixture.project.join(".idea");
    assert_eq!(std::fs::read_to_string(idea.join("fileTemplates/Note.md.ft")).unwrap(), "# Project ${NAME}\n");
    let xml = std::fs::read_to_string(idea.join("file.template.settings.xml")).unwrap();
    assert!(
        xml.contains(r#"<template name="Note.md" file-name="" reformat="false" live-template-enabled="false" />"#),
        "{xml}"
    );
    let listed = fixture.run("file.templates.list", json!({}));
    assert_eq!(listed["scheme"], "project");
    assert_eq!(listed["templates"][0]["source"], "project", "the project's template is over the IDE's");
    fixture.run("file.templates.create", json!({ "template": "Note", "name": "a" }));
    assert_eq!(std::fs::read_to_string(fixture.project.join("a.md")).unwrap(), "# Project a\n");
    fixture.run("file.templates.remove", json!({ "name": "Note", "scheme": "project" }));
    assert!(!idea.join("fileTemplates/Note.md.ft").exists());
    assert_eq!(fixture.run("file.templates.list", json!({}))["templates"][0]["source"], "default");
    let reloaded = FileTemplates::load(Some(fixture.data.clone()), "GoGarden");
    assert_eq!(reloaded.list(None)[0].template.text, "# IDE ${NAME}\n", "the IDE's templates are read back at start");
    let removed = fixture.fail("file.templates.remove", json!({ "name": "Nope", "scheme": "default" }));
    assert!(removed.contains("Nope"), "{removed}");
}

#[test]
fn a_kind_of_a_template_is_offered_by_its_dialog_and_not_listed_in_new() {
    let mut fixture = Fixture::with(|templates| {
        templates.set_plugins(Arc::new(|| {
            vec![Declared::of_plugin(
                "go",
                &[
                    json!({ "name": "Go File", "extension": "go", "text": "package x\n", "markers": ["*.go"] }),
                    json!({ "name": "App", "extension": "go", "text": "package main\n", "kindOf": "Go File", "markers": ["*.go"] }),
                ],
                &[],
            )]
        }));
    });
    let titles = |list: &Value| -> Vec<String> {
        list["items"].as_array().unwrap().iter().map(|item| item["title"].as_str().unwrap().to_owned()).collect()
    };
    let empty = fixture.run("file.new.list", json!({ "directory": "" }));
    assert!(!titles(&empty).iter().any(|title| title == "Go File"), "a marker with a star needs a file of that kind");
    std::fs::write(fixture.project.join("main.go"), "package main\n").unwrap();
    let list = fixture.run("file.new.list", json!({ "directory": "" }));
    assert!(titles(&list).iter().any(|title| title == "Go File"));
    assert!(!titles(&list).iter().any(|title| title == "App"), "a kind is not an item of its own");
    let item = list["items"].as_array().unwrap().iter().find(|item| item["title"] == "Go File").unwrap();
    assert_eq!(item["kinds"], json!(["App"]));
    fixture.run("file.templates.create", json!({ "template": "App", "name": "tool" }));
    assert_eq!(std::fs::read_to_string(fixture.project.join("tool.go")).unwrap(), "package main\n");
}

fn module_values(directory: &Path) -> Vec<(String, String)> {
    let package = directory.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    vec![("GO_PACKAGE_NAME".to_owned(), package)]
}

const MODULE_TEMPLATES: &str = r#"[
    { "name": "Go File", "extension": "go", "text": "package ${GO_PACKAGE_NAME}\n" },
    { "name": "File Header", "extension": "go", "kind": "include", "text": "// header\n" }
]"#;

static MODULE_VARIABLES: &[zen_modules::TemplateVariable] =
    &[zen_modules::TemplateVariable { name: "GO_PACKAGE_NAME", description: "workbench-file-templates-var-name" }];

#[test]
fn modules_and_plugins_declare_built_in_templates_and_variables() {
    let mut fixture = Fixture::with(|templates| {
        templates.declare(Declared::of_module(&zen_modules::FileTemplateSet {
            origin: "go",
            templates: MODULE_TEMPLATES,
            variables: MODULE_VARIABLES,
            values: Some(module_values),
        }));
        templates.set_plugins(Arc::new(|| {
            vec![Declared::of_plugin(
                "html",
                &[json!({ "name": "HTML File", "extension": "html", "text": "<title>${TITLE} ${HTML_LANG}</title>\n" }), json!({ "name": "" })],
                &[json!({ "name": "HTML_LANG", "description": "The language", "value": "en" })],
            )]
        }));
    });
    let listed = fixture.run("file.templates.list", json!({}));
    let names: Vec<&str> =
        listed["templates"].as_array().unwrap().iter().map(|t| t["template"]["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Go File", "File Header", "HTML File"], "an invalid plug-in template is left out");
    assert_eq!(listed["templates"][0]["origin"], "go");
    assert_eq!(listed["templates"][0]["builtin"], true);
    assert_eq!(listed["templates"][0]["asked"], json!(["NAME"]), "a module's variable is not asked for");
    assert_eq!(listed["templates"][2]["asked"], json!(["NAME", "TITLE"]));
    fixture.run("file.templates.create", json!({ "template": "Go File", "directory": "internal", "name": "x" }));
    assert_eq!(std::fs::read_to_string(fixture.project.join("internal/x.go")).unwrap(), "package internal\n");
    fixture.run(
        "file.templates.create",
        json!({ "template": "HTML File", "name": "index", "variables": { "TITLE": "Shop" } }),
    );
    assert_eq!(std::fs::read_to_string(fixture.project.join("index.html")).unwrap(), "<title>Shop en</title>\n");
    let variables = fixture.run("file.templates.variables", json!({ "directory": "internal" }));
    let go = variables.as_array().unwrap().iter().find(|v| v["name"] == "GO_PACKAGE_NAME").unwrap();
    assert_eq!((go["value"].as_str(), go["origin"].as_str()), (Some("internal"), Some("go")));

    let mut changed = template("Go File", "go", "package ${GO_PACKAGE_NAME} // changed\n");
    changed.reformat = true;
    let info = fixture.set(&changed, "default");
    assert_eq!((info["builtin"].as_bool(), info["modified"].as_bool()), (Some(true), Some(true)));
    let builtin = fixture.fail("file.templates.remove", json!({ "name": "Go File" }));
    assert!(builtin.contains("built in"), "{builtin}");
    assert_eq!(fixture.run("file.templates.reset", json!({ "name": "Go File" }))["reset"], 1);
    let back = &fixture.run("file.templates.list", json!({}))["templates"][0];
    assert_eq!((back["modified"].as_bool(), back["source"].as_str()), (Some(false), Some("builtin")));
    let same = FileTemplate { reformat: true, ..template("Go File", "go", "package ${GO_PACKAGE_NAME}\n") };
    fixture.set(&same, "default");
    assert!(!fixture.data.join("fileTemplates/Go File.go.ft").exists(), "a template like the built-in one is not kept");
}

#[test]
fn save_file_as_template_makes_a_template_of_the_current_file() {
    let mut fixture = Fixture::new();
    let file = fixture.project.join("internal/worker.go");
    std::fs::write(&file, "package internal\n").unwrap();
    fixture.run("file.document.open", json!({ "path": file }));
    fixture.run("editor.edit.type", json!({ "text": "// draft\n" }));
    let info = fixture.run("file.templates.saveFileAs", json!({}));
    assert_eq!(info["template"]["name"], "worker");
    assert_eq!(info["template"]["extension"], "go");
    assert_eq!(info["template"]["text"], "// draft\npackage internal\n", "unsaved changes are in it");
    assert_eq!(info["source"], "default");
    assert!(fixture.data.join("fileTemplates/worker.go.ft").is_file());
    let twice = fixture.fail("file.templates.saveFileAs", json!({}));
    assert!(twice.contains("already"), "{twice}");
    std::fs::write(fixture.project.join("Makefile"), "all:\n").unwrap();
    let other =
        fixture.run("file.templates.saveFileAs", json!({ "path": "Makefile", "name": "Build", "scheme": "project" }));
    assert_eq!(
        (other["template"]["name"].as_str(), other["template"]["extension"].as_str()),
        (Some("Build"), Some(""))
    );
    assert!(fixture.project.join(".idea/fileTemplates/Build.ft").is_file());
    fixture.run("file.templates.create", json!({ "template": "worker", "name": "second" }));
    assert_eq!(fixture.active_text(), "// draft\npackage internal\n");
}

#[test]
fn a_template_is_renamed_and_bad_ones_are_refused() {
    let mut fixture = Fixture::new();
    fixture.set(&template("Old", "txt", "x"), "default");
    fixture.run(
        "file.templates.set",
        json!({ "template": template("New", "txt", "x"), "previous": "Old", "scheme": "default" }),
    );
    let listed = fixture.run("file.templates.list", json!({}));
    assert_eq!(listed["templates"].as_array().unwrap().len(), 1);
    assert_eq!(listed["templates"][0]["template"]["name"], "New");
    for bad in
        [template("", "txt", "x"), template("a/b", "txt", "x"), template("ok", ".txt", "x"), template("ok", "t t", "x")]
    {
        let error = fixture.fail("file.templates.set", json!({ "template": bad }));
        assert!(error.contains("cannot be"), "{error}");
    }
}

#[test]
fn the_vue_plugin_declares_a_single_file_component_template() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/vue/plugin.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    let declared = manifest["fileTemplates"]["templates"].as_array().unwrap().clone();
    let mut fixture = Fixture::with(move |templates| {
        let declared = declared.clone();
        templates.set_plugins(Arc::new(move || vec![Declared::of_plugin("vue", &declared, &[])]));
    });
    let listed = fixture.run("file.templates.list", json!({}));
    let vue = listed["templates"].as_array().unwrap().iter().find(|t| t["template"]["extension"] == "vue").cloned();
    let vue = vue.unwrap_or_else(|| panic!("{listed}"));
    assert_eq!(vue["template"]["name"], "Vue Single File Component");
    fixture.run("file.templates.create", json!({ "template": "Vue Single File Component", "name": "HelloWorld" }));
    let created = std::fs::read_to_string(fixture.project.join("HelloWorld.vue")).unwrap();
    assert!(created.starts_with("<script setup lang=\"ts\">\n"), "{created}");
    assert!(
        created.contains("  <div class=\"HelloWorld\"></div>\n</template>") && created.contains("<style scoped>"),
        "{created}"
    );
}

#[test]
fn the_nuxt_plugin_declares_its_page_layout_component_composable_and_route_templates() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/nuxt/plugin.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    let declared = manifest["fileTemplates"]["templates"].as_array().unwrap().clone();
    let mut fixture = Fixture::with(move |templates| {
        let declared = declared.clone();
        templates.set_plugins(Arc::new(move || vec![Declared::of_plugin("nuxt", &declared, &[])]));
    });
    let listed = fixture.run("file.templates.list", json!({}));
    let names: Vec<&str> = listed["templates"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["template"]["name"].as_str())
        .filter(|name| name.starts_with("Nuxt "))
        .collect();
    assert_eq!(names.len(), 5, "{listed}");
    let created = |fixture: &mut Fixture, template: &str, name: &str, file: &str| {
        fixture.run("file.templates.create", json!({ "template": template, "name": name }));
        std::fs::read_to_string(fixture.project.join(file)).unwrap()
    };
    let page = created(&mut fixture, "Nuxt Page", "about", "about.vue");
    assert!(page.starts_with("<script setup lang=\"ts\">") && page.contains("<h1>about</h1>"), "{page}");
    let layout = created(&mut fixture, "Nuxt Layout", "default", "default.vue");
    assert!(layout.contains("<slot />"), "{layout}");
    let component = created(&mut fixture, "Nuxt Component", "AppHeader", "AppHeader.vue");
    assert!(component.contains("defineProps") && component.contains("class=\"AppHeader\""), "{component}");
    let composable = created(&mut fixture, "Nuxt Composable", "useCounter", "useCounter.ts");
    assert_eq!(composable, "export function useCounter() {\n  return {}\n}\n");
    let route = created(&mut fixture, "Nuxt Server Route", "hello", "hello.ts");
    assert!(route.starts_with("export default defineEventHandler(async (event) => {"), "{route}");
}

fn plugin_fixture(id: &'static str) -> Fixture {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../../plugins/{id}/plugin.json"));
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    let declared = manifest["fileTemplates"]["templates"].as_array().unwrap().clone();
    Fixture::with(move |templates| {
        let declared = declared.clone();
        templates.set_plugins(Arc::new(move || vec![Declared::of_plugin(id, &declared, &[])]));
    })
}

fn create(fixture: &mut Fixture, template: &str, params: Value, file: &str) -> String {
    let mut params = params;
    params["template"] = json!(template);
    fixture.run("file.templates.create", params);
    std::fs::read_to_string(fixture.project.join(file)).unwrap()
}

#[test]
fn the_react_plugin_declares_component_hook_and_context_templates() {
    let mut fixture = plugin_fixture("react");
    let component = create(&mut fixture, "React Component", json!({ "name": "Header" }), "Header.tsx");
    assert_eq!(component, "export function Header() {\n  return <div className=\"Header\"></div>\n}\n");
    let jsx = create(&mut fixture, "React Component (JSX)", json!({ "name": "Footer" }), "Footer.jsx");
    assert!(jsx.starts_with("export function Footer()"), "{jsx}");
    let hook = create(&mut fixture, "React Hook", json!({ "name": "useToggle" }), "useToggle.ts");
    assert!(hook.contains("export function useToggle()") && hook.contains("useState"), "{hook}");
    let context = create(&mut fixture, "React Context", json!({ "name": "Theme" }), "Theme.tsx");
    for part in [
        "const ThemeContext = createContext<ThemeValue | null>(null)",
        "export function ThemeProvider(",
        "export function useTheme()",
    ] {
        assert!(context.contains(part), "{part} in {context}");
    }
}

#[test]
fn the_next_plugin_declares_app_router_files_by_their_fixed_names_and_pages_router_templates() {
    let mut fixture = plugin_fixture("next");
    for (template, file) in [
        ("Next.js Page", "page.tsx"),
        ("Next.js Layout", "layout.tsx"),
        ("Next.js Route Handler", "route.ts"),
        ("Next.js Loading UI", "loading.tsx"),
        ("Next.js Error UI", "error.tsx"),
        ("Next.js Not Found", "not-found.tsx"),
    ] {
        let preview = fixture.run("file.templates.render", json!({ "template": template }));
        assert_eq!(preview["asked"], json!([]), "{template} asks for no name: {preview}");
        let text = create(&mut fixture, template, json!({}), file);
        assert!(text.contains("export"), "{template}: {text}");
    }
    let error = std::fs::read_to_string(fixture.project.join("error.tsx")).unwrap();
    assert!(error.starts_with("'use client'\n"), "{error}");
    let route = std::fs::read_to_string(fixture.project.join("route.ts")).unwrap();
    assert!(route.contains("export async function GET("), "{route}");
    let page = create(&mut fixture, "Next.js Pages Router Page", json!({ "name": "about" }), "about.tsx");
    assert!(page.contains("<h1>about</h1>"), "{page}");
    let api = create(&mut fixture, "Next.js API Route", json!({ "name": "hello" }), "hello.ts");
    assert!(api.contains("NextApiRequest") && api.contains("res.status(200)"), "{api}");
}

#[test]
fn the_sveltekit_plugin_declares_route_files_by_their_fixed_names_and_the_server_hooks() {
    let mut fixture = plugin_fixture("sveltekit");
    for (template, file, part) in [
        ("SvelteKit Page", "+page.svelte", "let { data }: PageProps = $props()"),
        ("SvelteKit Layout", "+layout.svelte", "{@render children()}"),
        ("SvelteKit Page Load", "+page.ts", "export const load: PageLoad = async () => {"),
        ("SvelteKit Page Server Load", "+page.server.ts", "import type { PageServerLoad } from './$types'"),
        ("SvelteKit Layout Load", "+layout.ts", "export const load: LayoutLoad"),
        ("SvelteKit Layout Server Load", "+layout.server.ts", "export const load: LayoutServerLoad"),
        ("SvelteKit Server Route", "+server.ts", "export const GET: RequestHandler"),
        ("SvelteKit Error Page", "+error.svelte", "import { page } from '$app/state'"),
        ("SvelteKit Server Hooks", "hooks.server.ts", "export const handle: Handle"),
    ] {
        let preview = fixture.run("file.templates.render", json!({ "template": template }));
        assert_eq!(preview["asked"], json!([]), "{template} asks for nothing: {preview}");
        let text = create(&mut fixture, template, json!({}), file);
        assert!(text.contains(part) && !text.contains("#[["), "{template}: {part} in {text}");
    }
}

#[test]
fn a_template_with_child_templates_creates_every_file_and_keeps_them_on_disk() {
    let mut fixture = Fixture::new();
    let mut service = template("Service", "go", "package ${PACKAGE}\n\ntype ${NAME} struct{}\n");
    service.children = vec![
        ChildTemplate {
            extension: "go".to_owned(),
            file_name: "${NAME}_test".to_owned(),
            text: "package ${PACKAGE}\n\n// ${FILE_NAME} tests ${NAME}\n".to_owned(),
            reformat: false,
            live_templates: false,
        },
        ChildTemplate {
            extension: "md".to_owned(),
            text: "# ${NAME} by ${OWNER}\n".to_owned(),
            ..ChildTemplate::default()
        },
    ];
    let info = fixture.set(&service, "project");
    assert_eq!(info["asked"], json!(["NAME", "PACKAGE", "OWNER"]), "the children's variables are asked for too");
    let folder = fixture.project.join(".idea/fileTemplates");
    assert!(folder.join("Service.go.ft").is_file());
    assert_eq!(
        std::fs::read_to_string(folder.join("Service.go.child.0.go")).unwrap(),
        "package ${PACKAGE}\n\n// ${FILE_NAME} tests ${NAME}\n"
    );
    assert!(folder.join("Service.go.child.1.md").is_file());
    let xml = std::fs::read_to_string(fixture.project.join(".idea/file.template.settings.xml")).unwrap();
    assert!(xml.contains(r#"<template name="Service.go.child.0.go" file-name="${NAME}_test""#), "{xml}");
    let listed = fixture.run("file.templates.list", json!({ "kind": "file" }));
    let names: Vec<&str> =
        listed["templates"].as_array().unwrap().iter().map(|t| t["template"]["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Service"], "children are not templates of their own");
    assert_eq!(listed["templates"][0]["template"]["children"][1]["extension"], "md");

    let missing = fixture.fail(
        "file.templates.create",
        json!({ "template": "Service", "directory": "internal", "name": "Orders", "variables": { "PACKAGE": "internal" } }),
    );
    assert!(missing.contains("OWNER"), "{missing}");
    assert!(!fixture.project.join("internal/Orders.go").exists(), "nothing is created while a value is missing");
    std::fs::write(fixture.project.join("internal/Orders.md"), "taken").unwrap();
    let taken = fixture.fail(
        "file.templates.create",
        json!({ "template": "Service", "directory": "internal", "name": "Orders", "variables": { "PACKAGE": "internal", "OWNER": "ann" } }),
    );
    assert!(taken.contains("Orders.md"), "{taken}");
    assert!(!fixture.project.join("internal/Orders.go").exists(), "nothing is created when a child's file exists");
    std::fs::remove_file(fixture.project.join("internal/Orders.md")).unwrap();
    let created = fixture.run(
        "file.templates.create",
        json!({ "template": "Service", "directory": "internal", "name": "Orders", "variables": { "PACKAGE": "internal", "OWNER": "ann" } }),
    );
    let internal = fixture.project.join("internal");
    assert_eq!(created["children"], json!([internal.join("Orders_test.go"), internal.join("Orders.md")]));
    assert_eq!(
        std::fs::read_to_string(internal.join("Orders_test.go")).unwrap(),
        "package internal\n\n// Orders_test.go tests Orders\n"
    );
    assert_eq!(std::fs::read_to_string(internal.join("Orders.md")).unwrap(), "# Orders by ann\n");
    let workbench = fixture.session.state::<Workbench>().unwrap();
    assert_eq!(workbench.active_document().unwrap().path(), internal.join("Orders.go"), "the main file is in front");
    assert_eq!(workbench.documents().len(), 3, "the children open behind it");

    service.children.remove(0);
    fixture.set(&service, "project");
    assert!(!folder.join("Service.go.child.0.go").exists(), "a removed child's file goes");
    assert!(folder.join("Service.go.child.0.md").is_file(), "the others are numbered again");
    fixture.run("file.templates.remove", json!({ "name": "Service", "scheme": "project" }));
    assert!(std::fs::read_dir(&folder).unwrap().next().is_none(), "removing a template removes its children");
    let nested = FileTemplate { kind: TemplateKind::Include, ..service.clone() };
    let refused = fixture.fail("file.templates.set", json!({ "template": nested, "scheme": "project" }));
    assert!(refused.contains("Files"), "{refused}");
}

#[test]
fn the_new_list_offers_file_directory_and_the_templates_that_fit() {
    let mut fixture = Fixture::with(|templates| {
        templates.set_plugins(Arc::new(|| {
            vec![Declared::of_plugin(
                "go",
                &[
                    json!({ "name": "Go File", "extension": "go", "text": "package x\n", "markers": ["go.mod"] }),
                    json!({ "name": "Go Header", "kind": "include", "extension": "go", "text": "// h\n" }),
                    json!({ "name": "Notes", "extension": "md", "text": "# ${NAME}\n" }),
                ],
                &[],
            )]
        }));
    });
    fixture.set(&template("Mine", "txt", "${NAME} ${WHO}\n"), "default");
    let titles = |list: &Value| -> Vec<String> {
        list["items"].as_array().unwrap().iter().map(|item| item["title"].as_str().unwrap().to_owned()).collect()
    };
    let list = fixture.run("file.new.list", json!({ "directory": "internal" }));
    assert_eq!(titles(&list), ["File", "Directory", "Notes", "Mine"]);
    assert_eq!(list["directory"], json!(fixture.project.join("internal")));
    assert_eq!(
        list["items"][3],
        json!({ "kind": "template", "title": "Mine", "template": "Mine", "extension": "txt", "asked": ["NAME", "WHO"] })
    );
    std::fs::write(fixture.project.join("go.mod"), "module shop\n").unwrap();
    let list = fixture.run("file.new.list", json!({ "directory": "internal" }));
    assert_eq!(titles(&list), ["File", "Directory", "Go File", "Notes", "Mine"], "go.mod above the folder");
    let info = fixture.run("file.templates.list", json!({ "kind": "file" }));
    assert_eq!(info["templates"][0]["markers"], json!(["go.mod"]));
    let outside = fixture.fail("file.new.list", json!({ "directory": "/" }));
    assert!(!outside.is_empty());
}

fn java_fixture() -> Fixture {
    Fixture::with(|templates| {
        templates.set_plugins(Arc::new(|| {
            vec![Declared::of_plugin(
                "java",
                &[
                    json!({ "name": "Java Class", "extension": "java", "text": "class ${NAME} {}\n",
                            "sourceFolder": true, "group": "Java Class" }),
                    json!({ "name": "Java Enum", "extension": "java", "text": "enum ${NAME} {}\n",
                            "sourceFolder": true, "group": "Java Class" }),
                    json!({ "name": "package-info.java", "extension": "java", "fileName": "package-info",
                            "text": "package x;\n", "sourceFolder": true }),
                    json!({ "name": "Notes", "extension": "md", "text": "# ${NAME}\n" }),
                ],
                &[],
            )]
        }));
    })
}

fn listed_titles(list: &Value) -> Vec<&str> {
    list["items"].as_array().unwrap().iter().map(|item| item["title"].as_str().unwrap()).collect()
}

#[test]
fn a_source_folder_lists_one_row_per_group_then_file_package_and_the_rest() {
    let mut fixture = java_fixture();
    std::fs::create_dir_all(fixture.project.join("src/main/java/com")).unwrap();
    let list = fixture.run("file.new.list", json!({ "directory": "src/main/java/com" }));
    assert_eq!(listed_titles(&list), ["Java Class", "File", "Package", "package-info.java"]);
    assert_eq!(list["items"][0]["group"], "Java Class");
    assert_eq!(list["items"][2]["kind"], "package");
}

#[test]
fn a_plain_folder_offers_no_template_meant_for_source_folders() {
    let mut fixture = java_fixture();
    let list = fixture.run("file.new.list", json!({ "directory": "internal" }));
    assert_eq!(listed_titles(&list), ["File", "Directory", "Notes"]);
}

#[test]
fn the_menu_list_never_offers_a_class_even_for_a_source_folder() {
    let mut fixture = java_fixture();
    std::fs::create_dir_all(fixture.project.join("src/main/java")).unwrap();
    std::fs::write(fixture.project.join("pom.xml"), "<project/>").unwrap();
    let list = fixture.run("file.new.list", json!({ "directory": "src/main/java", "menu": true }));
    assert_eq!(listed_titles(&list), ["File", "Directory", "Notes"]);
}

#[test]
fn src_is_a_source_folder_of_a_project_without_a_build_layout_once_it_holds_java_or_kotlin() {
    let mut fixture = java_fixture();
    let src = fixture.project.join("src");
    std::fs::create_dir_all(src.join("web")).unwrap();
    std::fs::write(src.join("web/index.js"), "").unwrap();
    assert_eq!(listed_titles(&fixture.run("file.new.list", json!({ "directory": "src" })))[0], "File");
    std::fs::write(src.join("Main.java"), "").unwrap();
    assert_eq!(listed_titles(&fixture.run("file.new.list", json!({ "directory": "src" })))[0], "Java Class");
}

#[test]
fn the_scheme_is_switched_and_listed_as_each_scheme_has_it() {
    let mut fixture = Fixture::new();
    fixture.set(&template("Mine", "txt", "ide\n"), "default");
    assert_eq!(fixture.run("file.templates.list", json!({}))["scheme"], "default");
    let used = fixture.run("file.templates.useScheme", json!({ "scheme": "project" }));
    assert_eq!(used, json!({ "scheme": "project" }));
    assert!(!fixture.project.join(".idea/fileTemplates").exists(), "nothing is written until a template is set");
    assert_eq!(fixture.run("file.templates.list", json!({}))["scheme"], "project");
    fixture.set(&template("Mine", "txt", "project\n"), "project");
    fixture.set(&template("Ours", "txt", "ours\n"), "project");
    let project = fixture.run("file.templates.list", json!({}));
    assert_eq!(project["templates"][0]["template"]["text"], "project\n");
    assert_eq!(project["templates"][0]["source"], "project");
    let default = fixture.run("file.templates.list", json!({ "scheme": "default" }));
    let names: Vec<&str> =
        default["templates"].as_array().unwrap().iter().map(|t| t["template"]["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Mine"], "the Default scheme leaves the project's templates out");
    assert_eq!(default["templates"][0]["template"]["text"], "ide\n");
    let idea = fixture.project.join(".idea");
    let used = fixture.run("file.templates.useScheme", json!({ "scheme": "default" }));
    assert_eq!(used, json!({ "scheme": "default" }));
    assert_eq!(
        std::fs::read_to_string(idea.join("fileTemplates/Ours.txt.ft")).unwrap(),
        "ours\n",
        "nothing is deleted"
    );
    assert!(idea.join("fileTemplates/Mine.txt.ft").is_file());
    assert_eq!(fixture.run("file.templates.list", json!({}))["scheme"], "default", "though .idea/fileTemplates stays");
    assert_eq!(fixture.run("file.templates.list", json!({}))["templates"][0]["template"]["text"], "ide\n");
    let reloaded = FileTemplates::load(Some(fixture.data.clone()), "");
    assert_eq!(reloaded.scheme_of(Some(&fixture.project)), Scheme::Default, "the choice is kept in the data folder");
    fixture.run("file.templates.useScheme", json!({ "scheme": "project" }));
    assert_eq!(fixture.run("file.templates.list", json!({}))["templates"][0]["template"]["text"], "project\n");
}

#[test]
fn a_child_that_cannot_be_written_leaves_no_file_behind() {
    let mut fixture = Fixture::new();
    let mut pair = template("Pair", "go", "package x\n");
    pair.children = vec![
        ChildTemplate { extension: "go".to_owned(), file_name: "${NAME}_a".to_owned(), ..ChildTemplate::default() },
        ChildTemplate {
            extension: "go".to_owned(),
            file_name: "blocker/${NAME}".to_owned(),
            ..ChildTemplate::default()
        },
    ];
    fixture.set(&pair, "default");
    let internal = fixture.project.join("internal");
    std::fs::write(internal.join("blocker"), "a file, not a folder").unwrap();
    let error =
        fixture.fail("file.templates.create", json!({ "template": "Pair", "directory": "internal", "name": "One" }));
    assert!(error.contains("blocker"), "{error}");
    assert!(!internal.join("One_a.go").exists(), "the child written first is taken back");
    assert!(!internal.join("One.go").exists());
}

#[test]
fn the_caret_ends_at_end_after_multibyte_text_and_crlf() {
    let mut fixture = Fixture::new();
    let mut live = template("Live", "txt", "Привет, 日本 😀\r\nтекст #[[$END$]]# хвост\r\n");
    live.live_templates = true;
    live.reformat = false;
    fixture.set(&live, "default");
    fixture.run("file.templates.create", json!({ "template": "Live", "directory": "internal", "name": "note" }));
    let text = std::fs::read_to_string(fixture.project.join("internal/note.txt")).unwrap();
    assert_eq!(text, "Привет, 日本 😀\r\nтекст  хвост\r\n");
    let end = text[..text.find("  ").unwrap() + 1].chars().count();
    let workbench = fixture.session.state::<Workbench>().unwrap();
    let caret = workbench.active_document().unwrap().selections().primary();
    assert_eq!((caret.anchor, caret.head), (end, end));
}

#[test]
fn the_kotlin_plugins_templates_name_their_package_when_there_is_one() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../plugins/kotlin/plugin.json");
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    let class = manifest["fileTemplates"]["templates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|template| template["name"] == "Kotlin Class")
        .unwrap()["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        plain(&class, &[("NAME", "Shop"), ("PACKAGE_NAME", "demo.shop")]).text,
        "package demo.shop\n\nclass Shop {\n}\n"
    );
    assert_eq!(plain(&class, &[("NAME", "Shop"), ("PACKAGE_NAME", "")]).text, "class Shop {\n}\n");
}
