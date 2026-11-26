use std::time::Instant;

use serde_json::{Value, json};

use super::*;
use crate::run::Runs;

fn english() {
    zen_i18n::register(crate::LOCALES);
    zen_i18n::set_language(Some("en"));
}

struct Fixture {
    registry: Registry,
    session: Session,
    dir: tempfile::TempDir,
    project: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let mut fixture = Self::untrusted();
        crate::trust::trust_session_in_tests(&mut fixture.session, &fixture.project);
        fixture
    }

    fn untrusted() -> Self {
        english();
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let project = std::fs::canonicalize(dir.path()).unwrap().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let mut session = Session::new();
        session.set_state(GlobalWatchers::load(Some(dir.path().join("data").join(FILE_NAME))));
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        Self { registry, session, dir, project }
    }

    fn run(&mut self, command: &str, params: Value) -> Result<Value, String> {
        self.registry.dispatch(&mut self.session, command, params).map_err(|error| match error {
            zen_core::DispatchError::Failed { error, .. } => error.to_string(),
            other => other.to_string(),
        })
    }

    fn as_agent(&mut self, command: &str, params: Value) -> Result<Value, String> {
        let Self { registry, session, .. } = self;
        zen_core::macros::as_agent(session, |session| {
            registry.dispatch(session, command, params).map_err(|error| match error {
                zen_core::DispatchError::Failed { error, .. } => {
                    format!("{}: {}", error.code.as_deref().unwrap_or(""), error.message)
                }
                other => other.to_string(),
            })
        })
    }

    fn log(&self) -> PathBuf {
        self.dir.path().join("runs.log")
    }

    fn logged(&self) -> Vec<String> {
        std::fs::read_to_string(self.log()).unwrap_or_default().lines().map(str::to_owned).collect()
    }

    fn recorder(&self, name: &str) -> FileWatcher {
        let script = self.dir.path().join("record.sh");
        std::fs::write(&script, "echo \"$1\" >> \"$LOG\"\n").unwrap();
        FileWatcher {
            name: name.to_owned(),
            program: "/bin/sh".to_owned(),
            arguments: format!("{} $FileName$", script.display()),
            environment: vec![EnvVar { name: "LOG".to_owned(), value: self.log().display().to_string() }],
            ..FileWatcher::default()
        }
    }

    fn save(&mut self, watcher: &FileWatcher, level: Level) -> String {
        let saved = self.run("run.fileWatchers.save", json!({ "watcher": watcher, "level": level })).unwrap();
        saved["id"].as_str().unwrap().to_owned()
    }

    fn file(&self, name: &str, text: &str) -> PathBuf {
        let path = self.project.join(name);
        std::fs::write(&path, text).unwrap();
        path
    }

    fn engine(&self) -> &engine::Engine {
        self.session.state::<engine::Engine>().unwrap()
    }

    fn wait_runs(&mut self) {
        let outputs: Vec<zen_run::SharedOutput> = {
            let runs = self.session.state::<Runs>();
            self.engine()
                .active_runs()
                .into_iter()
                .filter_map(|id| runs.and_then(|runs| runs.run(id)).map(|run| run.output.clone()))
                .collect()
        };
        for output in outputs {
            output.wait_end(Duration::from_secs(20));
        }
    }
}

