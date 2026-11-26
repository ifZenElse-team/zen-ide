use std::time::Instant;

use serde_json::{Value, json};
use zen_core::keymap::Keystroke;

use super::*;
use crate::keymaps::KeymapStore;

fn english() {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
}

fn setup() -> (Registry, Session, tempfile::TempDir, PathBuf) {
    english();
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let project = std::fs::canonicalize(dir.path()).unwrap().join("my project");
    std::fs::create_dir_all(&project).unwrap();
    let mut session = Session::new();
    session.set_state(ExternalTools::load(Some(dir.path().join("data").join(FILE_NAME))));
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    (registry, session, dir, project)
}

fn tool(name: &str, program: &str, arguments: &str) -> ExternalTool {
    ExternalTool {
        name: name.to_owned(),
        program: program.to_owned(),
        arguments: arguments.to_owned(),
        ..ExternalTool::default()
    }
}

fn save(registry: &Registry, session: &mut Session, tool: &ExternalTool) -> String {
    let saved = registry.dispatch(session, "run.externalTools.save", json!({ "tool": tool })).unwrap();
    saved["id"].as_str().unwrap().to_owned()
}

fn run(registry: &Registry, session: &mut Session, params: Value) -> Result<Value, String> {
    registry.dispatch(session, "run.externalTools.run", params).map_err(|error| match error {
        zen_core::DispatchError::Failed { error, .. } => error.to_string(),
        other => other.to_string(),
    })
}

fn console(registry: &Registry, session: &mut Session, run_id: u64) -> Vec<Value> {
    let output = registry.dispatch(session, "run.output.get", json!({ "runId": run_id })).unwrap();
    output["lines"].as_array().unwrap().clone()
}

