use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use zen_core::Registry;
use zen_run::fake::FakeProvider;
use zen_run::{
    CommandLine, Configuration, OutputParser, PlainParser, RunContext, RunEnv, RunProvider, RunTarget, Stream,
};

use super::*;
use crate::external_tools::{ExternalTool, ExternalTools};

const SHELL: &str = "shell.task";

struct ShellTasks;

impl RunProvider for ShellTasks {
    fn id(&self) -> &str {
        "shell"
    }

    fn kinds(&self) -> Vec<KindSpec> {
        Vec::new()
    }

    fn targets(&self, _: &RunContext, _: &Path, _: &str) -> Vec<RunTarget> {
        Vec::new()
    }

    fn context(&self, _: &Path, _: &Path, _: &str, _: usize) -> Option<Configuration> {
        None
    }

    fn command_lines(&self, _: &Configuration, _: &RunContext) -> Result<Vec<CommandLine>, String> {
        Ok(Vec::new())
    }

    fn parser(&self, _: &Configuration, _: &RunContext) -> Box<dyn OutputParser> {
        Box::new(PlainParser)
    }

    fn task_kinds(&self) -> Vec<KindSpec> {
        vec![KindSpec {
            id: SHELL.to_owned(),
            name_message: "shell-task".to_owned(),
            fields: vec![FieldSpec {
                key: "script".to_owned(),
                label_message: "shell-task-script".to_owned(),
                kind: FieldKind::Text,
            }],
            defaults: json!({ "script": "" }),
        }]
    }

    fn task_lines(&self, task: &BeforeLaunchTask, context: &RunContext) -> Option<Result<Vec<CommandLine>, String>> {
        (task.kind == SHELL).then(|| {
            let script = task.text("script").to_owned();
            Ok(vec![CommandLine {
                program: "sh".into(),
                args: vec!["-c".to_owned(), script.clone()],
                cwd: context.root.clone(),
                env: Vec::new(),
                display: format!("sh -c {script}"),
            }])
        })
    }

    fn adopt_task(&self, foreign: &ForeignTask, _: &Path) -> Option<BeforeLaunchTask> {
        (foreign.id == "ShellBeforeRunTask")
            .then(|| BeforeLaunchTask::new(SHELL, json!({ "script": foreign.options.get("script") })))
    }
}

struct Fixture {
    registry: Registry,
    session: Session,
    fake: FakeProvider,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        zen_i18n::register(crate::LOCALES);
        zen_i18n::set_language(Some("en"));
        let mut registry = Registry::new();
        crate::register(&mut registry).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new();
        session.set_state(ExternalTools::load(Some(dir.path().join("data/tools.json"))));
        let project = dir.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
        crate::trust::trust_session_in_tests(&mut session, &project);
        let fake = FakeProvider::new();
        fake.set_script(&[(Stream::Stdout, "the program ran")], 0);
        let providers: Vec<Arc<dyn RunProvider>> = vec![Arc::new(fake.clone()), Arc::new(ShellTasks)];
        session.state_mut::<Runs>().set_providers(providers, RunEnv::default());
        Self { registry, session, fake, dir }
    }

    fn run(&mut self, id: &str, params: Value) -> Value {
        self.registry.dispatch(&mut self.session, id, params).unwrap_or_else(|error| panic!("{id}: {error}"))
    }

    fn fail(&mut self, id: &str, params: Value) -> String {
        self.registry.dispatch(&mut self.session, id, params).unwrap_err().to_string()
    }

    fn create(&mut self, name: &str, tasks: Value) -> String {
        let mut configuration = serde_json::to_value(FakeProvider::configuration(name)).unwrap();
        configuration["common"]["beforeLaunch"] = tasks;
        let created = self.run("run.configuration.create", json!({ "configuration": configuration }));
        created["id"].as_str().unwrap().to_owned()
    }

    fn start(&mut self, id: &str) -> Value {
        self.run("run.process.start", json!({ "configurationId": id, "waitMs": 20000 }))
    }

    fn console(&mut self, run_id: &Value) -> Vec<String> {
        let output = self.run("run.output.get", json!({ "runId": run_id }));
        output["lines"].as_array().unwrap().iter().map(|line| line["text"].as_str().unwrap().to_owned()).collect()
    }
}

fn shell(script: &str) -> Value {
    json!({ "type": SHELL, "script": script })
}

fn position(lines: &[String], text: &str) -> usize {
    lines.iter().position(|line| line == text).unwrap_or_else(|| panic!("no line {text:?} in {lines:#?}"))
}

