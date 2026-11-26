#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_plugins::native::{Exports, NativePlugin};
use zen_syntax::TextFeatures;
use zen_workbench::plugin_runs::set_plugins;

const KUBECTL: &str = r#"#!/bin/sh
case "$*" in
  'config view -o json')
    out='{"current-context":"b-ctx","contexts":['
    sep=''
    IFS=:
    for file in $KUBECONFIG; do
      [ -f "$file" ] || continue
      read -r name < "$file"
      out="$out$sep{\"name\":\"$name\",\"context\":{\"cluster\":\"c\",\"user\":\"u\"}}"
      sep=','
    done
    echo "$out]}"
    exit 0 ;;
  *'get namespaces -o json')
    echo '{"items":[{"metadata":{"name":"default"},"status":{"phase":"Active"}}]}'
    exit 0 ;;
esac
echo '{"items":[]}'
"#;

#[test]
fn the_kubernetes_window_reads_the_kubeconfig_files_the_login_shell_names_and_says_which() {
    zen_i18n::register(zen_workbench::LOCALES);
    let manifest: Value = serde_json::from_str(include_str!("../../../../plugins/kubernetes/plugin.json")).unwrap();
    let windows: Vec<zen_syntax::ToolWindowDeclaration> =
        serde_json::from_value(manifest["toolWindows"].clone()).unwrap();
    let exports = Exports { tool_rows: Some(zen_plugin_kubernetes::tool_rows_request), ..Exports::default() };
    let plugin: Arc<dyn TextFeatures> =
        Arc::new(NativePlugin::new(&[], exports).with_runs("kubernetes", &["kubectl"]).with_tool_windows(windows));
    set_plugins(Some(Arc::new(move || vec![Arc::clone(&plugin)])));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let bin = root.join("login-bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("kubectl"), KUBECTL).unwrap();
    std::fs::set_permissions(bin.join("kubectl"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let kube = root.join("kube");
    std::fs::create_dir_all(&kube).unwrap();
    std::fs::write(kube.join("work.yaml"), "a-ctx\n").unwrap();
    std::fs::write(kube.join("home.yaml"), "b-ctx\n").unwrap();
    let files = [kube.join("work.yaml"), kube.join("home.yaml"), kube.join("gone.yaml")];
    let kubeconfig = std::env::join_paths(&files).unwrap().to_string_lossy().into_owned();
    let variables = [("KUBECONFIG", kubeconfig), ("PATH", format!("{}:/usr/bin:/bin", bin.display()))]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
    zen_env::adopt(zen_env::Environment { shell: "/bin/zsh".into(), variables });

    let project = root.join("shop");
    std::fs::create_dir_all(&project).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    let env = zen_workbench::build_tools::BuildEnv::default();
    env.set_data_dir(Some(root.join("data")));
    session.set_state(env);
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    registry.dispatch(&mut session, "project.trust.grant", json!({})).unwrap();

    let contexts = json!({ "plugin": "kubernetes", "window": "kubernetes", "section": "contexts" });
    let rows = registry.dispatch(&mut session, "plugins.toolWindow.rows", contexts).unwrap();
    let context = &rows["choices"][0];
    let options: Vec<&str> = context["options"].as_array().unwrap().iter().map(|o| o["id"].as_str().unwrap()).collect();
    assert_eq!(options, ["a-ctx", "b-ctx"], "every file of KUBECONFIG, found with the login shell's PATH: {rows}");
    assert_eq!(context["value"], "b-ctx");
    let source: Vec<(String, bool)> = context["source"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| (file["path"].as_str().unwrap().to_owned(), file["exists"].as_bool().unwrap()))
        .collect();
    let shown: Vec<(String, bool)> = files.iter().map(|file| (file.display().to_string(), file.exists())).collect();
    assert_eq!(source, shown, "the header names the files, the missing one marked");

    let status = registry.dispatch(&mut session, "ide.environment.get", json!({})).unwrap();
    assert_eq!(status["state"], "ready");
    assert!(status["names"].as_array().unwrap().contains(&json!("KUBECONFIG")));
    assert!(!status.to_string().contains("work.yaml"), "the status names variables, never their values: {status}");
}
