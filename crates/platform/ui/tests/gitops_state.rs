#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_plugins::native::{Exports, NativePlugin};
use zen_syntax::TextFeatures;
use zen_workbench::plugin_runs::{PluginRun, set_plugins};

const KUBECTL: &str = r##"#!/bin/sh
here=${0%/*}
printf '%s|' "$@" >> "$here/calls"; echo >> "$here/calls"
ctx=''; prev=''
for arg in "$@"; do [ "$prev" = --context ] && ctx=$arg; prev=$arg; done
case "$*" in
  'config view -o json')
    echo '{"current-context":"dev","contexts":[{"name":"dev","context":{"namespace":"team"}},{"name":"prod","context":{"namespace":"team"}}]}'
    exit 0 ;;
  *'get namespaces -o json')
    echo '{"items":[{"metadata":{"name":"team"}}]}'
    exit 0 ;;
  *'get kustomizations.'*)
    echo '{"items":[{"apiVersion":"kustomize.toolkit.fluxcd.io/v1","kind":"Kustomization","metadata":{"name":"apps","namespace":"flux-system"},"status":{"lastAppliedRevision":"main@'"$ctx"'","conditions":[{"type":"Ready","status":"False","reason":"BuildFailed","message":"kustomization path not found"}]}}]}'
    exit 0 ;;
  *'get applications.argoproj.io'*)
    echo 'error: the server does not have a resource type "applications"' >&2
    exit 1 ;;
  *'get events'*)
    echo "events for $*"
    exit 0 ;;
esac
echo "unexpected $*" >&2
exit 3
"##;

const APPS: &str = "apiVersion: kustomize.toolkit.fluxcd.io/v1\nkind: Kustomization\nmetadata:\n  name: apps\n  namespace: flux-system\nspec:\n  path: ./apps\n";

struct Setup {
    registry: Registry,
    session: Session,
    bin: PathBuf,
    _dir: tempfile::TempDir,
}

fn manifest(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn plugins() -> (Arc<dyn TextFeatures>, Arc<dyn TextFeatures>) {
    let kube = manifest(include_str!("../../../../plugins/kubernetes/plugin.json"));
    let gitops = manifest(include_str!("../../../../plugins/gitops/plugin.json"));
    let windows = |manifest: &Value| serde_json::from_value(manifest["toolWindows"].clone()).unwrap();
    let rules = |manifest: &Value| -> Vec<zen_syntax::PersonOnly> {
        serde_json::from_value(manifest["personOnly"].clone()).unwrap()
    };
    let kubernetes = NativePlugin::new(
        &[],
        Exports { tool_rows: Some(zen_plugin_kubernetes::tool_rows_request), ..Exports::default() },
    )
    .with_runs("kubernetes", &["kubectl"])
    .with_person_only(&rules(&kube))
    .with_tool_windows(windows(&kube));
    let index: zen_syntax::IndexDeclaration = serde_json::from_value(gitops["index"].clone()).unwrap();
    let flux = NativePlugin::new(
        &[],
        Exports {
            index: Some(zen_plugin_gitops::index_request),
            tool_rows: Some(zen_plugin_gitops::tool_rows_request),
            ..Exports::default()
        },
    )
    .with_runs("gitops", &["helm", "kustomize", "kubectl"])
    .with_person_only(&rules(&gitops))
    .with_tool_windows(windows(&gitops))
    .with_index("gitops", index);
    (Arc::new(kubernetes), Arc::new(flux))
}

fn setup() -> Setup {
    zen_i18n::register(zen_workbench::LOCALES);
    let (kubernetes, gitops) = plugins();
    let indexed = Arc::clone(&gitops);
    set_plugins(Some(Arc::new(move || vec![Arc::clone(&kubernetes), Arc::clone(&gitops)])));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("kubectl"), KUBECTL).unwrap();
    std::fs::set_permissions(bin.join("kubectl"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let project = root.join("fleet");
    std::fs::create_dir_all(project.join("apps")).unwrap();
    std::fs::write(project.join("apps.yaml"), APPS).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    let env = zen_workbench::build_tools::BuildEnv::default();
    env.set_data_dir(Some(root.join("data")));
    session.set_state(env);
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    registry.dispatch(&mut session, "project.trust.grant", json!({})).unwrap();
    zen_workbench::plugin_tools::set_folders(&mut session, vec![bin.clone()]);
    session
        .state_mut::<zen_workbench::run::Runs>()
        .set_providers(vec![Arc::new(PluginRun::in_folders(vec![bin.clone()]))], zen_run::RunEnv::default());
    session
        .state_mut::<zen_workbench::project_index::Indexes>()
        .set_plugins(Arc::new(move || vec![Arc::clone(&indexed)]));
    registry.dispatch(&mut session, "plugins.index.get", json!({ "waitMs": 30000 })).unwrap();
    Setup { registry, session, bin, _dir: dir }
}

impl Setup {
    fn call(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap()
    }

    fn state(&mut self) -> Value {
        self.call(
            "plugins.toolWindow.rows",
            json!({ "plugin": "gitops", "window": "gitops", "section": "resources", "waitMs": 20000 }),
        )
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.bin.join("calls")).unwrap_or_default()
    }
}

fn apps(answer: &Value) -> &Value {
    let rows = answer["rows"].as_array().unwrap();
    rows.iter().find(|row| row["id"] == "flux:Kustomization/flux-system/apps").unwrap_or_else(|| panic!("{answer}"))
}

#[test]
fn a_flux_kustomization_shows_the_cluster_s_state_from_the_kubernetes_window_s_context_read_only() {
    let mut setup = setup();
    let answer = setup.state();
    let cluster = &apps(&answer)["cluster"];
    assert_eq!(cluster["state"], "error", "{answer}");
    assert_eq!(cluster["columns"][0], "False (BuildFailed)");
    assert_eq!(cluster["columns"][3], "main@dev");
    assert_eq!(cluster["columns"][4], "kustomization path not found");
    assert_eq!(answer["cluster"]["from"], "State from dev");
    assert!(answer["cluster"]["errors"].to_string().contains("applications"), "Argo CD is missing, Flux stays");

    setup.call("plugins.toolWindow.rows", json!({ "plugin": "kubernetes", "window": "kubernetes", "section": "pods" }));
    let chose = json!({ "plugin": "kubernetes", "window": "kubernetes", "choice": "context", "value": "prod" });
    setup.call("plugins.toolWindow.choose", chose);
    let at_once = setup.call(
        "plugins.toolWindow.rows",
        json!({ "plugin": "gitops", "window": "gitops", "section": "resources", "waitMs": 0 }),
    );
    let shown = &apps(&at_once)["cluster"];
    assert!(shown.is_null() || shown["columns"][3] == "main@prod", "never dev's state under prod: {at_once}");
    assert_eq!(setup.state()["cluster"]["from"], "State from prod");

    let events = json!({
        "plugin": "gitops", "window": "gitops", "section": "resources", "action": "events",
        "rows": ["flux:Kustomization/flux-system/apps"],
    });
    let started = setup.call("plugins.toolWindow.act", events);
    assert!(started["runId"].as_u64().is_some(), "{started}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !setup.calls().contains("get|events") {
        assert!(std::time::Instant::now() < deadline, "{}", setup.calls());
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let calls = setup.calls();
    assert!(
        calls.contains(
            "--context|prod|get|events|--namespace|flux-system|--field-selector|involvedObject.kind=Kustomization,involvedObject.name=apps|"
        ),
        "{calls}"
    );
    for line in calls.lines() {
        let words: Vec<&str> = line.split('|').collect();
        assert!(words.contains(&"get") || words.starts_with(&["config", "view"]), "only reads: {line}");
    }
}