#[test]
fn watchers_are_kept_per_level_and_edited_by_commands() {
    let mut fixture = Fixture::new();
    let watcher = fixture.recorder("Record");
    let first = fixture.save(&watcher, Level::Project);
    let second = fixture.save(&watcher, Level::Project);
    let global = fixture.save(&FileWatcher { name: "Lint All".to_owned(), ..watcher.clone() }, Level::Global);
    assert_eq!((first.as_str(), second.as_str(), global.as_str()), ("record", "record-2", "lint-all"));
    let saved: Value = serde_json::from_slice(&std::fs::read(fixture.project.join(PROJECT_FILE)).unwrap()).unwrap();
    assert_eq!(saved["formatVersion"], 1);
    assert_eq!(saved["watchers"].as_array().unwrap().len(), 2);
    assert!(saved["watchers"][0].get("level").is_none(), "the level is where it is kept");
    assert!(fixture.dir.path().join("data").join(FILE_NAME).exists());

    let listed = fixture.run("run.fileWatchers.list", json!({})).unwrap();
    let rows: Vec<(String, String)> = listed["watchers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row["level"].as_str().unwrap().to_owned(), row["id"].as_str().unwrap().to_owned()))
        .collect();
    let expected = [("project", "record"), ("project", "record-2"), ("global", "lint-all")];
    assert_eq!(rows, expected.map(|(level, id)| (level.to_owned(), id.to_owned())));
    let moved = fixture.run("run.fileWatchers.move", json!({ "id": second, "level": "project", "up": true })).unwrap();
    assert_eq!(moved["moved"], true);
    let again = fixture.run("run.fileWatchers.move", json!({ "id": second, "level": "project", "up": true })).unwrap();
    assert_eq!(again["moved"], false, "first of its level");
    let copy = fixture.run("run.fileWatchers.copy", json!({ "id": global, "level": "global" })).unwrap();
    assert_eq!(copy["id"], "lint-all-copy");
    fixture.run("run.fileWatchers.remove", json!({ "id": first, "level": "project" })).unwrap();
    fixture.run("run.fileWatchers.enable", json!({ "id": second, "level": "project", "enabled": false })).unwrap();
    let listed = fixture.run("run.fileWatchers.list", json!({})).unwrap();
    assert_eq!(listed["watchers"].as_array().unwrap().len(), 3);
    assert_eq!(listed["watchers"][0]["enabled"], false);

    let no_program = FileWatcher { program: " ".to_owned(), ..watcher.clone() };
    let error = fixture.run("run.fileWatchers.save", json!({ "watcher": no_program })).unwrap_err();
    assert!(error.contains("needs a program"), "{error}");
    let bad = FileWatcher { output_filters: vec!["$LINE$ only".to_owned()], ..watcher.clone() };
    assert!(fixture.run("run.fileWatchers.save", json!({ "watcher": bad })).is_err());

    let all = fixture.run("run.fileWatchers.list", json!({})).unwrap()["watchers"].clone();
    let kept: Vec<Value> = all.as_array().unwrap().iter().filter(|row| row["level"] == "global").cloned().collect();
    let replaced = fixture.run("run.fileWatchers.replace", json!({ "watchers": kept })).unwrap();
    assert_eq!(replaced["watchers"].as_array().unwrap().len(), 2, "OK keeps only what the table holds");
    let saved: Value = serde_json::from_slice(&std::fs::read(fixture.project.join(PROJECT_FILE)).unwrap()).unwrap();
    assert_eq!(saved["watchers"], json!([]));
}

#[test]
fn import_adds_what_export_wrote_with_free_ids() {
    let mut fixture = Fixture::new();
    let watcher = fixture.recorder("Record");
    fixture.save(&watcher, Level::Project);
    let file = fixture.dir.path().join("watchers.json");
    let exported = fixture.run("run.fileWatchers.export", json!({ "path": file })).unwrap();
    assert_eq!(exported["count"], 1);
    let imported = fixture.run("run.fileWatchers.import", json!({ "path": file, "level": "project" })).unwrap();
    assert_eq!(imported["ids"], json!(["record-2"]));
    let imported = fixture.run("run.fileWatchers.import", json!({ "path": file, "level": "global" })).unwrap();
    assert_eq!(imported["ids"], json!(["record"]));
    std::fs::write(&file, "{ broken").unwrap();
    let error = fixture.run("run.fileWatchers.import", json!({ "path": file })).unwrap_err();
    assert!(error.contains("Cannot import"), "{error}");
}

#[test]
fn templates_come_from_modules_and_the_web_set_by_name() {
    let mut fixture = Fixture::new();
    fixture.session.state_mut::<Templates>().declare(
        "go",
        r#"[{ "name": "go fmt", "fileType": "go", "program": "$GoExecPath$", "arguments": "fmt $FilePath$" }]"#,
    );
    let listed = fixture.run("run.fileWatchers.templates", json!({})).unwrap();
    let names: Vec<(&str, &str)> = listed
        .as_array()
        .unwrap()
        .iter()
        .map(|template| (template["origin"].as_str().unwrap(), template["watcher"]["name"].as_str().unwrap()))
        .collect();
    assert_eq!(names, [("go", "go fmt"), ("platform", "Prettier"), ("platform", "SCSS"), ("platform", "TypeScript")]);
    for template in listed.as_array().unwrap() {
        let watcher: FileWatcher = serde_json::from_value(template["watcher"].clone()).unwrap();
        assert!(watcher.validate().is_ok(), "{}", watcher.name);
        assert!(watcher.id.is_empty());
    }
}

#[test]
fn a_save_runs_matching_watchers_after_the_pause_and_one_at_a_time() {
    let mut fixture = Fixture::new();
    let watcher = FileWatcher { auto_save: false, ..fixture.recorder("Record") };
    fixture.save(&watcher, Level::Project);
    let a = fixture.file("a.txt", "one\n");
    let b = fixture.file("b.txt", "two\n");
    fixture.run("file.document.open", json!({ "path": a })).unwrap();
    let start = Instant::now();
    step(&mut fixture.session, start);
    fixture.run("editor.edit.insert", json!({ "text": "x" })).unwrap();
    step(&mut fixture.session, start);
    assert_eq!(fixture.engine().pending_files(), std::slice::from_ref(&a), "an edit waits");
    let later = start + AUTO_SAVE_DELAY + DEBOUNCE;
    step(&mut fixture.session, later);
    assert!(fixture.engine().active_runs().is_empty(), "no auto-save, no run");

    fixture.run("file.document.save", json!({})).unwrap();
    assert_eq!(step(&mut fixture.session, later), Some(DEBOUNCE));
    step(&mut fixture.session, later + DEBOUNCE / 2);
    assert!(fixture.engine().active_runs().is_empty(), "debounced");
    files_changed(&mut fixture.session, std::slice::from_ref(&a), later + DEBOUNCE / 2);
    step(&mut fixture.session, later + DEBOUNCE);
    assert_eq!(fixture.engine().active_runs().len(), 1);
    fixture.wait_runs();
    step(&mut fixture.session, later + DEBOUNCE * 2);
    assert_eq!(fixture.logged(), ["a.txt"], "the disk's echo of the save is no second run");

    let asked = fixture.run("run.fileWatchers.run", json!({ "paths": [a, b] })).unwrap();
    assert_eq!(asked["queued"], 2);
    assert_eq!(fixture.engine().active_runs().len(), 1, "one run per watcher at a time");
    assert_eq!(fixture.engine().queued_files(), std::slice::from_ref(&b));
    let waited = fixture.run("run.fileWatchers.run", json!({ "paths": [b], "waitMs": 20000 })).unwrap();
    assert_eq!(waited["done"], true);
    assert_eq!(fixture.logged(), ["a.txt", "a.txt", "b.txt"]);
    let tasks = fixture.run("tasks.background.list", json!({})).unwrap();
    assert!(tasks.to_string().contains("Record: "), "each run is a background task: {tasks}");
}

#[test]
fn an_edit_is_saved_after_a_pause_when_auto_save_is_on() {
    let mut fixture = Fixture::new();
    let watcher = fixture.recorder("Record");
    fixture.save(&watcher, Level::Project);
    let a = fixture.file("a.txt", "one\n");
    fixture.run("file.document.open", json!({ "path": a })).unwrap();
    let start = Instant::now();
    step(&mut fixture.session, start);
    fixture.run("editor.edit.insert", json!({ "text": "x" })).unwrap();
    step(&mut fixture.session, start);
    step(&mut fixture.session, start + AUTO_SAVE_DELAY);
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "xone\n");
    assert_eq!(fixture.engine().active_runs().len(), 1);
    fixture.wait_runs();
    step(&mut fixture.session, start + AUTO_SAVE_DELAY + DEBOUNCE);
    assert_eq!(fixture.logged(), ["a.txt"]);
}

