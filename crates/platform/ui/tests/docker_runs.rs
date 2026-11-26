#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_plugins::native::{Exports, NativePlugin};
use zen_syntax::TextFeatures;
use zen_workbench::plugin_runs::{PluginRun, set_plugins};

const COMPOSE: &str = "name: shop\nservices:\n  web:\n    image: nginx\n  db:\n    image: postgres\n";

fn docker() -> Arc<dyn TextFeatures> {
    let exports = Exports { run_targets: Some(zen_plugin_dockerfile::run_targets_request), ..Exports::default() };
    Arc::new(NativePlugin::new(&["yaml"], exports).with_runs("dockerfile", &["docker"]))
}

#[test]
fn the_docker_plug_in_offers_each_compose_service_and_the_platform_runs_it() {
    zen_i18n::register(zen_workbench::LOCALES);
    set_plugins(Some(Arc::new(|| vec![docker()])));
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let program = bin.join("docker");
    std::fs::write(&program, "#!/bin/sh\nfor arg in \"$@\"; do echo \"[$arg]\"; done\npwd\n").unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let project = dir.path().canonicalize().unwrap().join("shop");
    std::fs::create_dir_all(project.join("deploy")).unwrap();
    let file = project.join("deploy/compose.yaml");
    std::fs::write(&file, COMPOSE).unwrap();
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
    let targets = registry.dispatch(&mut session, "run.targets.list", json!({ "path": file })).unwrap();
    let rows: Vec<(u64, &str)> = targets["targets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|target| (target["line"].as_u64().unwrap(), target["name"].as_str().unwrap()))
        .collect();
    assert_eq!(rows, [(2, "All services"), (3, "web"), (5, "db")], "{targets}");
    let params = json!({ "context": { "path": file, "line": 5 }, "waitMs": 30000 });
    let done = registry.dispatch(&mut session, "run.process.start", params).unwrap();
    let output = registry.dispatch(&mut session, "run.output.get", json!({ "runId": done["runId"] })).unwrap();
    let text: Vec<&str> =
        output["lines"].as_array().unwrap().iter().filter_map(|line: &Value| line["text"].as_str()).collect();
    let text = text.join("\n");
    assert!(text.contains("[compose]\n[-f]") && text.contains("[db]"), "{text}");
    assert!(text.contains(&format!("[{}]", file.display())), "{text}");
    assert!(text.contains(&project.join("deploy").display().to_string()), "runs in the file's folder: {text}");
}

#[test]
fn the_docker_tool_window_lists_containers_through_the_plug_in_and_stops_one() {
    zen_i18n::register(zen_workbench::LOCALES);
    let manifest: Value = serde_json::from_str(include_str!("../../../../plugins/dockerfile/plugin.json")).unwrap();
    let windows: Vec<zen_syntax::ToolWindowDeclaration> =
        serde_json::from_value(manifest["toolWindows"].clone()).unwrap();
    let exports = Exports {
        run_targets: Some(zen_plugin_dockerfile::run_targets_request),
        tool_rows: Some(zen_plugin_dockerfile::tool_rows_request),
        ..Exports::default()
    };
    let plugin: Arc<dyn TextFeatures> =
        Arc::new(NativePlugin::new(&["yaml"], exports).with_runs("dockerfile", &["docker"]).with_tool_windows(windows));
    set_plugins(Some(Arc::new(move || vec![Arc::clone(&plugin)])));
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let program = bin.join("docker");
    let script = concat!(
        "#!/bin/sh\n",
        "printf '%s|' \"$@\" >> \"${0%/*}/calls\"; echo >> \"${0%/*}/calls\"\n",
        "if [ \"$1 $2\" = 'container ls' ]; then\n",
        "  echo '{\"ID\":\"3f2a\",\"Image\":\"nginx\",\"Names\":\"shop-web-1\",\"Ports\":\"\",\"State\":\"running\",\"Status\":\"Up 2 hours\"}'\n",
        "fi\n",
    );
    std::fs::write(&program, script).unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let project = dir.path().canonicalize().unwrap().join("shop");
    std::fs::create_dir_all(&project).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    let env = zen_workbench::build_tools::BuildEnv::default();
    env.set_data_dir(Some(dir.path().join("data")));
    session.set_state(env);
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    registry.dispatch(&mut session, "project.trust.grant", json!({})).unwrap();
    zen_workbench::plugin_tools::set_folders(&mut session, vec![bin.clone()]);
    let section = json!({ "plugin": "dockerfile", "window": "docker", "section": "containers" });
    let rows = registry.dispatch(&mut session, "plugins.toolWindow.rows", section.clone()).unwrap();
    assert_eq!(rows["rows"][0]["columns"], json!(["shop-web-1", "nginx", "Up 2 hours", ""]), "{rows}");
    assert_eq!(rows["rows"][0]["state"], "running");
    let mut stop = section;
    stop["action"] = json!("stop");
    stop["rows"] = json!(["3f2a"]);
    stop["waitMs"] = json!(20000);
    let done = registry.dispatch(&mut session, "plugins.toolWindow.act", stop).unwrap();
    assert_eq!(done["done"][0]["ok"], true, "{done}");
    let calls = std::fs::read_to_string(bin.join("calls")).unwrap();
    assert!(calls.lines().any(|call| call == "container|stop|3f2a|"), "{calls}");
    assert!(calls.lines().next().unwrap().starts_with("container|ls|--all|--no-trunc|--format|{{json .}}|"), "{calls}");
}
