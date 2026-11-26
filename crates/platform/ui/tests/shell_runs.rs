#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_plugins::native::{Exports, NativePlugin};
use zen_syntax::TextFeatures;
use zen_workbench::plugin_runs::{PluginRun, set_plugins};

fn shell() -> Arc<dyn TextFeatures> {
    let exports = Exports { run_targets: Some(zen_plugin_shell::run_targets_request), ..Exports::default() };
    Arc::new(NativePlugin::new(&["sh", "bash", "zsh"], exports).with_runs("shell", zen_plugin_shell::SHELLS))
}

#[test]
fn a_script_runs_with_the_shell_its_hashbang_names_in_its_own_folder() {
    zen_i18n::register(zen_workbench::LOCALES);
    set_plugins(Some(Arc::new(|| vec![shell()])));
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let program = bin.join("bash");
    std::fs::write(&program, "#!/bin/sh\nfor arg in \"$@\"; do echo \"[$arg]\"; done\npwd\n").unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let project = dir.path().canonicalize().unwrap().join("shop");
    std::fs::create_dir_all(project.join("tools")).unwrap();
    let script = project.join("tools/build.sh");
    std::fs::write(&script, "#!/bin/bash -e\necho built\n").unwrap();
    let plain = project.join("tools/notes.txt");
    std::fs::write(&plain, "#!/bin/bash\n").unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    let env = zen_workbench::build_tools::BuildEnv::default();
    env.set_data_dir(Some(dir.path().join("data")));
    session.set_state(env);
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    registry.dispatch(&mut session, "project.trust.grant", json!({})).unwrap();
    session
        .state_mut::<zen_workbench::run::Runs>()
        .set_providers(vec![Arc::new(PluginRun::in_folders(vec![bin]))], zen_run::RunEnv::default());
    let targets = registry.dispatch(&mut session, "run.targets.list", json!({ "path": script })).unwrap();
    let rows: Vec<(u64, &str)> = targets["targets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|target| (target["line"].as_u64().unwrap(), target["name"].as_str().unwrap()))
        .collect();
    assert_eq!(rows, [(1, "build.sh")], "{targets}");
    let none = registry.dispatch(&mut session, "run.targets.list", json!({ "path": plain })).unwrap();
    assert_eq!(none["targets"], json!([]), "a file the plug-in does not serve has no run icon");
    let params = json!({ "context": { "path": script, "line": 1 }, "waitMs": 30000 });
    let done = registry.dispatch(&mut session, "run.process.start", params).unwrap();
    let output = registry.dispatch(&mut session, "run.output.get", json!({ "runId": done["runId"] })).unwrap();
    let text: Vec<&str> =
        output["lines"].as_array().unwrap().iter().filter_map(|line: &Value| line["text"].as_str()).collect();
    let text = text.join("\n");
    assert!(text.contains(&format!("[-e]\n[{}]", script.display())), "{text}");
    assert!(text.contains(&project.join("tools").display().to_string()), "runs in the script's folder: {text}");
}