#[test]
fn external_changes_follow_the_trigger_option_and_the_scope() {
    let mut fixture = Fixture::new();
    let watcher = FileWatcher { trigger_on_external: false, ..fixture.recorder("Saves only") };
    fixture.save(&watcher, Level::Project);
    let outside = fixture.dir.path().join("outside.txt");
    std::fs::write(&outside, "x").unwrap();
    let typed = FileWatcher { file_type: "nothing-has-it".to_owned(), ..fixture.recorder("Typed") };
    fixture.save(&typed, Level::Global);
    let any = fixture.recorder("Any");
    fixture.save(&any, Level::Global);
    let a = fixture.file("a.txt", "one\n");
    let start = Instant::now();
    let project = fixture.project.clone();
    files_changed(&mut fixture.session, &[a.clone(), outside.clone(), project], start);
    step(&mut fixture.session, start + DEBOUNCE);
    assert_eq!(
        fixture.engine().active_names(),
        ["Any"],
        "neither the outside file, the folder, the other type nor a save-only watcher"
    );
    fixture.wait_runs();
    let open = FileWatcher { scope: WatchScope::OpenFiles, ..fixture.recorder("Open") };
    fixture.save(&open, Level::Project);
    let asked = fixture.run("run.fileWatchers.run", json!({ "paths": [outside], "id": "open" })).unwrap();
    assert_eq!(asked["queued"], 0, "not open");
    fixture.run("file.document.open", json!({ "path": outside })).unwrap();
    let asked =
        fixture.run("run.fileWatchers.run", json!({ "paths": [outside], "id": "open", "waitMs": 20000 })).unwrap();
    assert_eq!(asked["queued"], 1, "the Open Files scope takes an open file outside the project");
    assert_eq!(fixture.logged(), ["a.txt", "outside.txt"]);
}

