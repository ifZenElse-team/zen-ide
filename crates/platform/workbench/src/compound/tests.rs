use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use zen_core::{Registry, Session};
use zen_run::fake::{FakeProvider, KIND as FAKE, ShellProvider};
use zen_run::{RunEnv, RunProvider};

use super::*;
use crate::project_settings::ProjectSettingsState;

fn setup() -> (Registry, Session, tempfile::TempDir) {
    zen_i18n::register(crate::LOCALES);
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
    crate::trust::trust_session_in_tests(&mut session, dir.path());
    let providers: Vec<Arc<dyn RunProvider>> = vec![Arc::new(FakeProvider::new()), Arc::new(ShellProvider)];
    session.state_mut::<Runs>().set_providers(providers, RunEnv::default());
    (registry, session, dir)
}

fn create(registry: &Registry, session: &mut Session, configuration: Value) -> String {
    let created = registry.dispatch(session, "run.configuration.create", json!({ "configuration": configuration }));
    created.unwrap()["id"].as_str().unwrap().to_owned()
}

fn shell(name: &str, script: &str) -> Value {
    serde_json::to_value(ShellProvider::configuration(name, script)).unwrap()
}

fn compound(name: &str, members: &[&str]) -> Value {
    json!({ "name": name, "kind": KIND, "options": { "configurations": members } })
}

fn state(registry: &Registry, session: &mut Session, run_id: &Value) -> Value {
    registry.dispatch(session, "run.output.get", json!({ "runId": run_id, "waitMs": 100 })).unwrap()["state"].clone()
}

#[test]
fn the_compound_kind_is_listed_with_the_providers_kinds() {
    let (registry, mut session, _dir) = setup();
    let listed = registry.dispatch(&mut session, "run.configuration.list", json!({})).unwrap();
    let kinds = listed["kinds"].as_array().unwrap();
    let last = kinds.last().unwrap();
    assert_eq!((last["id"].as_str(), last["provider"].as_str()), (Some(KIND), Some(PROVIDER)));
    assert_eq!(last["defaults"], json!({ "configurations": [] }));
    assert_eq!(kinds[0]["id"], FAKE, "the providers' kinds come first");
}