fn texts(lines: &[Value], stream: &str) -> Vec<String> {
    lines.iter().filter(|line| line["stream"] == stream).map(|line| line["text"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn tools_are_kept_at_the_ide_level_and_edited_by_commands() {
    let (registry, mut session, dir, _) = setup();
    let file = dir.path().join("data").join(FILE_NAME);
    let listed = registry.dispatch(&mut session, "run.externalTools.list", json!({})).unwrap();
    assert_eq!(listed["tools"], json!([]));
    let first = save(&registry, &mut session, &tool("Format File", "/usr/bin/true", ""));
    let second = save(&registry, &mut session, &tool("Format File", "/usr/bin/true", ""));
    let other = ExternalTool { group: "Linters".to_owned(), ..tool("Lint", "/usr/bin/true", "") };
    let third = save(&registry, &mut session, &other);
    assert_eq!((first.as_str(), second.as_str(), third.as_str()), ("format-file", "format-file-2", "lint"));
    let saved: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    assert_eq!(saved["formatVersion"], 1);
    assert_eq!(saved["tools"].as_array().unwrap().len(), 3);

    let moved = registry.dispatch(&mut session, "run.externalTools.move", json!({ "id": second, "up": true })).unwrap();
    assert_eq!(moved["moved"], true);
    let again = registry.dispatch(&mut session, "run.externalTools.move", json!({ "id": second, "up": true })).unwrap();
    assert_eq!(again["moved"], false, "first in its group");
    let alone = registry.dispatch(&mut session, "run.externalTools.move", json!({ "id": third, "up": true })).unwrap();
    assert_eq!(alone["moved"], false, "Lint is alone in its group");
    let copied = registry.dispatch(&mut session, "run.externalTools.copy", json!({ "id": third })).unwrap();
    assert_eq!(copied["id"], "lint-copy");
    registry.dispatch(&mut session, "run.externalTools.remove", json!({ "id": first })).unwrap();
    let ids = |session: &mut Session| -> Vec<String> {
        let listed = registry.dispatch(session, "run.externalTools.list", json!({})).unwrap();
        listed["tools"].as_array().unwrap().iter().map(|tool| tool["id"].as_str().unwrap().to_owned()).collect()
    };
    assert_eq!(ids(&mut session), ["format-file-2", "lint", "lint-copy"]);
    let names = ExternalTools::load(Some(file.clone())).tools().into_iter().map(|tool| tool.name).collect::<Vec<_>>();
    assert_eq!(names, ["Format File", "Lint", "Lint (copy)"], "read back from the file");

    let mut disabled = ExternalTools::load(Some(file.clone())).get("lint").unwrap();
    disabled.enabled = false;
    let replaced = registry
        .dispatch(
            &mut session,
            "run.externalTools.replace",
            json!({ "tools": [disabled, tool("New", "/bin/echo", "")] }),
        )
        .unwrap();
    assert_eq!(replaced["tools"][0]["enabled"], false);
    assert_eq!(replaced["tools"][1]["id"], "new");

    let refused =
        registry.dispatch(&mut session, "run.externalTools.save", json!({ "tool": tool("", "/bin/echo", "") }));
    assert!(refused.unwrap_err().to_string().contains("needs a name"));
    let no_program = registry.dispatch(&mut session, "run.externalTools.save", json!({ "tool": tool("X", " ", "") }));
    assert!(no_program.is_err());
    let filter = ExternalTool { output_filters: vec!["$LINE$ only".to_owned()], ..tool("F", "/bin/echo", "") };
    let bad = registry.dispatch(&mut session, "run.externalTools.save", json!({ "tool": filter }));
    assert!(bad.unwrap_err().to_string().contains("$FILE_PATH$"));
}

#[test]
fn a_damaged_file_is_kept_aside_and_the_list_starts_empty() {
    english();
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(FILE_NAME);
    std::fs::write(&file, "{ not json").unwrap();
    let tools = ExternalTools::load(Some(file.clone()));
    assert!(tools.tools().is_empty());
    assert!(tools.take_notice().is_some_and(|notice| notice.contains("externalTools.json.corrupt")));
    assert!(tools.take_notice().is_none(), "once");
    assert!(dir.path().join("externalTools.json.corrupt").exists());
}

#[test]
fn every_tool_is_an_action_of_every_keymap() {
    let dir = tempfile::tempdir().unwrap();
    let keymaps: SharedKeymaps = Arc::new(Mutex::new(KeymapStore::new(Some(dir.path().to_owned()), Vec::new())));
    let tools = ExternalTools::load(None);
    tools.connect_keymaps(Arc::clone(&keymaps));
    let id = tools.save_tool(tool("Lint", "/usr/bin/true", "")).unwrap();
    let action = format!("run.externalTools.run/{id}");
    let binding = keymaps::lock(&keymaps).active().bindings().into_iter().find(|binding| binding.action == action);
    let binding = binding.expect("the tool is a keymap action");
    assert_eq!((binding.command.as_str(), binding.params), (RUN_ACTION, json!({ "toolId": "lint" })));
    let shortcut = Keystroke::parse("⌃⌥L").unwrap();
    keymaps::lock(&keymaps).set_changes([(action.clone(), vec![shortcut.clone()])].into()).unwrap();
    let found = keymaps::lock(&keymaps).active().lookup(&shortcut, Context::Global).map(|binding| binding.action);
    assert_eq!(found.as_deref(), Some(action.as_str()));
    tools.remove(&id).unwrap();
    assert!(keymaps::lock(&keymaps).active().bindings().iter().all(|binding| binding.action != action));
}

#[test]
fn macros_take_the_editors_file_caret_selection_and_the_clipboard() {
    let (registry, mut session, _dir, project) = setup();
    let file = project.join("src dir").join("main file.go");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "package main\n\nfunc main() {}\n").unwrap();
    std::fs::write(project.join("go.mod"), "module example.com/app\n").unwrap();
    let macros = registry.dispatch(&mut session, "run.externalTools.macros", json!({})).unwrap();
    let value = |macros: &Value, token: &str| macros.as_array().unwrap().iter().find(|m| m["token"] == token).cloned();
    assert_eq!(macros.as_array().unwrap().len(), 18);
    assert_eq!(value(&macros, "$FilePath$").unwrap()["missing"], "no file is open in the editor");
    assert_eq!(value(&macros, "$ProjectName$").unwrap()["value"], "my project");
    registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
    let head = json!({ "line": 2, "column": 9 });
    let anchor = json!({ "line": 2, "column": 5 });
    registry
        .dispatch(&mut session, "editor.selection.set", json!({ "selections": [{ "head": head, "anchor": anchor }] }))
        .unwrap();
    registry.dispatch(&mut session, "project.file.copyPath", json!({ "paths": [project] })).unwrap();
    let macros = registry.dispatch(&mut session, "run.externalTools.macros", json!({})).unwrap();
    for (token, expected) in [
        ("$FilePath$", file.display().to_string()),
        ("$FileDir$", project.join("src dir").display().to_string()),
        ("$FileName$", "main file.go".to_owned()),
        ("$FileNameWithoutExtension$", "main file".to_owned()),
        ("$ProjectFileDir$", project.display().to_string()),
        ("$SelectedText$", "main".to_owned()),
        ("$LineNumber$", "3".to_owned()),
        ("$ColumnNumber$", "10".to_owned()),
        ("$ClipboardContent$", project.display().to_string()),
        ("$GoFileImportPath$", "example.com/app/src dir".to_owned()),
        ("$OSName$", std::env::consts::OS.to_owned()),
        ("$TempDir$", std::env::temp_dir().display().to_string()),
    ] {
        assert_eq!(value(&macros, token).unwrap()["value"], expected, "{token}");
    }
    assert_eq!(value(&macros, "$Prompt$").unwrap()["missing"], "it is asked for when the tool starts");
    let elsewhere = project.join("other.txt");
    let from_tree = registry.dispatch(&mut session, "run.externalTools.macros", json!({ "file": elsewhere })).unwrap();
    assert_eq!(value(&from_tree, "$FileName$").unwrap()["value"], "other.txt");
    assert!(value(&from_tree, "$LineNumber$").unwrap().get("value").is_none(), "no caret in a file from the tree");
    assert_eq!(value(&from_tree, "$GoFileImportPath$").unwrap()["value"], "example.com/app");

    let params = json!({ "program": "$ProjectFileDir$/bin/x", "arguments": "-f \"$FilePath$\" $Prompt$", "workingDirectory": "$FileDir$" });
    let preview = registry.dispatch(&mut session, "run.externalTools.preview", params).unwrap();
    assert_eq!(preview["program"], format!("{}/bin/x", project.display()));
    assert_eq!(preview["arguments"], json!(["-f", file.display().to_string(), "<Prompt>"]));
    assert_eq!(preview["workingDirectory"], project.join("src dir").display().to_string());
    assert_eq!(preview["missing"], json!(["$Prompt$"]));
}

fn wait_done(registry: &Registry, session: &mut Session, run_id: u64) {
    let start = Instant::now();
    loop {
        let output = registry.dispatch(session, "run.output.get", json!({ "runId": run_id, "waitMs": 200 })).unwrap();
        if output["state"] != "running" {
            return;
        }
        assert!(start.elapsed() < Duration::from_secs(20), "the run did not end");
    }
}

#[cfg(unix)]
#[test]
fn a_run_passes_each_macro_value_as_one_argument_without_a_shell() {
    let (registry, mut session, _dir, project) = setup();
    let file = project.join("it's a file.txt");
    std::fs::write(&file, "x\n").unwrap();
    registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
    let script = project.join("args.sh");
    std::fs::write(&script, "for a in \"$@\"; do printf '[%s]\\n' \"$a\"; done\n").unwrap();
    let echo = ExternalTool {
        working_directory: "$FileDir$".to_owned(),
        ..tool(
            "Show Args",
            "/bin/sh",
            &format!("'{}' $FilePath$ \"$Prompt$\" '$FileName$;rm -rf x' $FilePrompt$", script.display()),
        )
    };
    let id = save(&registry, &mut session, &echo);
    let missing = run(&registry, &mut session, json!({ "id": id })).unwrap_err();
    assert!(missing.contains("$Prompt$") && missing.contains("asked for"), "{missing}");
    let params = json!({ "id": id, "prompt": "two \"quoted\" words", "filePrompt": "/tmp/a b.txt", "waitMs": 20000 });
    let started = run(&registry, &mut session, params).unwrap();
    assert_eq!(started["summary"]["exitCode"], 0, "{started}");
    let lines = console(&registry, &mut session, started["runId"].as_u64().unwrap());
    assert_eq!(
        texts(&lines, "stdout"),
        [
            format!("[{}]", file.display()),
            "[two \"quoted\" words]".to_owned(),
            "[it's a file.txt;rm -rf x]".to_owned(),
            "[/tmp/a b.txt]".to_owned(),
        ]
    );
    let system = texts(&lines, "system");
    assert!(
        system.first().is_some_and(|line| line.starts_with("/bin/sh ")),
        "the command line comes first: {system:?}"
    );
    assert_eq!(system.last().map(String::as_str), Some("Process finished with exit code 0"));
    assert!(project.join("it's a file.txt").exists());
    let listed = registry.dispatch(&mut session, "run.process.list", json!({})).unwrap();
    let info = &listed["runs"][0];
    assert_eq!((info["name"].as_str(), info["kind"].as_str()), (Some("Show Args"), Some(KIND)));
    assert_eq!(info["externalTool"]["toolId"], "show-args");
    assert_eq!(info["externalTool"]["openConsole"], true);
}

#[cfg(unix)]
#[test]
fn a_macro_without_a_value_fails_the_run_with_a_message() {
    let (registry, mut session, _dir, _project) = setup();
    let id = save(&registry, &mut session, &tool("Cat", "/bin/echo", "$FilePath$"));
    let error = run(&registry, &mut session, json!({ "id": id })).unwrap_err();
    assert_eq!(error, "Cannot run “Cat”: $FilePath$ has no value (no file is open in the editor).");
    let listed = registry.dispatch(&mut session, "run.process.list", json!({})).unwrap();
    assert_eq!(listed["runs"], json!([]), "nothing started");
    let off = ExternalTool { enabled: false, ..tool("Off", "/usr/bin/true", "") };
    let off = save(&registry, &mut session, &off);
    assert!(run(&registry, &mut session, json!({ "id": off })).unwrap_err().contains("switched off"));
    assert!(run(&registry, &mut session, json!({ "id": "nope" })).unwrap_err().contains("no external tool"));
}

#[cfg(unix)]
#[test]
fn output_filters_link_files_and_synchronize_reads_files_again() {
    let (registry, mut session, _dir, project) = setup();
    let file = project.join("notes.txt");
    std::fs::write(&file, "old\n").unwrap();
    registry.dispatch(&mut session, "file.document.open", json!({ "path": file })).unwrap();
    let script = format!("printf 'new\\n' > '{0}'; printf 'warn: {0} at 1,2\\n' >&2", file.display());
    let writer = ExternalTool {
        output_filters: vec!["warn: $FILE_PATH$ at $LINE$,$COLUMN$".to_owned()],
        ..tool("Rewrite", "/bin/sh", &format!("-c {}", zen_run::external_tool::quoted(&script)))
    };
    let id = save(&registry, &mut session, &writer);
    let started = run(&registry, &mut session, json!({ "id": id })).unwrap();
    let run_id = started["runId"].as_u64().unwrap();
    wait_done(&registry, &mut session, run_id);
    let lines = console(&registry, &mut session, run_id);
    let warning = lines.iter().find(|line| line["stream"] == "stderr").unwrap();
    assert_eq!(warning["links"][0]["file"], file.display().to_string());
    assert_eq!((warning["links"][0]["line"].as_u64(), warning["links"][0]["column"].as_u64()), (Some(1), Some(2)));
    assert_eq!(warning["links"][0]["range"]["start"], 0);
    let text = |session: &Session| session.state::<Workbench>().unwrap().documents()[0].text().to_owned();
    assert_eq!(text(&session), "old\n", "not yet: the window settles it");
    assert!(settle(&mut session));
    assert_eq!(text(&session), "new\n");
    assert!(!settle(&mut session), "once per run");

    let rerun = registry.dispatch(&mut session, "run.process.rerun", json!({ "runId": run_id })).unwrap();
    let again = rerun["runId"].as_u64().unwrap();
    wait_done(&registry, &mut session, again);
    assert_eq!(texts(&console(&registry, &mut session, again), "stderr").len(), 1, "Rerun runs the same line");

    let quiet = ExternalTool { synchronize: false, ..writer.clone() };
    let quiet_id =
        save(&registry, &mut session, &ExternalTool { id: String::new(), name: "Quiet".to_owned(), ..quiet });
    std::fs::write(&file, "edited elsewhere\n").unwrap();
    assert!(settle(&mut session), "the rerun synchronizes too");
    let waited = run(&registry, &mut session, json!({ "id": quiet_id, "waitMs": 20000 })).unwrap();
    assert_eq!(waited["summary"]["state"], "finished");
    assert!(!settle(&mut session), "Synchronize off");
}

#[test]
fn the_kind_of_external_tool_runs_is_never_offered() {
    let runs = Runs::default();
    assert!(runs.kinds().iter().all(|kind| kind.spec.id != KIND));
}

#[test]
fn a_new_tool_never_takes_the_id_of_a_tool_saved_after_it() {
    english();
    let tools = ExternalTools::load(None);
    let lint = tools.save_tool(tool("Lint", "/usr/bin/true", "")).unwrap();
    let kept = ExternalTool { id: "lint-copy".to_owned(), ..tool("Older", "/usr/bin/true", "") };
    let copy = ExternalTool { id: String::new(), ..tool("Lint copy", "/usr/bin/true", "") };
    let first = tools.get(&lint).unwrap();
    let replaced = tools.replace(vec![first.clone(), copy, kept, first]).unwrap();
    let ids: Vec<&str> = replaced.iter().map(|tool| tool.id.as_str()).collect();
    assert_eq!(ids, ["lint", "lint-copy-2", "lint-copy", "lint-2"]);
}

#[cfg(unix)]
#[test]
fn an_agent_starts_no_external_tool_in_safe_mode_and_the_person_still_does() {
    let (registry, mut session, _dir, project) = setup();
    let id = save(&registry, &mut session, &tool("Echo", "/bin/echo", "hello"));
    crate::trust::revoke_in_tests(&mut session, &project);
    let params = json!({ "id": id, "waitMs": 30_000 });
    let refused = zen_core::macros::as_agent(&mut session, |session| {
        registry.dispatch(session, "run.externalTools.run", params.clone())
    });
    assert!(format!("{refused:?}").contains(crate::trust::NOT_TRUSTED), "{refused:?}");
    let started = run(&registry, &mut session, params).unwrap();
    assert_eq!(started["summary"]["exitCode"], 0, "{started}");
    let rerun = json!({ "runId": started["runId"] });
    let refused = zen_core::macros::as_agent(&mut session, |session| {
        registry.dispatch(session, "run.process.rerun", rerun.clone())
    });
    assert!(format!("{refused:?}").contains(crate::trust::NOT_TRUSTED), "{refused:?}");
}