#[test]
fn a_failing_watcher_is_disabled_with_a_notification_that_enables_it_again() {
    let mut fixture = Fixture::new();
    let missing = FileWatcher {
        name: "Missing".to_owned(),
        program: fixture.dir.path().join("no-such-program").display().to_string(),
        ..FileWatcher::default()
    };
    let id = fixture.save(&missing, Level::Project);
    let failing = FileWatcher {
        name: "Exit 3".to_owned(),
        program: "/bin/sh".to_owned(),
        arguments: "-c 'exit 3'".to_owned(),
        ..FileWatcher::default()
    };
    fixture.save(&failing, Level::Project);
    let a = fixture.file("a.txt", "one\n");
    let answer = fixture.run("run.fileWatchers.run", json!({ "paths": [a], "waitMs": 20000 })).unwrap();
    let finished = answer["finished"].as_array().unwrap();
    assert_eq!(finished.len(), 2, "{answer}");
    assert!(finished.iter().all(|run| run["disabled"].is_string()), "{answer}");
    assert!(finished.iter().any(|run| run["disabled"].as_str().unwrap().contains("exit code 3")), "{answer}");
    let listed = fixture.run("run.fileWatchers.list", json!({})).unwrap();
    assert!(listed["watchers"].as_array().unwrap().iter().all(|row| row["enabled"] == false));
    let notifications = fixture.run("ui.notifications.list", json!({})).unwrap();
    let text = notifications.to_string();
    assert!(text.contains("File watcher disabled") && text.contains("run.fileWatchers.enable"), "{text}");
    let first = notifications["notifications"]
        .as_array()
        .unwrap()
        .iter()
        .find(|notification| notification["body"].as_str().unwrap().contains("Missing"))
        .unwrap()
        .clone();
    let action = &first["actions"][0];
    assert_eq!(action["label"], "Enable again");
    fixture.run(action["command"].as_str().unwrap(), action["params"].clone()).unwrap();
    assert!(find(&mut fixture.session, Level::Project, &id).unwrap().enabled);
}