#[cfg(unix)]
#[test]
fn a_compound_starts_every_member_in_a_run_of_its_own_and_stopping_it_stops_them_all() {
    let (registry, mut session, _dir) = setup();
    let server = create(&registry, &mut session, shell("server", "echo server up; sleep 60"));
    let worker = create(&registry, &mut session, shell("worker", "echo worker up; sleep 60"));
    let inner = create(&registry, &mut session, compound("inner", &[&worker]));
    let all = create(&registry, &mut session, compound("all", &[&server, &inner, &server]));

    let started = registry.dispatch(&mut session, "run.process.start", json!({ "configurationId": all })).unwrap();
    let children = started["children"].as_array().unwrap().clone();
    assert_eq!(children.len(), 2, "a member listed twice, or through an inner compound, runs once: {started}");
    assert_eq!(started["runId"], children[0]["runId"]);
    let list = registry.dispatch(&mut session, "run.process.list", json!({})).unwrap();
    let runs = list["runs"].as_array().unwrap();
    let names: Vec<&str> = runs.iter().map(|run| run["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["server", "worker"], "each in a tab of its own");
    assert!(runs.iter().all(|run| run["compound"] == all), "{list}");
    for child in &children {
        assert_eq!(state(&registry, &mut session, &child["runId"]), "running");
    }

    let asked = json!({ "configurationId": all, "confirmRerun": true });
    let error = registry.dispatch(&mut session, "run.process.start", asked).unwrap_err();
    assert!(error.to_string().contains(&t!("workbench-run-already-running", name = "all")), "{error}");

    let stopped = registry.dispatch(&mut session, "run.process.stop", json!({ "configurationId": all })).unwrap();
    assert_eq!(stopped["runs"].as_array().unwrap().len(), 2, "{stopped}");
    let deadline = Instant::now() + Duration::from_secs(20);
    for child in &children {
        while state(&registry, &mut session, &child["runId"]) == "running" {
            assert!(Instant::now() < deadline, "the member does not stop");
        }
        assert_eq!(state(&registry, &mut session, &child["runId"]), "stopped");
    }
}

#[test]
fn a_compound_waits_for_all_its_members_and_answers_each_summary() {
    let (registry, mut session, _dir) = setup();
    let first = create(&registry, &mut session, serde_json::to_value(FakeProvider::configuration("a")).unwrap());
    let second = create(&registry, &mut session, serde_json::to_value(FakeProvider::configuration("b")).unwrap());
    let both = create(&registry, &mut session, compound("both", &[&first, &second]));
    let params = json!({ "configurationId": both, "waitMs": 5000 });
    let started = registry.dispatch(&mut session, "run.process.start", params).unwrap();
    let children = started["children"].as_array().unwrap();
    assert_eq!(children.len(), 2);
    assert!(children.iter().all(|child| child["summary"]["done"] == true), "{started}");
    let missing = create(&registry, &mut session, compound("broken", &["nope"]));
    let error = registry.dispatch(&mut session, "run.process.start", json!({ "configurationId": missing }));
    let message = error.unwrap_err().to_string();
    assert!(message.contains(&t!("workbench-run-compound-missing", name = "broken", id = "nope")), "{message}");
    let empty = create(&registry, &mut session, compound("empty", &[]));
    let error = registry.dispatch(&mut session, "run.process.start", json!({ "configurationId": empty }));
    assert!(error.unwrap_err().to_string().contains(&t!("workbench-run-compound-empty", name = "empty")));
}

#[test]
fn a_compound_that_contains_itself_is_refused() {
    let (registry, mut session, dir) = setup();
    let app = create(&registry, &mut session, serde_json::to_value(FakeProvider::configuration("app")).unwrap());
    let outer = create(&registry, &mut session, compound("outer", &[&app]));
    let inner = create(&registry, &mut session, compound("inner", &[&outer]));
    let mut looped = compound("outer", &[&app, &inner]);
    looped["id"] = json!(outer);
    let error = registry.dispatch(&mut session, "run.configuration.update", json!({ "configuration": looped }));
    let message = error.unwrap_err().to_string();
    assert!(message.contains(&t!("workbench-run-compound-cycle", name = "outer", member = "outer")), "{message}");
    let mut itself = compound("self", &["compound-9"]);
    itself["id"] = json!("compound-9");
    assert!(registry.dispatch(&mut session, "run.configuration.create", json!({ "configuration": itself })).is_err());

    let file = RunConfigurations::read(dir.path()).unwrap();
    assert_eq!(file.configurations.len(), 3, "nothing refused was saved");
    let edited = Configuration {
        id: outer.clone(),
        name: "outer".to_owned(),
        kind: KIND.to_owned(),
        options: json!({ "configurations": [inner] }),
        ..Configuration::default()
    };
    assert_eq!(cycle(&file.configurations, &edited), Some("outer".to_owned()), "as the dialog checks a draft");
    assert_eq!(cycle(&file.configurations, file.get(&inner).unwrap()), None);
}

struct Foreign;

impl zen_project::settings::SettingsProvider for Foreign {
    fn id(&self) -> &str {
        "foreign"
    }

    fn watches(&self, _: &Path) -> bool {
        false
    }

    fn load(
        &self,
        root: &Path,
        _: &dyn zen_project::settings::ReadFiles,
    ) -> Option<zen_project::settings::ProjectSettings> {
        let foreign = |name: &str, type_id: &str, options: &[(&str, &str)]| zen_run::ForeignRunConfiguration {
            source_file: root.join(format!(".run/{name}.run.xml")),
            name: name.to_owned(),
            type_id: type_id.to_owned(),
            options: options.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect(),
            shared: true,
            ..Default::default()
        };
        Some(zen_project::settings::ProjectSettings {
            run_configurations: vec![
                foreign("Serve", zen_run::fake::ADOPTED_TYPE, &[("target", "serve"), ("folderName", "Servers")]),
                foreign(
                    "Everything",
                    FOREIGN_TYPE,
                    &[
                        ("toRun.0.name", "Serve"),
                        ("toRun.0.type", "Whatever"),
                        ("toRun.1.name", "Own"),
                        ("toRun.2.name", "Unknown"),
                    ],
                ),
                foreign("Single", FOREIGN_TYPE, &[("toRun.name", "Own")]),
            ],
            ..Default::default()
        })
    }
}

#[test]
fn adopted_compounds_list_their_members_by_name_and_folders_are_adopted() {
    let (registry, mut session, dir) = setup();
    let own = create(&registry, &mut session, serde_json::to_value(FakeProvider::configuration("Own")).unwrap());
    session.state_mut::<ProjectSettingsState>().set_providers(vec![Arc::new(Foreign)]);
    let listed = registry.dispatch(&mut session, "run.configuration.list", json!({})).unwrap();
    let configurations: Vec<Configuration> = serde_json::from_value(listed["configurations"].clone()).unwrap();
    let serve = configurations.iter().find(|kept| kept.name == "Serve").unwrap();
    assert_eq!(serve.folder, "Servers");
    let everything = configurations.iter().find(|kept| kept.name == "Everything").unwrap();
    assert_eq!(everything.kind, KIND);
    assert_eq!(members(everything), [serve.id.clone(), own.clone()], "a member no one adopted is left out");
    assert_eq!(everything.adopted_from, Some(dir.path().join(".run/Everything.run.xml")));
    let single = configurations.iter().find(|kept| kept.name == "Single").unwrap();
    assert_eq!(members(single), [own]);
    let started = registry.dispatch(&mut session, "run.process.start", json!({ "configurationId": everything.id }));
    assert_eq!(started.unwrap()["children"].as_array().unwrap().len(), 2);
    let file = std::fs::read_to_string(dir.path().join(zen_project::run::FILE)).unwrap();
    assert!(!file.contains("Everything"), "{file}");
}

#[test]
fn members_are_read_and_written_in_the_options() {
    let configuration = Configuration { kind: KIND.to_owned(), ..Configuration::default() };
    assert!(is_compound(&configuration) && members(&configuration).is_empty());
    let mut changed = configuration;
    set_members(&mut changed, &["a".to_owned()]);
    assert_eq!(changed.options, json!({ "configurations": ["a"] }));
}

#[test]
fn a_compound_never_runs_a_saved_configuration_of_a_project_not_trusted() {
    zen_i18n::register(crate::LOCALES);
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
    let providers: Vec<Arc<dyn RunProvider>> = vec![Arc::new(FakeProvider::new()), Arc::new(ShellProvider)];
    session.state_mut::<Runs>().set_providers(providers, RunEnv::default());
    let saved = create(&registry, &mut session, serde_json::to_value(FakeProvider::configuration("saved")).unwrap());
    for command in ["run.process.start", "debug.session.start"] {
        let inline = json!({ "configuration": compound("inline", &[&saved]) });
        let refused = zen_core::macros::as_agent(&mut session, |session| registry.dispatch(session, command, inline));
        match refused {
            Err(zen_core::DispatchError::Failed { error, .. }) => {
                assert_eq!(error.code.as_deref(), Some(crate::trust::NOT_TRUSTED), "{command}: {error:?}")
            }
            other => panic!("{command}: a saved member ran in a project not trusted: {other:?}"),
        }
    }
    let list = registry.dispatch(&mut session, "run.process.list", json!({})).unwrap();
    assert_eq!(list["runs"], json!([]), "{list}");
}