#[cfg(unix)]
#[test]
fn tasks_run_in_order_in_the_runs_console_before_its_program() {
    let mut fixture = Fixture::new();
    let id = fixture.create("app", json!([shell("echo one"), shell("echo two")]));
    let started = fixture.start(&id);
    assert_eq!(started["summary"]["state"], "finished");
    assert_eq!(started["summary"]["exitCode"], 0);
    let lines = fixture.console(&started["runId"]);
    let first = position(&lines, "#before-launch shell-task “echo one”");
    let one = position(&lines, "one");
    let second = position(&lines, "#before-launch shell-task “echo two”");
    let two = position(&lines, "two");
    let program = position(&lines, "the program ran");
    assert!(first < one && one < second && second < two && two < program, "{lines:#?}");
}

#[cfg(unix)]
#[test]
fn a_failing_task_cancels_the_start_and_the_run_ends_before_launch_failed() {
    let mut fixture = Fixture::new();
    let id = fixture.create("app", json!([shell("echo broken; exit 2"), shell("echo never")]));
    let started = fixture.start(&id);
    let summary = &started["summary"];
    assert_eq!(summary["state"], "beforeLaunchFailed");
    assert_eq!(summary["exitCode"], 2);
    assert!(summary["error"].as_str().unwrap().contains("exit code 2"), "{summary}");
    let lines = fixture.console(&started["runId"]);
    assert!(lines.contains(&"broken".to_owned()), "{lines:#?}");
    assert!(!lines.contains(&"never".to_owned()), "{lines:#?}");
    assert!(!lines.contains(&"the program ran".to_owned()), "{lines:#?}");
    let missing = fixture.create("missing", json!([shell("exit 0"), { "type": "nosuch.task" }]));
    let started = fixture.start(&missing);
    assert_eq!(started["summary"]["state"], "finished", "an unknown type is skipped with a line");
    let lines = fixture.console(&started["runId"]);
    assert!(
        lines.iter().any(|line| line.starts_with("#before-launch nosuch.task (unknown task) skipped")),
        "{lines:#?}"
    );
}