#[test]
fn a_cancelled_watcher_run_stops_its_program_and_stays_enabled() {
    let mut fixture = Fixture::new();
    let hanging = FileWatcher {
        name: "Hanging".to_owned(),
        program: "/bin/sh".to_owned(),
        arguments: "-c 'sleep 60; sleep 60'".to_owned(),
        ..FileWatcher::default()
    };
    fixture.save(&hanging, Level::Project);
    let a = fixture.file("a.txt", "one\n");
    fixture.run("run.fileWatchers.run", json!({ "paths": [a] })).unwrap();
    fixture.run("tasks.background.cancel", json!({})).unwrap();
    fixture.wait_runs();
    step(&mut fixture.session, Instant::now());
    let tasks = fixture.run("tasks.background.list", json!({})).unwrap();
    assert_eq!(tasks["tasks"].as_array().unwrap().last().unwrap()["state"], "cancelled", "{tasks}");
    let listed = fixture.run("run.fileWatchers.list", json!({})).unwrap();
    assert_eq!(listed["watchers"][0]["enabled"], true, "{listed}");
}

#[test]
fn filter_matches_become_problems_and_stdout_can_become_the_output_file() {
    let mut fixture = Fixture::new();
    let a = fixture.file("a.txt", "one\ntwo words\n");
    let lint = FileWatcher {
        name: "Lint".to_owned(),
        program: "/bin/sh".to_owned(),
        arguments: "-c 'echo \"$0:2:5: a problem here\"; exit 1' $FileName$".to_owned(),
        working_directory: "$FileDir$".to_owned(),
        output_filters: vec!["^$FILE_PATH$:$LINE$:$COLUMN$: $MESSAGE$".to_owned()],
        ..FileWatcher::default()
    };
    fixture.save(&lint, Level::Project);
    let answer = fixture.run("run.fileWatchers.run", json!({ "paths": [a], "waitMs": 20000 })).unwrap();
    assert_eq!(answer["finished"][0]["problems"], 1, "{answer}");
    assert!(answer["finished"][0].get("disabled").is_none(), "findings are no failure");
    let found = problems(&fixture.session, &a);
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].line, found[0].column, found[0].start), (2, 5, 8));
    assert_eq!((found[0].message.as_str(), found[0].source.as_deref()), ("a problem here", Some("Lint")));
    let listed = fixture.run("problems.project.list", json!({})).unwrap();
    assert!(listed.to_string().contains("a problem here"), "{listed}");

    fixture.run("run.fileWatchers.remove", json!({ "id": "lint" })).unwrap();
    let copy = FileWatcher {
        name: "Upper".to_owned(),
        program: "/bin/sh".to_owned(),
        arguments: "-c 'tr a-z A-Z < \"$0\"' $FileName$".to_owned(),
        working_directory: "$FileDir$".to_owned(),
        output_paths: "$FileNameWithoutExtension$.out".to_owned(),
        create_output_from_stdout: true,
        ..FileWatcher::default()
    };
    fixture.save(&copy, Level::Project);
    fixture.run("run.fileWatchers.run", json!({ "paths": [a], "waitMs": 20000 })).unwrap();
    assert_eq!(std::fs::read_to_string(fixture.project.join("a.out")).unwrap(), "ONE\nTWO WORDS\n");
}

#[test]
fn the_project_watchers_run_only_once_the_project_is_trusted() {
    let mut fixture = Fixture::untrusted();
    fixture.session.set_state(crate::build_tools::BuildEnv::default());
    let project = fixture.save(&fixture.recorder("Project"), Level::Project);
    let a = fixture.file("a.txt", "one\n");
    files_changed(&mut fixture.session, std::slice::from_ref(&a), Instant::now());
    step(&mut fixture.session, Instant::now() + DEBOUNCE * 2);
    assert!(fixture.engine().active_runs().is_empty(), "a project's own watcher waits for trust");
    let refused = fixture.run("run.fileWatchers.run", json!({ "paths": [a], "id": project })).unwrap_err();
    assert!(refused.contains("not trusted"), "{refused}");
    let notices = fixture.run("ui.notifications.list", json!({})).unwrap().to_string();
    assert!(notices.contains("Trust Project"), "the person is asked: {notices}");

    fixture.save(&fixture.recorder("Global"), Level::Global);
    let waited = fixture.run("run.fileWatchers.run", json!({ "paths": [a], "waitMs": 20000 })).unwrap();
    assert_eq!(waited["queued"], 1, "the person's global watchers still run");
    assert_eq!(fixture.logged(), ["a.txt"]);

    crate::trust::trust_session_in_tests(&mut fixture.session, &fixture.project);
    let waited = fixture.run("run.fileWatchers.run", json!({ "paths": [a], "id": project, "waitMs": 20000 })).unwrap();
    assert_eq!(waited["done"], true);
    assert_eq!(fixture.logged(), ["a.txt", "a.txt"]);
}

