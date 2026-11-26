#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_plugins::native::{Exports, NativePlugin};
use zen_syntax::TextFeatures;
use zen_workbench::plugin_runs::set_plugins;

const KUBECTL: &str = r#"#!/bin/sh
here=${0%/*}
printf '%s|' "$@" >> "$here/calls"; echo >> "$here/calls"
ctx=''; prev=''
for arg in "$@"; do [ "$prev" = --context ] && ctx=$arg; prev=$arg; done
case "$*" in
  'config view -o json')
    echo '{"current-context":"dev","contexts":[{"name":"dev","context":{"cluster":"kind-dev","user":"me","namespace":"team"}},{"name":"prod","context":{"cluster":"eks","user":"ops"}}],"users":[{"name":"me","user":{"token":"REDACTED"}}]}'
    exit 0 ;;
  *'get namespaces -o json')
    echo '{"items":[{"metadata":{"name":"default"},"status":{"phase":"Active"}},{"metadata":{"name":"team"},"status":{"phase":"Active"}}]}'
    exit 0 ;;
  *'get pods -o json')
    echo "{\"items\":[{\"metadata\":{\"name\":\"web-$ctx\"},\"spec\":{\"containers\":[{\"name\":\"web\"}]},\"status\":{\"phase\":\"Running\",\"containerStatuses\":[{\"name\":\"web\",\"ready\":true,\"restartCount\":0,\"state\":{\"running\":{}}}]}}]}"
    exit 0 ;;
esac
echo "done: $*"
"#;

fn failure(error: DispatchError) -> zen_core::CommandError {
    match error {
        DispatchError::Failed { error, .. } => error,
        other => panic!("not a command failure: {other:?}"),
    }
}

#[test]
fn the_kubernetes_window_lists_in_the_chosen_context_and_asks_before_deleting_naming_it() {
    zen_i18n::register(zen_workbench::LOCALES);
    let manifest: Value = serde_json::from_str(include_str!("../../../../plugins/kubernetes/plugin.json")).unwrap();
    let windows: Vec<zen_syntax::ToolWindowDeclaration> =
        serde_json::from_value(manifest["toolWindows"].clone()).unwrap();
    let exports = Exports { tool_rows: Some(zen_plugin_kubernetes::tool_rows_request), ..Exports::default() };
    let plugin: Arc<dyn TextFeatures> =
        Arc::new(NativePlugin::new(&[], exports).with_runs("kubernetes", &["kubectl"]).with_tool_windows(windows));
    set_plugins(Some(Arc::new(move || vec![Arc::clone(&plugin)])));
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let program = bin.join("kubectl");
    std::fs::write(&program, KUBECTL).unwrap();
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

    let pods = json!({ "plugin": "kubernetes", "window": "kubernetes", "section": "pods" });
    let rows = registry.dispatch(&mut session, "plugins.toolWindow.rows", pods.clone()).unwrap();
    assert_eq!(rows["choices"][0]["value"], "dev", "{rows}");
    assert_eq!(rows["choices"][1]["value"], "team", "the context's own namespace");
    assert_eq!(rows["rows"][0]["id"], "pod/web-dev");
    assert_eq!(rows["rows"][0]["state"], "running");

    let mut delete = pods.clone();
    delete["action"] = json!("delete");
    delete["rows"] = json!(["pod/web-dev"]);
    delete["waitMs"] = json!(20000);
    let asked = failure(registry.dispatch(&mut session, "plugins.toolWindow.act", delete.clone()).unwrap_err());
    assert_eq!(asked.code.as_deref(), Some("confirmationNeeded"));
    assert_eq!(asked.message, "Delete web-dev in the context dev, namespace team? This cannot be undone.");
    delete["confirmed"] = json!(true);
    let done = registry.dispatch(&mut session, "plugins.toolWindow.act", delete).unwrap();
    assert_eq!(done["done"][0]["ok"], true, "{done}");

    let choose = json!({ "plugin": "kubernetes", "window": "kubernetes", "choice": "context", "value": "prod" });
    registry.dispatch(&mut session, "plugins.toolWindow.choose", choose).unwrap();
    let rows = registry.dispatch(&mut session, "plugins.toolWindow.rows", pods).unwrap();
    assert_eq!(rows["rows"][0]["id"], "pod/web-prod", "{rows}");
    assert_eq!(rows["choices"][1]["value"], "default", "prod names no namespace of its own");

    let calls = std::fs::read_to_string(bin.join("calls")).unwrap();
    assert!(
        calls.lines().any(|call| call == "--context|dev|--namespace|team|delete|pod/web-dev|"),
        "the delete names the context and namespace it was asked in: {calls}"
    );
    assert!(
        calls
            .lines()
            .any(|call| call.starts_with("--context|prod|--namespace|default|--request-timeout|10s|get|pods|"))
    );
    assert!(!calls.contains("use-context") && !calls.contains("set-context") && !calls.contains("--raw"), "{calls}");
}
