use std::path::Path;
use std::time::Duration;

use serde_json::json;
use zen_project::support::ProjectSupport;

use super::*;
use crate::plugin_projects::PluginProject;

fn session_with(root: &Path, formatter_script: &str) -> (Registry, Session) {
    let bin = root.join("node_modules/.bin");
    std::fs::create_dir_all(&bin).unwrap();
    let program = bin.join("fmt");
    std::fs::write(&program, formatter_script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    zen_i18n::register(crate::LOCALES);
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": root })).unwrap();
    crate::trust::trust_session_in_tests(&mut session, root);
    let support = ProjectSupport::parse(
        r#"{ "markers": { "files": [".fmtrc"] }, "formatters": [{ "id": "fmt", "name": "Fmt", "command": "fmt",
            "args": ["--file", "${file}"], "extensions": ["txt"], "onReformat": "codeQuality.fmt.onReformat",
            "onSave": "codeQuality.fmt.onSave" }] }"#,
    )
    .unwrap();
    session.state_mut::<PluginProjects>().set(vec![PluginProject { id: "fmt".into(), name: "Fmt".into(), support }]);
    (registry, session)
}

#[cfg(unix)]
#[test]
fn reformat_code_and_saving_run_the_projects_formatter_as_its_settings_say() {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    std::fs::write(root.join(".fmtrc"), "").unwrap();
    std::fs::write(root.join("notes.txt"), "hello\n").unwrap();
    let (registry, mut session) = session_with(root, "#!/bin/sh\n[ \"$2\" = notes.txt ] || exit 3\ntr a-z A-Z\n");
    let path = root.join("notes.txt");
    registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
    assert!(formatter_for(&session, &path, false).is_some(), "on Reformat Code by default");
    assert!(formatter_for(&session, &path, true).is_none(), "not on save by default");
    assert!(formatter_for(&session, &root.join("main.go"), false).is_none(), "not its files");

    let answer = registry.dispatch(&mut session, "editor.code.reformat", json!({ "waitMs": 5000 })).unwrap();
    assert_eq!(answer["changed"], true, "{answer}");
    let text = |session: &Session| session.state::<Workbench>().unwrap().active_document().unwrap().text().to_owned();
    assert_eq!(text(&session), "HELLO\n");

    session.settings_mut().set("codeQuality.fmt.onReformat", false).unwrap();
    assert!(formatter_for(&session, &path, false).is_none(), "turned off for Reformat Code");
    session.settings_mut().set("codeQuality.fmt.onSave", true).unwrap();
    registry.dispatch(&mut session, "editor.edit.type", json!({ "text": "again " })).unwrap();
    registry.dispatch(&mut session, "file.document.save", json!({})).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "AGAIN HELLO\n");

    let tools = registry.dispatch(&mut session, "codeQuality.tools.list", json!({})).unwrap();
    assert_eq!(tools["formatters"][0]["name"], "Fmt");
    assert_eq!(tools["formatters"][0]["onSave"], json!(["codeQuality.fmt.onSave", true]));
    let form = registry.dispatch(&mut session, "codeQuality.settings.form", json!({})).unwrap();
    assert!(form.to_string().contains("codeQuality.fmt.onReformat"), "{form}");
}

#[cfg(unix)]
#[test]
fn a_failing_formatter_says_why_and_leaves_the_text() {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    std::fs::write(root.join(".fmtrc"), "").unwrap();
    std::fs::write(root.join("notes.txt"), "hello\n").unwrap();
    let (registry, mut session) = session_with(root, "#!/bin/sh\necho 'SyntaxError: bad input' >&2\nexit 2\n");
    let path = root.join("notes.txt");
    registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
    let answer = registry.dispatch(&mut session, "editor.code.reformat", json!({ "waitMs": 5000 })).unwrap();
    assert!(answer["error"].as_str().unwrap().contains("SyntaxError: bad input"), "{answer}");
    let run = FormatterRun::start(&formatter_for(&session, &path, false).unwrap(), &path, "x".into(), 1);
    assert!(run.wait(Duration::from_secs(5)).unwrap().is_err());
}

#[test]
fn the_form_lists_each_linter_with_its_configuration_and_fixes_on_save() {
    zen_i18n::register(crate::LOCALES);
    let tools = ToolList {
        linters: vec![LinterInfo {
            id: "eslint".into(),
            name: "ESLint".into(),
            enabled: true,
            source: ServerSource::Local,
            formats: false,
            fix_on_save: Some(("codeQuality.eslint.fixOnSave".into(), false)),
        }],
        formatters: Vec::new(),
    };
    let form = serde_json::to_value(form(&tools)).unwrap();
    let text = form.to_string();
    assert!(text.contains("codeQuality.linter.set") && text.contains("codeQuality.eslint.fixOnSave"), "{text}");
    assert_eq!(form["items"][1]["selected"], "on");
    let empty = serde_json::to_value(super::form(&ToolList::default())).unwrap();
    assert_eq!(empty["items"].as_array().unwrap().len(), 1, "a note what to install");
}

#[cfg(unix)]
#[test]
fn a_fixer_runs_on_save_with_the_project_program_and_stays_out_of_reformat_code() {
    let folder = tempfile::tempdir().unwrap();
    let root = folder.path();
    std::fs::write(root.join("a.css"), "A\n").unwrap();
    std::fs::write(root.join(".stylelintrc.json"), "{}").unwrap();
    let (registry, mut session) = session_with(root, "#!/bin/sh\ntr A-Z a-z\n");
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../plugins/stylelint/plugin.json")).unwrap();
    let support = ProjectSupport::parse(&manifest["project"].to_string()).unwrap();
    let bin = root.join("node_modules/.bin");
    std::fs::rename(bin.join("fmt"), bin.join("stylelint")).unwrap();
    session.state_mut::<PluginProjects>().set(vec![PluginProject {
        id: "stylelint".into(),
        name: "Stylelint".into(),
        support,
    }]);
    let path = root.join("a.css");
    registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
    assert!(formatter_for(&session, &path, false).is_none(), "Reformat Code stays with the language");
    assert!(formatter_for(&session, &path, true).is_none(), "off until chosen");
    session.settings_mut().set("codeQuality.stylelint.onSave", true).unwrap();
    assert!(formatter_for(&session, &path, true).is_some_and(|formatter| formatter.declared.fixer));
    registry.dispatch(&mut session, "editor.edit.insert", json!({ "text": "B" })).unwrap();
    registry.dispatch(&mut session, "file.document.save", json!({})).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "ba\n");
}