#[test]
fn agents_cannot_set_up_or_switch_on_a_watcher_but_can_run_and_switch_one_off() {
    let mut fixture = Fixture::new();
    let watcher = fixture.recorder("Record");
    let id = fixture.save(&watcher, Level::Project);
    let exported = fixture.dir.path().join("exported.json");
    fixture.run("run.fileWatchers.export", json!({ "path": exported })).unwrap();
    let attempts = [
        ("run.fileWatchers.save", json!({ "watcher": watcher })),
        ("run.fileWatchers.save", json!({ "watcher": watcher, "level": "global" })),
        ("run.fileWatchers.replace", json!({ "watchers": [{ "level": "project", "watcher": watcher }] })),
        ("run.fileWatchers.copy", json!({ "id": id })),
        ("run.fileWatchers.import", json!({ "path": exported })),
        ("run.fileWatchers.enable", json!({ "id": id })),
    ];
    for (command, params) in attempts {
        let refused = fixture.as_agent(command, params).unwrap_err();
        assert!(refused.starts_with("agentNotAllowed"), "{command}: {refused}");
    }
    assert_eq!(fixture.run("run.fileWatchers.list", json!({})).unwrap()["watchers"].as_array().unwrap().len(), 1);

    let a = fixture.file("a.txt", "one\n");
    let waited = fixture.as_agent("run.fileWatchers.run", json!({ "paths": [a], "waitMs": 20000 })).unwrap();
    assert_eq!(waited["done"], true, "an agent runs the person's watchers");
    fixture.as_agent("run.fileWatchers.enable", json!({ "id": id, "enabled": false })).unwrap();
    assert!(!find(&mut fixture.session, Level::Project, &id).unwrap().enabled);

    crate::notifications::post(
        &mut fixture.session,
        crate::notifications::NewNotification::message(
            crate::notifications::Group::General,
            crate::notifications::Kind::Error,
            "failed".to_owned(),
        )
        .action(crate::notifications::Action::new(
            "enable",
            "Enable again".to_owned(),
            EnableWatcher::ID,
            json!({ "id": id, "level": "project" }),
        )),
    );
    let notifications = fixture.run("ui.notifications.list", json!({})).unwrap();
    let posted = notifications["notifications"][0]["id"].clone();
    let refused = fixture.as_agent("ui.notifications.action", json!({ "id": posted, "action": "enable" })).unwrap_err();
    assert!(refused.starts_with("agentNotAllowed"), "{refused}");
}

#[test]
fn agents_cannot_run_watchers_on_or_export_into_hidden_files() {
    let mut fixture = Fixture::new();
    fixture.save(&fixture.recorder("Record"), Level::Project);
    let secret = fixture.file(".env", "TOKEN=1\n");
    fixture.session.state_mut::<crate::hidden_files::HiddenFromAgents>().set(&[".env".to_owned()]);
    let refused = fixture.as_agent("run.fileWatchers.run", json!({ "paths": [secret], "waitMs": 20000 })).unwrap_err();
    assert!(refused.starts_with("redacted"), "{refused}");
    let refused = fixture.as_agent("run.fileWatchers.export", json!({ "path": secret })).unwrap_err();
    assert!(refused.starts_with("redacted"), "{refused}");
    assert_eq!(std::fs::read_to_string(&secret).unwrap(), "TOKEN=1\n");
    assert!(fixture.logged().is_empty());
    let waited = fixture.run("run.fileWatchers.run", json!({ "paths": [secret], "waitMs": 20000 })).unwrap();
    assert_eq!(waited["done"], true, "the person's own run is not limited");
}