#[cfg(unix)]
#[test]
fn run_another_configuration_waits_for_its_end_and_a_cycle_fails() {
    let mut fixture = Fixture::new();
    fixture.create("build", json!([shell("echo building")]));
    let app = fixture.create("app", json!([{ "type": RUN_CONFIGURATION, "name": "build" }, shell("echo after")]));
    let started = fixture.start(&app);
    assert_eq!(started["summary"]["state"], "finished");
    let lines = fixture.console(&started["runId"]);
    let done = position(&lines, "#before-launch Run Another Configuration “build” finished");
    assert!(done < position(&lines, "after"), "{lines:#?}");
    let runs = fixture.run("run.process.list", json!({}));
    let names: Vec<&str> = runs["runs"].as_array().unwrap().iter().map(|run| run["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["app", "build"], "the other configuration runs in a tab of its own");

    fixture.fake.set_script(&[(Stream::Stdout, "the program ran")], 1);
    let started = fixture.start(&app);
    assert_eq!(started["summary"]["state"], "beforeLaunchFailed", "the other one ended with exit code 1");

    fixture.fake.set_script(&[], 0);
    let first = fixture.create("first", json!([{ "type": RUN_CONFIGURATION, "name": "second" }]));
    fixture.create("second", json!([{ "type": RUN_CONFIGURATION, "name": "first" }]));
    let started = fixture.start(&first);
    assert_eq!(started["summary"]["state"], "beforeLaunchFailed");
    let lost = fixture.create("lost", json!([{ "type": RUN_CONFIGURATION, "name": "nosuch" }]));
    let started = fixture.start(&lost);
    let error = started["summary"]["error"].as_str().unwrap().to_owned();
    assert!(error.contains("there is no run configuration “nosuch”"), "{error}");
}

#[cfg(unix)]
#[test]
fn run_external_tool_runs_the_tool_in_its_tab_and_waits() {
    let mut fixture = Fixture::new();
    let tool = ExternalTool {
        name: "Generate".to_owned(),
        program: "sh".to_owned(),
        arguments: "-c \"echo generated\"".to_owned(),
        ..ExternalTool::default()
    };
    let saved = fixture.run("run.externalTools.save", json!({ "tool": tool }));
    let app = fixture.create("app", json!([{ "type": EXTERNAL_TOOL, "tool": saved["id"] }]));
    let listed = fixture.run("run.beforeLaunch.list", json!({ "configurationId": app }));
    assert_eq!(listed["tasks"][0]["label"], "Run External Tool “Generate”", "the label names the tool");
    let started = fixture.start(&app);
    assert_eq!(started["summary"]["state"], "finished");
    let runs = fixture.run("run.process.list", json!({}));
    let tool_run = runs["runs"].as_array().unwrap().iter().find(|run| run["name"] == "Generate").unwrap().clone();
    assert_eq!(tool_run["exitCode"], 0);
    assert!(fixture.console(&tool_run["runId"]).contains(&"generated".to_owned()));
    let by_name = fixture.create("by name", json!([{ "type": EXTERNAL_TOOL, "tool": "Generate" }]));
    assert_eq!(fixture.start(&by_name)["summary"]["state"], "finished");
    let lost = fixture.create("lost", json!([{ "type": EXTERNAL_TOOL, "tool": "nosuch" }]));
    assert_eq!(fixture.start(&lost)["summary"]["state"], "beforeLaunchFailed");
}

#[cfg(unix)]
#[test]
fn activate_tool_window_asks_the_window_in_its_turn() {
    let mut fixture = Fixture::new();
    let release = fixture.dir.path().join("release");
    let script = format!("until [ -e '{}' ]; do sleep 0.01; done", release.display());
    let app = fixture.create("app", json!([shell(&script), { "type": TOOL_WINDOW, "toolWindow": "terminal" }]));
    let started = fixture.run("run.process.start", json!({ "configurationId": app }));
    assert!(take_tool_windows(&mut fixture.session).is_empty(), "not before the task before it has ended");
    std::fs::write(&release, "").unwrap();
    let run_id = started["runId"].as_u64().unwrap();
    let summary = crate::run::wait_for_run(&mut fixture.session, run_id, Duration::from_secs(20)).unwrap();
    assert_eq!(summary.state, RunState::Finished);
    assert_eq!(take_tool_windows(&mut fixture.session), ["terminal"]);
    let wrong = fixture.create("wrong", json!([{ "type": TOOL_WINDOW, "toolWindow": "nosuch" }]));
    assert_eq!(fixture.start(&wrong)["summary"]["state"], "beforeLaunchFailed");
}

#[test]
fn save_files_saves_the_modified_documents() {
    let mut fixture = Fixture::new();
    let file = fixture.dir.path().join("project/notes.txt");
    std::fs::write(&file, "old\n").unwrap();
    fixture.run("editor.text.write", json!({ "path": file, "text": "new\n" }));
    let app = fixture.create("app", json!([{ "type": SAVE_FILES }]));
    let started = fixture.start(&app);
    assert_eq!(started["summary"]["state"], "finished");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "new\n");
    assert!(fixture.console(&started["runId"]).contains(&"#before-launch Save Files".to_owned()));
}

#[cfg(unix)]
#[test]
fn stopping_during_the_tasks_stops_the_task_and_never_starts_the_program() {
    let mut fixture = Fixture::new();
    let app = fixture.create("app", json!([shell("echo waiting; exec sleep 30")]));
    let started = fixture.run("run.process.start", json!({ "configurationId": app }));
    let run_id = started["runId"].as_u64().unwrap();
    let output = fixture.session.state::<Runs>().unwrap().run(run_id).unwrap().output.clone();
    output.wait(2, Duration::from_secs(20));
    let stopped = fixture.run("run.process.stop", json!({ "runId": run_id }));
    assert_eq!(stopped["stopped"], run_id);
    let summary = crate::run::wait_for_run(&mut fixture.session, run_id, Duration::from_secs(20)).unwrap();
    assert_eq!(summary.state, RunState::Stopped);
    assert!(!advance(&mut fixture.session), "the launch is dropped");
    assert!(!fixture.console(&started["runId"]).contains(&"the program ran".to_owned()));
}

#[test]
fn commands_add_edit_move_and_remove_tasks_in_the_file() {
    let mut fixture = Fixture::new();
    let app = fixture.create("app", json!([]));
    let types = fixture.run("run.beforeLaunch.types", json!({}));
    let ids: Vec<&str> = types["types"].as_array().unwrap().iter().map(|kind| kind["id"].as_str().unwrap()).collect();
    assert_eq!(ids, [RUN_CONFIGURATION, EXTERNAL_TOOL, TOOL_WINDOW, SAVE_FILES, SHELL]);
    assert_eq!(types["types"][4]["provider"], "shell");
    assert_eq!(types["types"][0]["name"], "Run Another Configuration");
    assert!(types["types"][0].get("provider").is_none());

    fixture.run("run.beforeLaunch.add", json!({ "configurationId": app, "task": shell("echo a") }));
    fixture.run("run.beforeLaunch.add", json!({ "configurationId": app, "task": { "type": SAVE_FILES }, "index": 0 }));
    let list = fixture.run("run.beforeLaunch.add", json!({ "configurationId": app, "task": shell("echo c") }));
    let labels = |list: &Value| -> Vec<String> {
        list["tasks"].as_array().unwrap().iter().map(|task| task["label"].as_str().unwrap().to_owned()).collect()
    };
    assert_eq!(labels(&list), ["Save Files", "shell-task “echo a”", "shell-task “echo c”"]);
    let list = fixture.run("run.beforeLaunch.move", json!({ "configurationId": app, "index": 0, "up": false }));
    assert_eq!(labels(&list)[..2], ["shell-task “echo a”", "Save Files"]);
    let list = fixture.run("run.beforeLaunch.move", json!({ "configurationId": app, "index": 0, "up": true }));
    assert_eq!(labels(&list)[0], "shell-task “echo a”", "the first stays first");
    let task = shell("echo b");
    let list = fixture.run("run.beforeLaunch.update", json!({ "configurationId": app, "index": 2, "task": task }));
    assert_eq!(list["tasks"][2]["script"], "echo b");
    let list = fixture.run("run.beforeLaunch.remove", json!({ "configurationId": app, "index": 1 }));
    assert_eq!(labels(&list), ["shell-task “echo a”", "shell-task “echo b”"]);
    let file = zen_run::RunConfigurations::read(&fixture.dir.path().join("project")).unwrap();
    assert_eq!(file.configurations[0].common.before_launch[1].text("script"), "echo b");

    let out = fixture.fail("run.beforeLaunch.remove", json!({ "configurationId": app, "index": 5 }));
    assert!(out.contains("has 2"), "{out}");
    let unknown = fixture.fail("run.beforeLaunch.add", json!({ "configurationId": app, "task": { "type": "nosuch" } }));
    assert!(unknown.contains("runConfiguration, externalTool"), "{unknown}");

    assert_eq!(fixture.run("run.beforeLaunch.page", json!({ "configurationId": app }))["show"], Value::Null);
    let list = fixture.run("run.beforeLaunch.showPage", json!({ "configurationId": app, "show": true }));
    assert_eq!(list["showPage"], true);
    fixture.run("run.configuration.select", json!({ "id": app }));
    assert_eq!(fixture.run("run.beforeLaunch.page", json!({}))["show"], app.as_str());
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
        let task = |id: &str, enabled: bool, options: &[(&str, &str)]| ForeignTask {
            id: id.to_owned(),
            enabled,
            options: options.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect(),
        };
        Some(zen_project::settings::ProjectSettings {
            run_configurations: vec![zen_run::ForeignRunConfiguration {
                source_file: root.join(".run/Serve.run.xml"),
                name: "Serve".to_owned(),
                type_id: zen_run::fake::ADOPTED_TYPE.to_owned(),
                options: [("target".to_owned(), "serve".to_owned())].into_iter().collect(),
                shared: true,
                before_launch: vec![
                    task("RunConfigurationTask", true, &[("run_configuration_name", "Build")]),
                    task("ToolBeforeRunTask", true, &[("actionId", "Tool_External Tools_Generate")]),
                    task("Make", false, &[]),
                    task("ShellBeforeRunTask", true, &[("script", "echo adopted")]),
                    task("BuildArtifacts", true, &[]),
                ],
                ..Default::default()
            }],
            ..Default::default()
        })
    }
}

