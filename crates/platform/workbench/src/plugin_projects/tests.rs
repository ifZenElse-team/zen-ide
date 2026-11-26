use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_project::support::ProjectSupport;
use zen_run::RunEnv;

use super::{PluginProject, PluginProjects};
use crate::npm::NpmRun;
use crate::run::Runs;

fn plugin(id: &str) -> PluginProject {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../../../plugins/{id}/plugin.json"));
    let manifest: Value = serde_json::from_slice(&std::fs::read(manifest).unwrap()).unwrap();
    let support = ProjectSupport::parse(&manifest["project"].to_string()).unwrap();
    PluginProject { id: id.to_owned(), name: manifest["name"].as_str().unwrap().to_owned(), support }
}

fn fake_tools(dir: &Path) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for tool in ["npm", "npx", "pnpm", "yarn", "bun"] {
        let path = bin.join(tool);
        std::fs::write(&path, format!("#!/bin/sh\necho {tool} \"$@\"\npwd -P\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    bin
}

struct Fixture {
    registry: Registry,
    session: Session,
    project: PathBuf,
    data: PathBuf,
    plugins: Vec<&'static str>,
    _dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self::with(&[("nuxt.config.ts", "export default defineNuxtConfig({})\n")], &["nuxt"])
    }

    fn with(files: &[(&str, &str)], plugins: &[&'static str]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("shop");
        std::fs::create_dir_all(&project).unwrap();
        for (name, text) in files {
            std::fs::write(project.join(name), text).unwrap();
        }
        let data = dir.path().join("data");
        let bin = fake_tools(dir.path());
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let mut fixture =
            Self { registry, session: Session::new(), project, data, plugins: plugins.to_vec(), _dir: dir };
        fixture.run("file.project.open", json!({ "path": fixture.project }));
        let project = fixture.project.clone();
        crate::trust::trust_session_in_tests(&mut fixture.session, &project);
        fixture
            .session
            .state_mut::<Runs>()
            .set_providers(vec![Arc::new(NpmRun::in_folders(vec![bin]))], RunEnv::default());
        fixture.reopen();
        fixture
    }

    fn reopen(&mut self) {
        let mut projects = PluginProjects::default();
        projects.set(self.plugins.iter().map(|id| plugin(id)).collect());
        projects.set_data_dir(self.data.clone());
        self.session.set_state(projects);
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn fail(&mut self, id: &str, params: Value) -> String {
        self.registry.dispatch(&mut self.session, id, params).unwrap_err().to_string()
    }

    fn configuration_names(&mut self) -> Vec<String> {
        let listed = self.run("run.configuration.list", json!({}));
        listed["configurations"].as_array().unwrap().iter().map(|c| c["name"].as_str().unwrap().to_owned()).collect()
    }
}

#[test]
fn a_nuxt_project_lists_the_plugin_with_its_configurations_and_setup() {
    let mut fixture = Fixture::new();
    let listed = fixture.run("plugins.project.list", json!({}));
    assert_eq!(listed["plugins"][0]["id"], "nuxt");
    assert_eq!(
        listed["plugins"][0]["runConfigurations"],
        json!(["nuxi dev", "nuxi build", "nuxi generate", "nuxi preview"])
    );
    assert_eq!(listed["plugins"][0]["setup"]["name"], "nuxi prepare");
    assert_eq!(listed["plugins"][0]["setup"]["needed"], true);
    std::fs::create_dir(fixture.project.join(".nuxt")).unwrap();
    let listed = fixture.run("plugins.project.list", json!({}));
    assert_eq!(listed["plugins"][0]["setup"]["needed"], false);
    std::fs::remove_file(fixture.project.join("nuxt.config.ts")).unwrap();
    assert_eq!(fixture.run("plugins.project.list", json!({}))["plugins"], json!([]));
    std::fs::write(fixture.project.join("package.json"), r#"{ "devDependencies": { "nuxt": "^3.13.0" } }"#).unwrap();
    assert_eq!(fixture.run("plugins.project.list", json!({}))["plugins"][0]["id"], "nuxt", "by the dependency");
}

#[test]
fn applying_adds_the_configurations_and_offers_the_setup_once() {
    let mut fixture = Fixture::new();
    let applied = fixture.run("plugins.project.apply", json!({}));
    let added: Vec<&str> = applied["added"].as_array().unwrap().iter().map(|a| a["name"].as_str().unwrap()).collect();
    assert_eq!(added, ["nuxi dev", "nuxi build", "nuxi generate", "nuxi preview"]);
    assert_eq!(applied["offers"][0]["plugin"], "nuxt");
    assert_eq!(applied["offers"][0]["creates"], ".nuxt");
    assert_eq!(fixture.configuration_names(), ["nuxi dev", "nuxi build", "nuxi generate", "nuxi preview"]);
    let saved = std::fs::read_to_string(zen_project::run::file_in(&fixture.project)).unwrap();
    assert!(saved.contains("\"programArguments\": \"generate\""), "{saved}");

    let id = applied["added"][1]["id"].clone();
    fixture.run("run.configuration.delete", json!({ "id": id }));
    assert_eq!(fixture.run("plugins.project.apply", json!({})), json!({ "added": [], "offers": [] }));
    fixture.reopen();
    assert_eq!(fixture.run("plugins.project.apply", json!({})), json!({ "added": [], "offers": [] }));
    assert_eq!(fixture.configuration_names(), ["nuxi dev", "nuxi generate", "nuxi preview"]);
}

#[test]
fn applying_keeps_the_projects_own_configurations_of_the_same_name() {
    let mut fixture = Fixture::new();
    let own = json!({ "name": "nuxi dev", "kind": "npm", "options": { "command": "run", "script": "dev" } });
    fixture.run("run.configuration.create", json!({ "configuration": own }));
    std::fs::create_dir(fixture.project.join(".nuxt")).unwrap();
    let applied = fixture.run("plugins.project.apply", json!({}));
    assert_eq!(applied["added"].as_array().unwrap().len(), 3);
    assert_eq!(applied["offers"], json!([]), "prepared already");
    let listed = fixture.run("run.configuration.list", json!({}));
    assert_eq!(listed["configurations"][0]["options"]["script"], "dev", "the project's own stays");
}

#[test]
fn a_project_the_plugin_does_not_support_gets_nothing() {
    let mut fixture = Fixture::new();
    std::fs::remove_file(fixture.project.join("nuxt.config.ts")).unwrap();
    assert_eq!(fixture.run("plugins.project.apply", json!({})), json!({ "added": [], "offers": [] }));
    assert!(fixture.configuration_names().is_empty());
    let error = fixture.fail("plugins.project.setup", json!({ "plugin": "nuxt" }));
    assert!(error.contains("nuxt"), "{error}");
}

#[cfg(unix)]
#[test]
fn the_setup_runs_nuxi_prepare_with_the_projects_package_manager() {
    let mut fixture = Fixture::new();
    let started = fixture.run("plugins.project.setup", json!({ "waitMs": 30_000 }));
    assert_eq!(started["summary"]["exitCode"], 0, "{started}");
    let output = fixture.run("run.output.get", json!({ "runId": started["runId"], "from": 0 }));
    let lines: Vec<&str> =
        output["lines"].as_array().unwrap().iter().map(|line| line["text"].as_str().unwrap()).collect();
    let folder = fixture.project.canonicalize().unwrap();
    assert_eq!(lines[..3], ["npx --no nuxi prepare", "npx --no nuxi prepare", folder.to_str().unwrap()], "{output}");

    std::fs::write(fixture.project.join("pnpm-lock.yaml"), "").unwrap();
    let started = fixture.run("plugins.project.setup", json!({ "plugin": "nuxt", "waitMs": 30_000 }));
    let output = fixture.run("run.output.get", json!({ "runId": started["runId"], "from": 0 }));
    assert_eq!(output["lines"][1]["text"], "pnpm exec nuxi prepare", "{output}");

    let error = fixture.fail("plugins.project.setup", json!({ "plugin": "vue" }));
    assert!(error.contains("vue"), "{error}");
}

#[test]
fn a_plugins_setup_step_starts_nothing_in_safe_mode() {
    let mut fixture = Fixture::new();
    fixture.run("project.trust.revoke", json!({}));
    let refused = fixture.registry.dispatch(&mut fixture.session, "plugins.project.setup", json!({ "plugin": "nuxt" }));
    match refused {
        Err(zen_core::DispatchError::Failed { error, .. }) => {
            assert_eq!(error.code.as_deref(), Some(crate::trust::NOT_TRUSTED), "{error}");
        }
        other => panic!("not refused: {other:?}"),
    }
    assert_eq!(fixture.run("run.process.list", json!({}))["runs"], json!([]));
}

#[test]
fn a_react_project_gets_the_vite_configurations_only_with_vite() {
    let mut plain = Fixture::with(&[("package.json", r#"{ "dependencies": { "react": "^19" } }"#)], &["react"]);
    let listed = plain.run("plugins.project.list", json!({}));
    assert_eq!(listed["plugins"][0]["id"], "react");
    assert_eq!(listed["plugins"][0]["runConfigurations"], json!([]), "{listed}");
    assert_eq!(plain.run("plugins.project.apply", json!({})), json!({ "added": [], "offers": [] }));

    let vite = r#"{ "dependencies": { "react": "^19" }, "devDependencies": { "vite": "^6" } }"#;
    let mut fixture = Fixture::with(&[("package.json", vite)], &["react"]);
    fixture.run("plugins.project.apply", json!({}));
    assert_eq!(fixture.configuration_names(), ["vite", "vite build", "vite preview"]);
}

#[cfg(unix)]
#[test]
fn a_next_project_gets_its_configurations_which_run_next_with_the_package_manager() {
    let manifest = r#"{ "dependencies": { "next": "15.5.0", "react": "^19" } }"#;
    let mut fixture = Fixture::with(&[("package.json", manifest), ("pnpm-lock.yaml", "")], &["react", "next"]);
    let listed = fixture.run("plugins.project.list", json!({}));
    let ids: Vec<&str> = listed["plugins"].as_array().unwrap().iter().filter_map(|p| p["id"].as_str()).collect();
    assert_eq!(ids, ["react", "next"]);
    let applied = fixture.run("plugins.project.apply", json!({}));
    assert_eq!(applied["offers"], json!([]), "Next.js has no setup step");
    assert_eq!(fixture.configuration_names(), ["next dev", "next build", "next start", "next lint"]);
    let id = applied["added"][0]["id"].clone();
    let started = fixture.run("run.process.start", json!({ "configurationId": id, "waitMs": 30_000 }));
    let output = fixture.run("run.output.get", json!({ "runId": started["runId"], "from": 0 }));
    assert_eq!(output["lines"][1]["text"], "pnpm exec next dev", "{output}");
}

#[cfg(unix)]
#[test]
fn a_sveltekit_project_gets_vite_configurations_and_the_svelte_kit_sync_offer() {
    let manifest = r#"{ "devDependencies": { "@sveltejs/kit": "^2.20.0", "svelte": "^5", "vite": "^6" } }"#;
    let mut fixture =
        Fixture::with(&[("package.json", manifest), ("svelte.config.js", ""), ("pnpm-lock.yaml", "")], &["sveltekit"]);
    let listed = fixture.run("plugins.project.list", json!({}));
    assert_eq!(listed["plugins"][0]["setup"]["name"], "svelte-kit sync", "{listed}");
    assert_eq!(listed["plugins"][0]["setup"]["needed"], true);
    let applied = fixture.run("plugins.project.apply", json!({}));
    assert_eq!(applied["offers"][0]["creates"], ".svelte-kit", "{applied}");
    assert_eq!(fixture.configuration_names(), ["vite dev", "vite build", "vite preview"]);
    assert_eq!(fixture.run("plugins.project.apply", json!({})), json!({ "added": [], "offers": [] }), "once");
    let started = fixture.run("plugins.project.setup", json!({ "plugin": "sveltekit", "waitMs": 30_000 }));
    assert_eq!(started["summary"]["exitCode"], 0, "{started}");
    let output = fixture.run("run.output.get", json!({ "runId": started["runId"], "from": 0 }));
    assert_eq!(output["lines"][1]["text"], "pnpm exec svelte-kit sync", "{output}");
    let id = applied["added"][0]["id"].clone();
    let started = fixture.run("run.process.start", json!({ "configurationId": id, "waitMs": 30_000 }));
    let output = fixture.run("run.output.get", json!({ "runId": started["runId"], "from": 0 }));
    assert_eq!(output["lines"][1]["text"], "pnpm exec vite dev", "{output}");
}

#[test]
fn a_svelte_project_without_kit_is_not_a_sveltekit_project() {
    let manifest = r#"{ "devDependencies": { "svelte": "^5", "vite": "^6" } }"#;
    let mut fixture = Fixture::with(&[("package.json", manifest), ("svelte.config.js", "")], &["sveltekit"]);
    assert_eq!(fixture.run("plugins.project.list", json!({}))["plugins"], json!([]));
}

#[test]
fn a_person_runs_no_setup_of_a_project_not_trusted() {
    let mut fixture = Fixture::new();
    fixture.run("project.trust.revoke", json!({}));
    match fixture.registry.dispatch(&mut fixture.session, "plugins.project.setup", json!({})) {
        Err(zen_core::DispatchError::Failed { error, .. }) => {
            assert_eq!(error.code.as_deref(), Some(crate::trust::NOT_TRUSTED), "{error:?}")
        }
        other => panic!("not refused: {other:?}"),
    }
}