#[cfg(unix)]
#[test]
fn run_file_watchers_before_launch_runs_them_on_the_files_first_and_a_failure_cancels_the_start() {
    use zen_run::fake::FakeProvider;
    let mut fixture = Fixture::new();
    let fake = FakeProvider::new();
    fake.set_script(&[(zen_run::Stream::Stdout, "the program ran")], 0);
    fixture.session.state_mut::<Runs>().set_providers(vec![Arc::new(fake)], zen_run::RunEnv::default());
    crate::before_launch::register_type(&mut fixture.session, task_type());
    let record = fixture.recorder("Record");
    let record = fixture.save(&record, Level::Project);
    fixture.file("a.txt", "a");
    fixture.file("b.txt", "b");
    std::fs::create_dir_all(fixture.project.join(".idea")).unwrap();
    fixture.file(".idea/workspace.txt", "skipped");

    let types = fixture.run("run.beforeLaunch.types", json!({})).unwrap();
    let listed = types["types"].as_array().unwrap().iter().find(|kind| kind["id"] == TASK).unwrap().clone();
    assert_eq!(listed["name"], "Run File Watchers");
    assert_eq!(listed["fields"][0]["kind"]["type"], "fileWatcher");

    let mut configuration = serde_json::to_value(FakeProvider::configuration("app")).unwrap();
    configuration["common"]["beforeLaunch"] = json!([{ "type": TASK, "watcher": "", "files": "all" }]);
    let created = fixture.run("run.configuration.create", json!({ "configuration": configuration })).unwrap();
    let id = created["id"].as_str().unwrap().to_owned();
    let tasks = fixture.run("run.beforeLaunch.list", json!({ "configurationId": id })).unwrap();
    assert_eq!(tasks["tasks"][0]["label"], "Run File Watchers “All files”");

    assert!(fixture.session.state::<Workbench>().is_some_and(|workbench| workbench.index.is_some()));
    let start = json!({ "configurationId": id, "waitMs": 20_000 });
    let started = fixture.run("run.process.start", start.clone()).unwrap();
    assert_eq!(started["summary"]["state"], "finished", "{started}");
    assert_eq!(fixture.logged(), ["a.txt", "b.txt"], "the watcher ran on each file before the start");
    let console = fixture.run("run.output.get", json!({ "runId": started["runId"] })).unwrap().to_string();
    let done = console.find("#before-launch Run File Watchers “All files” finished").expect(&console);
    assert!(done < console.find("the program ran").expect(&console), "{console}");

    let failing = FileWatcher {
        name: "Broken".to_owned(),
        program: "/bin/sh".to_owned(),
        arguments: "-c \"exit 4\"".to_owned(),
        ..FileWatcher::default()
    };
    fixture.save(&failing, Level::Project);
    let task = json!({ "type": TASK, "watcher": format!("project:{record}"), "files": "all" });
    fixture.run("run.beforeLaunch.update", json!({ "configurationId": id, "index": 0, "task": task })).unwrap();
    let tasks = fixture.run("run.beforeLaunch.list", json!({ "configurationId": id })).unwrap();
    assert_eq!(tasks["tasks"][0]["label"], "Run File Watchers “Record”");
    let started = fixture.run("run.process.start", start.clone()).unwrap();
    assert_eq!(started["summary"]["state"], "finished", "only the chosen watcher runs: {started}");

    let task = json!({ "type": TASK, "files": "all" });
    fixture.run("run.beforeLaunch.update", json!({ "configurationId": id, "index": 0, "task": task })).unwrap();
    let started = fixture.run("run.process.start", start.clone()).unwrap();
    assert_eq!(started["summary"]["state"], "beforeLaunchFailed", "{started}");
    let error = started["summary"]["error"].as_str().unwrap();
    assert!(error.contains("“Broken” failed on"), "{error}");
    let console = fixture.run("run.output.get", json!({ "runId": started["runId"] })).unwrap().to_string();
    assert!(!console.contains("the program ran"), "{console}");

    let task = json!({ "type": TASK, "watcher": "project:nosuch" });
    fixture.run("run.beforeLaunch.update", json!({ "configurationId": id, "index": 0, "task": task })).unwrap();
    let started = fixture.run("run.process.start", start).unwrap();
    assert!(started["summary"]["error"].as_str().unwrap().contains("no file watcher “project:nosuch”"), "{started}");
}