#[test]
fn adopted_files_map_the_known_tasks_and_list_the_others_as_skipped() {
    let mut fixture = Fixture::new();
    let tool = ExternalTool { name: "Generate".to_owned(), program: "true".to_owned(), ..ExternalTool::default() };
    let saved = fixture.run("run.externalTools.save", json!({ "tool": tool }));
    let settings = fixture.session.state_mut::<crate::project_settings::ProjectSettingsState>();
    settings.set_providers(vec![Arc::new(Foreign)]);
    let listed = fixture.run("run.configuration.list", json!({}));
    let adopted = listed["configurations"][0].clone();
    assert_eq!(
        adopted["common"]["beforeLaunch"],
        json!([
            { "type": RUN_CONFIGURATION, "name": "Build" },
            { "type": EXTERNAL_TOOL, "tool": saved["id"] },
            { "type": SHELL, "script": "echo adopted" },
            { "type": SKIPPED, "foreignId": "BuildArtifacts" },
        ])
    );
    let tasks = fixture.run("run.beforeLaunch.list", json!({ "configurationId": adopted["id"] }));
    assert_eq!(tasks["tasks"][3]["label"], "BuildArtifacts (not supported)");
    let refused = fixture.fail("run.beforeLaunch.remove", json!({ "configurationId": adopted["id"], "index": 0 }));
    assert!(refused.contains("Serve"), "{refused}");
}
