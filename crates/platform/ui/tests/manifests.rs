#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::{DispatchError, Registry, Session};
use zen_plugins::native::{Exports, NativePlugin};
use zen_syntax::TextFeatures;
use zen_workbench::plugin_runs::{PluginRun, set_plugins};

const KUBECTL: &str = r#"#!/bin/sh
here=${0%/*}
printf '%s|' "$@" >> "$here/calls"; printf 'diff=%s\n' "$KUBECTL_EXTERNAL_DIFF" >> "$here/calls"
case "$*" in
  'config view -o json')
    while IFS= read -r line; do echo "$line"; done < "$here/config"
    exit 0 ;;
  *'get namespaces -o json')
    echo '{"items":[{"metadata":{"name":"default"}},{"metadata":{"name":"team"}}]}'
    exit 0 ;;
  *' diff -f '*)
    echo "diff -N -U1000000 /tmp/LIVE-7/apps.v1.Deployment.team.web /tmp/MERGED-7/apps.v1.Deployment.team.web"
    echo "--- /tmp/LIVE-7/apps.v1.Deployment.team.web	2026-11-16"
    echo "+++ /tmp/MERGED-7/apps.v1.Deployment.team.web	2026-11-16"
    echo "@@ -1,3 +1,3 @@"
    echo " kind: Deployment"
    echo "-replicas: 2"
    echo "+replicas: 3"
    echo " name: web"
    exit 1 ;;
esac
echo "done"
"#;

const CONFIG: &str = r#"{"current-context":"dev","contexts":[{"name":"dev","context":{"cluster":"kind","user":"me","namespace":"team"}}]}"#;

const MANIFEST: &str = "# web\napiVersion: apps/v1\nkind: Deployment\nmetadata:\n  name: web\n";

struct Setup {
    registry: Registry,
    session: Session,
    project: PathBuf,
    bin: PathBuf,
    _dir: tempfile::TempDir,
}

fn kubernetes() -> Arc<dyn TextFeatures> {
    let manifest: Value = serde_json::from_str(include_str!("../../../../plugins/kubernetes/plugin.json")).unwrap();
    let windows: Vec<zen_syntax::ToolWindowDeclaration> =
        serde_json::from_value(manifest["toolWindows"].clone()).unwrap();
    let rules: Vec<zen_syntax::PersonOnly> = serde_json::from_value(manifest["personOnly"].clone()).unwrap();
    let files: Vec<String> = serde_json::from_value(manifest["targetFiles"].clone()).unwrap();
    let files: Vec<&str> = files.iter().map(String::as_str).collect();
    let exports = Exports {
        tool_rows: Some(zen_plugin_kubernetes::tool_rows_request),
        run_targets: Some(zen_plugin_kubernetes::run_targets_request),
        ..Exports::default()
    };
    Arc::new(
        NativePlugin::new(&[], exports)
            .with_runs("kubernetes", &["kubectl"])
            .with_person_only(&rules)
            .with_target_files(&files)
            .with_tool_windows(windows),
    )
}

fn setup(config: &str, trusted: bool) -> Setup {
    zen_i18n::register(zen_workbench::LOCALES);
    let plugin = kubernetes();
    set_plugins(Some(Arc::new(move || vec![Arc::clone(&plugin)])));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("kubectl"), KUBECTL).unwrap();
    std::fs::set_permissions(bin.join("kubectl"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(bin.join("config"), format!("{config}\n")).unwrap();
    let project = root.join("shop");
    std::fs::create_dir_all(project.join("deploy/db")).unwrap();
    std::fs::write(project.join("deploy/web.yaml"), MANIFEST).unwrap();
    std::fs::write(project.join("deploy/db/db.yaml"), MANIFEST.replace("web", "db")).unwrap();
    std::fs::write(project.join("deploy/values.yaml"), "replicas: 2\n").unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    let env = zen_workbench::build_tools::BuildEnv::default();
    env.set_data_dir(Some(root.join("data")));
    session.set_state(env);
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    if trusted {
        registry.dispatch(&mut session, "project.trust.grant", json!({})).unwrap();
    }
    zen_workbench::plugin_tools::set_folders(&mut session, vec![bin.clone()]);
    session
        .state_mut::<zen_workbench::run::Runs>()
        .set_providers(vec![Arc::new(PluginRun::in_folders(vec![bin.clone()]))], zen_run::RunEnv::default());
    Setup { registry, session, project, bin, _dir: dir }
}

impl Setup {
    fn call(&mut self, id: &str, params: Value) -> Result<Value, DispatchError> {
        self.registry.dispatch(&mut self.session, id, params)
    }

    fn agent(&mut self, id: &str, params: Value) -> Result<Value, DispatchError> {
        let Setup { registry, session, .. } = self;
        zen_core::macros::as_agent(session, |session| registry.dispatch(session, id, params))
    }

    fn targets(&mut self, path: &Path) -> Vec<Value> {
        let listed = self.call("run.targets.list", json!({ "path": path, "full": true })).unwrap();
        listed["targets"].as_array().unwrap().clone()
    }

    fn calls(&self) -> String {
        std::fs::read_to_string(self.bin.join("calls")).unwrap_or_default()
    }
}

fn failure(error: DispatchError) -> zen_core::CommandError {
    match error {
        DispatchError::Failed { error, .. } => error,
        other => panic!("not a command failure: {other:?}"),
    }
}

#[test]
fn a_manifest_s_diff_runs_in_the_window_s_context_and_opens_the_cluster_s_object_beside_the_file_s() {
    let mut setup = setup(CONFIG, true);
    let web = setup.project.join("deploy/web.yaml");
    let targets = setup.targets(&web);
    let names: Vec<(&str, u64)> =
        targets.iter().map(|target| (target["name"].as_str().unwrap(), target["line"].as_u64().unwrap())).collect();
    assert_eq!(names, [("Diff with Cluster", 2), ("Apply to Cluster", 2)], "on the apiVersion line, from 1");
    assert!(setup.targets(&setup.project.join("deploy/values.yaml")).is_empty(), "no manifest, no targets");

    let started =
        setup.call("run.process.start", json!({ "context": { "path": web, "line": 2 }, "waitMs": 30000 })).unwrap();
    assert!(started["runId"].as_u64().is_some(), "{started}");
    let deadline = Instant::now() + Duration::from_secs(30);
    let diffs = loop {
        let taken = zen_workbench::plugin_diff::take(&mut setup.session);
        if !taken.is_empty() {
            break taken;
        }
        assert!(Instant::now() < deadline, "the diff never came back");
        std::thread::sleep(Duration::from_millis(20));
    };
    let result = &diffs[0];
    assert_eq!(result.objects.len(), 1);
    let object = &result.objects[0];
    assert_eq!(object.left, "kind: Deployment\nreplicas: 2\nname: web\n");
    assert_eq!(object.right, "kind: Deployment\nreplicas: 3\nname: web\n");
    let (left, right) = (result.left_title(object), result.right_title(object));
    assert!(left.contains("Live") && left.contains("dev") && left.contains("team"), "{left}");
    assert!(right.contains("deploy/web.yaml"), "{right}");
    let calls = setup.calls();
    assert!(
        calls.contains(&format!("--context|dev|--namespace|team|diff|-f|{}|diff=diff -N -U1000000", web.display())),
        "{calls}"
    );
}

#[test]
fn applying_a_manifest_is_the_person_s_every_time_and_never_an_agent_s() {
    let mut setup = setup(CONFIG, true);
    let web = setup.project.join("deploy/web.yaml");
    let apply = setup.targets(&web)[1]["configuration"].clone();
    let asked =
        failure(setup.call("run.process.start", json!({ "configuration": apply, "waitMs": 30000 })).unwrap_err());
    assert_eq!(asked.code.as_deref(), Some("personConfirmation"), "{}", asked.message);
    let line = format!("kubectl --context dev --namespace team apply -f {}", web.display());
    assert!(
        asked.message.contains(&line),
        "the question names the cluster, the namespace and the file: {}",
        asked.message
    );
    setup.call("plugins.runAccess.set", json!({ "plugin": "kubernetes", "access": "full" })).unwrap();
    let refused =
        failure(setup.agent("run.process.start", json!({ "configuration": apply, "waitMs": 30000 })).unwrap_err());
    assert_eq!(refused.code.as_deref(), Some("userOnly"), "even with full access");
    assert!(!setup.calls().contains("apply"), "nothing was applied");
}

#[test]
fn a_folder_of_manifests_is_diffed_and_applied_recursively_from_its_own_targets() {
    let mut setup = setup(CONFIG, true);
    let folder = setup.project.join("deploy");
    let targets = setup.targets(&folder);
    let names: Vec<&str> = targets.iter().map(|target| target["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Diff Folder with Cluster", "Apply Folder to Cluster"]);
    let args = targets[0]["configuration"]["options"]["args"].as_array().unwrap().clone();
    assert_eq!(args.last().unwrap(), "-R");
    setup.call("run.process.start", json!({ "context": { "path": folder, "line": 1 }, "waitMs": 30000 })).unwrap();
    assert!(setup.calls().contains(&format!("diff|-f|{}|-R|", folder.display())), "{}", setup.calls());
}

#[test]
fn without_a_valid_context_or_in_an_untrusted_project_nothing_runs() {
    let mut empty = setup(r#"{"contexts":[]}"#, true);
    let web = empty.project.join("deploy/web.yaml");
    let refused =
        failure(empty.call("run.process.start", json!({ "context": { "path": web, "line": 2 } })).unwrap_err());
    assert_eq!(refused.code.as_deref(), Some("choiceNeeded"), "{}", refused.message);
    assert!(!empty.calls().contains("diff|"), "{}", empty.calls());

    let mut untrusted = setup(CONFIG, false);
    let web = untrusted.project.join("deploy/web.yaml");
    let refused =
        failure(untrusted.call("run.process.start", json!({ "context": { "path": web, "line": 2 } })).unwrap_err());
    assert_eq!(refused.code.as_deref(), Some("notTrusted"), "{}", refused.message);
    assert!(untrusted.calls().is_empty(), "not even the kubeconfig is read: {}", untrusted.calls());
}
