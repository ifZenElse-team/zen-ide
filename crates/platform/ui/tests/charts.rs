#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_plugins::native::{Exports, NativePlugin};
use zen_syntax::TextFeatures;
use zen_workbench::plugin_runs::{PluginRun, set_plugins};

const HELM: &str = r##"#!/bin/sh
printf '%s|' "$@" >> "${0%/*}/calls"
echo "---"
echo "# Source: web/templates/deployment.yaml"
echo "kind: Deployment"
echo "replicas: 2"
"##;

const TEMPLATE: &str = "kind: Deployment\nreplicas: {{ .Values.replicaCount }}\ntag: {{ .Values.image.digest }}\n";

struct Setup {
    registry: Registry,
    session: Session,
    project: PathBuf,
    bin: PathBuf,
    _dir: tempfile::TempDir,
}

fn gitops() -> Arc<dyn TextFeatures> {
    let manifest: Value = serde_json::from_str(include_str!("../../../../plugins/gitops/plugin.json")).unwrap();
    let index: zen_syntax::IndexDeclaration = serde_json::from_value(manifest["index"].clone()).unwrap();
    let files: Vec<String> = serde_json::from_value(manifest["targetFiles"].clone()).unwrap();
    let files: Vec<&str> = files.iter().map(String::as_str).collect();
    let exports = Exports {
        index: Some(zen_plugin_gitops::index_request),
        run_targets: Some(zen_plugin_gitops::run_targets_request),
        ..Exports::default()
    };
    Arc::new(
        NativePlugin::new(&[], exports)
            .with_runs("gitops", &["helm", "kustomize"])
            .with_target_files(&files)
            .with_index("gitops", index),
    )
}

fn setup() -> Setup {
    zen_i18n::register(zen_workbench::LOCALES);
    let plugin = gitops();
    let indexed = Arc::clone(&plugin);
    set_plugins(Some(Arc::new(move || vec![Arc::clone(&plugin)])));
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("helm"), HELM).unwrap();
    std::fs::set_permissions(bin.join("helm"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let project = root.join("fleet");
    std::fs::create_dir_all(project.join("charts/web/templates")).unwrap();
    std::fs::write(project.join("charts/web/Chart.yaml"), "apiVersion: v2\nname: web\nversion: 0.1.0\n").unwrap();
    std::fs::write(project.join("charts/web/values.yaml"), "replicaCount: 2\nimage:\n  tag: \"1.27\"\n").unwrap();
    std::fs::write(project.join("charts/web/templates/deployment.yaml"), TEMPLATE).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    let mut session = Session::new();
    let env = zen_workbench::build_tools::BuildEnv::default();
    env.set_data_dir(Some(root.join("data")));
    session.set_state(env);
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    registry.dispatch(&mut session, "project.trust.grant", json!({})).unwrap();
    session
        .state_mut::<zen_workbench::run::Runs>()
        .set_providers(vec![Arc::new(PluginRun::in_folders(vec![bin.clone()]))], zen_run::RunEnv::default());
    session
        .state_mut::<zen_workbench::project_index::Indexes>()
        .set_plugins(Arc::new(move || vec![Arc::clone(&indexed)]));
    Setup { registry, session, project, bin, _dir: dir }
}

impl Setup {
    fn call(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap()
    }
}

#[test]
fn a_template_s_values_lead_to_their_keys_complete_and_warn_when_no_values_file_has_them() {
    let mut setup = setup();
    let indexed = setup.call("plugins.index.get", json!({ "plugin": "gitops", "waitMs": 30000 }));
    assert_eq!(indexed["indexing"], false, "{indexed}");
    let template = setup.project.join("charts/web/templates/deployment.yaml");
    let problems = setup.call("problems.file.list", json!({ "path": template }));
    let text = problems.to_string();
    assert!(text.contains("image.digest") && !text.contains("replicaCount"), "{text}");

    setup.call("file.document.open", json!({ "path": template }));
    let offset = TEMPLATE.find(".Values.replicaCount").unwrap() + 9;
    let went = setup.call("navigate.declaration.goTo", json!({ "path": template, "offset": offset }));
    assert_eq!(went["navigated"], true, "{went}");
    let workbench = setup.session.state::<zen_workbench::Workbench>().unwrap();
    assert_eq!(workbench.active_document().unwrap().path(), setup.project.join("charts/web/values.yaml"));

    setup.call("file.document.open", json!({ "path": template }));
    let typed = format!("{TEMPLATE}image: {{{{ .Values.ima");
    setup.call("editor.text.write", json!({ "path": template, "text": typed }));
    setup.call("navigate.place.goTo", json!({ "path": template, "offset": typed.chars().count() }));
    let listed = setup.call("editor.completion.start", json!({}));
    let labels: Vec<&str> =
        listed["items"].as_array().unwrap().iter().filter_map(|item| item["label"].as_str()).collect();
    assert_eq!(labels, ["image", "image.tag"], "{listed}");
}

#[test]
fn a_chart_s_preview_runs_helm_template_locally_and_opens_the_yaml_read_only() {
    let mut setup = setup();
    let chart = setup.project.join("charts/web/Chart.yaml");
    let listed = setup.call("run.targets.list", json!({ "path": chart }));
    let names: Vec<&str> =
        listed["targets"].as_array().unwrap().iter().filter_map(|target| target["name"].as_str()).collect();
    assert_eq!(names, ["Preview with helm template"], "{listed}");
    setup.call("run.process.start", json!({ "context": { "path": chart, "line": 2 }, "waitMs": 30000 }));
    let deadline = Instant::now() + Duration::from_secs(30);
    let ready = loop {
        let ready = zen_workbench::plugin_document::take(&mut setup.session);
        if !ready.is_empty() {
            break ready;
        }
        assert!(Instant::now() < deadline, "the preview never came back");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(ready[0].text.contains("kind: Deployment"), "{:?}", ready[0]);
    assert_eq!(ready[0].asked.command, "helm template charts/web");
    let opened = zen_workbench::plugin_document::open(&mut setup.session, &ready[0]).unwrap();
    assert!(!opened.starts_with(&setup.project));
    assert!(zen_workbench::virtual_documents::reason(&opened).is_some(), "read-only");
    let calls = std::fs::read_to_string(setup.bin.join("calls")).unwrap();
    assert_eq!(calls, format!("template|{}|", setup.project.join("charts/web").display()));
}
