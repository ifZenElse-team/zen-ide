use std::sync::Arc;

use serde_json::{Value, json};
use zen_core::{CommandError, DispatchError, Registry, Session};
use zen_syntax::{Expansion, Feature, LineFold, OutlineItem, Outliner, RunOffer, Suggestions, TextFeatures};

use super::*;

const COMPOSE: &str = "services:\n  web:\n    image: nginx\n  db:\n    image: postgres\n";

pub(crate) struct Docker;

impl Outliner for Docker {
    fn handles(&self, file_name: &str) -> bool {
        file_name.contains("compose")
    }

    fn outline(&self, _text: &str) -> Vec<OutlineItem> {
        Vec::new()
    }
}

impl TextFeatures for Docker {
    fn supports(&self, feature: Feature) -> bool {
        feature == Feature::RunTargets
    }

    fn folds(&self, _text: &str) -> Vec<LineFold> {
        Vec::new()
    }

    fn complete(&self, _text: &str, _caret: usize) -> Option<Suggestions> {
        None
    }

    fn expand(&self, _text: &str, _caret: usize) -> Option<Expansion> {
        None
    }

    fn run_targets(&self, path: &str, _text: &str) -> Vec<RunOffer> {
        let offer = |line: u32, name: &str, args: &[&str]| RunOffer {
            line,
            name: name.to_owned(),
            id: format!("up:{name}"),
            program: "docker".to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            cwd: None,
            env: vec![("COMPOSE_MENU".to_owned(), "false".to_owned())],
            test: false,
            diff: false,
            document: None,
        };
        assert_eq!(path, "compose.yaml", "the plug-in gets the path relative to the project");
        vec![
            offer(1, "web", &["compose", "-f", "${file}", "up", "web; rm -rf /"]),
            offer(3, "db", &["compose", "-f", "${file}", "up", "db"]),
        ]
    }

    fn plugin_id(&self) -> Option<&str> {
        Some("dockerfile")
    }

    fn runs(&self) -> Vec<String> {
        vec!["docker".to_owned()]
    }

    fn person_only(&self) -> Vec<zen_syntax::PersonOnly> {
        vec![zen_syntax::PersonOnly { program: "docker".to_owned(), subcommands: vec!["rm".to_owned()] }]
    }

    fn tool_windows(&self) -> Vec<zen_syntax::ToolWindowDeclaration> {
        let text = |en: &str| [("en".to_owned(), en.to_owned())].into_iter().collect();
        let action = |id: &str, kind, args: &[&str], rows| zen_syntax::ToolAction {
            id: id.to_owned(),
            title: text(id),
            program: "docker".to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            kind,
            rows,
            confirm: Default::default(),
            inputs: Vec::new(),
        };
        let input = |id: &str, kind, from: &str| zen_syntax::ToolInput {
            id: id.to_owned(),
            title: text(id),
            kind,
            default: None,
            default_from: from.to_owned(),
        };
        let scale = zen_syntax::ToolAction {
            confirm: text("Scale ${rows} to ${input.replicas}?"),
            inputs: vec![input("replicas", zen_syntax::ToolInputKind::Count, "")],
            ..action("scale", Change, &["scale", "${row.id}", "--replicas", "${input.replicas}"], true)
        };
        let forward = zen_syntax::ToolAction {
            inputs: vec![input("ports", zen_syntax::ToolInputKind::Forward, "name")],
            ..action(
                "forward",
                Stream,
                &["port-forward", "--address", "127.0.0.1", "${row.id}", "${input.ports}"],
                true,
            )
        };
        use zen_syntax::ToolActionKind::{Change, Destroy, Stream};
        vec![zen_syntax::ToolWindowDeclaration {
            id: "docker".to_owned(),
            title: text("Docker"),
            icon: r#"<svg viewBox="0 0 16 16"><rect x="2" y="2" width="12" height="12"/></svg>"#.to_owned(),
            side: "right".to_owned(),
            choices: Vec::new(),
            sections: vec![zen_syntax::ToolSection {
                id: "containers".to_owned(),
                title: text("Containers"),
                columns: vec!["name".to_owned(), "state".to_owned()],
                column_titles: [("name".to_owned(), text("Name"))].into_iter().collect(),
                list: zen_syntax::ToolCommand {
                    program: "docker".to_owned(),
                    args: vec!["ps".to_owned(), "-a".to_owned()],
                },
                actions: vec![
                    action("stop", Change, &["stop", "${row.id}"], true),
                    action("rename", Change, &["rename", "${row.id}", "${row.name}"], true),
                    action("remove", Destroy, &["rm", "${row.id}"], true),
                    action("prune", Destroy, &["container", "prune", "-f"], false),
                    action("logs", Stream, &["logs", "${row.id}"], true),
                    action("sneak", Change, &["exec", "${row.id}", "sh"], true),
                    scale,
                    forward,
                ],
                rows: String::new(),
                state: None,
            }],
            window_values: Vec::new(),
        }]
    }

    fn tool_rows(&self, section: &str, output: &str) -> Option<Vec<zen_syntax::ToolRow>> {
        assert_eq!(section, "containers");
        Some(
            output
                .lines()
                .filter_map(|line| {
                    let mut parts = line.split(' ');
                    let (id, name, state) = (parts.next()?, parts.next()?, parts.next()?);
                    Some(zen_syntax::ToolRow {
                        id: id.to_owned(),
                        columns: vec![name.to_owned(), state.to_owned()],
                        state: Some(state.to_owned()),
                    })
                })
                .collect(),
        )
    }
}

pub(crate) struct Fixture {
    pub(crate) registry: Registry,
    pub(crate) session: Session,
    pub(crate) project: PathBuf,
    pub(crate) bin: PathBuf,
    _dir: tempfile::TempDir,
}

pub(crate) fn fixture(trusted: bool) -> Fixture {
    zen_i18n::register(crate::LOCALES);
    set_plugins(Some(Arc::new(|| {
        vec![
            Arc::new(Docker) as Arc<dyn TextFeatures>,
            Arc::new(crate::plugin_tools::choice_tests::Kube),
            Arc::new(crate::plugin_tools::state_tests::Fleet),
        ]
    })));
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let docker = bin.join("docker");
        std::fs::write(
            &docker,
            concat!(
                "#!/bin/sh\n",
                "here=${0%/*}\n",
                "printf '%s|' \"$@\" >> \"$here/calls\"; echo >> \"$here/calls\"\n",
                "if [ -f \"$here/down\" ]; then echo 'Cannot connect to the daemon' >&2; exit 1; fi\n",
                "if [ -f \"$here/slowlist\" ]; then while [ ! -f \"$here/go\" ]; do :; done; fi\n",
                "if [ \"$1\" = ps ]; then echo 'c1 web running'; echo 'c2 db exited'; exit 0; fi\n",
                "if [ -f \"$here/slow\" ]; then while [ ! -f \"$here/go\" ]; do :; done; fi\n",
                "for arg in \"$@\"; do echo \"[$arg]\"; done\necho \"menu=$COMPOSE_MENU\"\n",
            ),
        )
        .unwrap();
        std::fs::set_permissions(&docker, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let project = dir.path().canonicalize().unwrap().join("shop");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("compose.yaml"), COMPOSE).unwrap();
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(crate::build_tools::BuildEnv::default());
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    if trusted {
        crate::trust::trust_session_in_tests(&mut session, &project);
    }
    session
        .state_mut::<crate::run::Runs>()
        .set_providers(vec![Arc::new(PluginRun::in_folders(vec![bin.clone()]))], zen_run::RunEnv::default());
    crate::plugin_tools::set_folders(&mut session, vec![bin.clone()]);
    Fixture { registry, session, project, bin, _dir: dir }
}

impl Fixture {
    pub(crate) fn call(&mut self, id: &str, params: Value) -> Result<Value, DispatchError> {
        self.registry.dispatch(&mut self.session, id, params)
    }

    pub(crate) fn as_agent(&mut self, id: &str, params: Value) -> Result<Value, DispatchError> {
        let Fixture { registry, session, .. } = self;
        zen_core::macros::as_agent(session, |session| registry.dispatch(session, id, params))
    }

    fn start(&self, line: u32) -> Value {
        json!({ "context": { "path": self.project.join("compose.yaml"), "line": line + 1 }, "waitMs": 30000 })
    }

    pub(crate) fn output(&mut self, answer: &Value) -> String {
        let read = self.call("run.output.get", json!({ "runId": answer["runId"] })).unwrap();
        let lines = read["lines"].as_array().unwrap();
        lines.iter().map(|line| line["text"].as_str().unwrap_or_default()).collect::<Vec<_>>().join("\n")
    }
}

pub(crate) fn failure(error: DispatchError) -> CommandError {
    match error {
        DispatchError::Failed { error, .. } => error,
        other => panic!("{other}"),
    }
}

pub(crate) fn code(error: DispatchError) -> String {
    failure(error).code.unwrap_or_default()
}

#[test]
fn a_plug_in_s_targets_are_listed_and_run_as_an_argument_list() {
    let mut fixture = fixture(true);
    let path = fixture.project.join("compose.yaml");
    let targets = fixture.call("run.targets.list", json!({ "path": path })).unwrap();
    let names: Vec<&str> = targets["targets"].as_array().unwrap().iter().filter_map(|t| t["name"].as_str()).collect();
    assert_eq!(names, ["web", "db"], "{targets}");
    let done = fixture.call("run.process.start", fixture.start(1)).unwrap();
    let printed = fixture.output(&done);
    assert!(printed.contains(&format!("[{}]", path.display())), "the file is filled in: {printed}");
    assert!(printed.contains("[web; rm -rf /]"), "one argument, never a shell: {printed}");
    assert!(printed.contains("menu=false"), "the plug-in's own variable: {printed}");
}

#[test]
fn a_program_the_plug_in_did_not_declare_is_refused() {
    let mut fixture = fixture(true);
    let mut configuration = configuration(
        "dockerfile",
        "compose.yaml",
        &RunOffer {
            line: 1,
            name: "x".to_owned(),
            id: "x".to_owned(),
            program: "sh".to_owned(),
            ..RunOffer::default()
        },
    );
    configuration.name = "sneaky".to_owned();
    let refused = fixture.call("run.process.start", json!({ "configuration": configuration, "waitMs": 30000 }));
    let answer = format!("{refused:?}");
    assert!(answer.contains("sh") && answer.contains("dockerfile"), "{answer}");
}

#[test]
fn the_environment_of_a_plug_in_cannot_set_path_or_the_platform_s_variables() {
    for name in ["PATH", "Path", "ZEN_DATA_DIR", "DYLD_INSERT_LIBRARIES", "LD_PRELOAD", "A=B"] {
        assert!(!zen_syntax::kept_variable(name), "{name}");
    }
    assert!(zen_syntax::kept_variable("COMPOSE_MENU"));
    let context = RunContext { root: PathBuf::from("/p"), ..RunContext::default() };
    let mut configuration = configuration(
        "dockerfile",
        "compose.yaml",
        &RunOffer {
            program: "docker".to_owned(),
            env: vec![("PATH".to_owned(), "/evil".to_owned()), ("COMPOSE_MENU".to_owned(), "false".to_owned())],
            ..RunOffer::default()
        },
    );
    configuration.options["plugin"] = json!("dockerfile");
    set_plugins(Some(Arc::new(|| vec![Arc::new(Docker) as Arc<dyn TextFeatures>])));
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("docker"), "").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path().join("docker"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let lines = PluginRun::in_folders(vec![dir.path().to_owned()]).command_lines(&configuration, &context).unwrap();
    let paths: Vec<&str> =
        lines[0].env.iter().filter(|(name, _)| name == "PATH").map(|(_, value)| value.as_str()).collect();
    assert_eq!(paths.len(), 1, "{:?}", lines[0].env);
    assert!(!paths[0].contains("/evil"));
}

#[test]
fn an_untrusted_project_runs_no_plug_in_target() {
    let mut fixture = fixture(false);
    let refused = fixture.call("run.process.start", fixture.start(1)).unwrap_err();
    assert_eq!(code(refused), crate::trust::NOT_TRUSTED);
    let listed = fixture.call("run.targets.list", json!({ "path": fixture.project.join("compose.yaml") })).unwrap();
    assert_eq!(listed["targets"].as_array().map(Vec::len), Some(2), "targets are still listed: {listed}");
}

#[test]
fn agents_are_refused_by_default_asked_when_the_person_says_so_and_cannot_change_it() {
    let mut fixture = fixture(true);
    let start = fixture.start(3);
    let refused = fixture.as_agent("run.process.start", start.clone()).unwrap_err();
    assert_eq!(code(refused), AGENT_NOT_ALLOWED);
    let set = json!({ "plugin": "dockerfile", "access": "full" });
    assert_eq!(code(fixture.as_agent("plugins.runAccess.set", set).unwrap_err()), AGENT_NOT_ALLOWED);
    fixture.call("plugins.runAccess.set", json!({ "plugin": "dockerfile", "access": "ask" })).unwrap();
    let asked = failure(fixture.as_agent("run.process.start", start.clone()).unwrap_err());
    assert_eq!(asked.code.as_deref(), Some(CONFIRMATION_NEEDED), "{asked:?}");
    let token = asked.data.as_ref().and_then(|data| data["confirmation"].as_str()).unwrap().to_owned();
    let mut with_token = start.clone();
    with_token["confirmation"] = json!(token);
    assert_eq!(code(fixture.as_agent("run.process.start", with_token.clone()).unwrap_err()), CONFIRMATION_NEEDED);
    assert_eq!(
        code(fixture.as_agent("plugins.runAccess.allow", json!({ "token": token })).unwrap_err()),
        AGENT_NOT_ALLOWED,
        "the yes is the person's"
    );
    let notifications = fixture.call("ui.notifications.list", json!({})).unwrap();
    assert!(notifications.to_string().contains(AllowAgentRun::ID), "{notifications}");
    fixture.call("plugins.runAccess.allow", json!({ "token": token })).unwrap();
    let started = fixture.as_agent("run.process.start", with_token.clone()).unwrap();
    assert!(fixture.output(&started).contains("[db]"));
    assert_eq!(
        code(fixture.as_agent("run.process.start", with_token).unwrap_err()),
        CONFIRMATION_NEEDED,
        "a yes runs once"
    );
    fixture.call("plugins.runAccess.set", json!({ "plugin": "dockerfile", "access": "full" })).unwrap();
    assert!(fixture.as_agent("run.process.start", start).is_ok());
    assert_eq!(fixture.as_agent("plugins.runAccess.get", json!({})).unwrap()["agents"]["dockerfile"], "full");
}

fn removal() -> Value {
    let configuration = configuration(
        "dockerfile",
        "compose.yaml",
        &RunOffer {
            line: 0,
            name: "remove".to_owned(),
            id: "rm".to_owned(),
            program: "docker".to_owned(),
            args: vec!["compose".to_owned(), "-f".to_owned(), "${file}".to_owned(), "rm".to_owned()],
            ..RunOffer::default()
        },
    );
    json!({ "configuration": configuration, "waitMs": 30000 })
}

#[test]
fn an_agent_never_starts_a_person_only_run_whatever_the_plug_in_s_setting() {
    let mut fixture = fixture(true);
    fixture.call("plugins.runAccess.set", json!({ "plugin": "dockerfile", "access": "full" })).unwrap();
    let refused = failure(fixture.as_agent("run.process.start", removal()).unwrap_err());
    assert_eq!(refused.code.as_deref(), Some(crate::plugin_manager::USER_ONLY), "{refused:?}");
    assert!(refused.message.contains("docker compose -f") && refused.message.contains(" rm"), "{refused:?}");
    let runs = fixture.session.state::<crate::run::Runs>().unwrap();
    assert!(runs.runs().is_empty(), "nothing started");
    assert!(runs.person_question().is_none(), "an agent's start asks nobody");
    let started = fixture.as_agent("run.process.start", fixture.start(3)).unwrap();
    assert!(fixture.output(&started).contains("[db]"), "other targets run as the setting says");
    let mut rewritten = removal();
    rewritten["configuration"]["options"]["args"] = json!(["compose", "--", "rm"]);
    let refused = fixture.as_agent("run.process.start", rewritten).unwrap_err();
    assert_eq!(code(refused), crate::plugin_manager::USER_ONLY, "however an agent writes the command");
}

#[test]
fn a_person_only_run_asks_the_person_with_the_whole_command_line_every_time() {
    let mut fixture = fixture(true);
    let asked = failure(fixture.call("run.process.start", removal()).unwrap_err());
    assert_eq!(asked.code.as_deref(), Some(PERSON_CONFIRMATION), "{asked:?}");
    let question = fixture.session.state::<crate::run::Runs>().unwrap().person_question().cloned().unwrap();
    assert!(question.command_line.ends_with(&format!("{} rm", fixture.project.join("compose.yaml").display())));
    assert!(question.command_line.starts_with("docker compose -f"), "{question:?}");
    assert_eq!(question.folder, fixture.project, "{question:?}");
    let mut wrong = removal();
    wrong["confirmation"] = json!("person-0-0");
    assert_eq!(code(fixture.call("run.process.start", wrong).unwrap_err()), PERSON_CONFIRMATION, "a made-up yes");
    let question = fixture.session.state::<crate::run::Runs>().unwrap().person_question().cloned().unwrap();
    fixture.call("plugins.runAccess.set", json!({ "plugin": "dockerfile", "access": "full" })).unwrap();
    let mut by_agent = removal();
    by_agent["confirmation"] = json!(question.token);
    assert_eq!(
        code(fixture.as_agent("run.process.start", by_agent.clone()).unwrap_err()),
        crate::plugin_manager::USER_ONLY
    );
    let question = fixture.session.state::<crate::run::Runs>().unwrap().person_question().cloned();
    assert!(question.is_none() || question.as_ref().is_some_and(|q| q.token == by_agent["confirmation"]));
    let asked = fixture.call("run.process.start", removal()).unwrap_err();
    assert_eq!(code(asked), PERSON_CONFIRMATION);
    let question = fixture.session.state::<crate::run::Runs>().unwrap().person_question().cloned().unwrap();
    let mut yes = json!({ "configuration": question.configuration, "confirmation": question.token, "waitMs": 30000 });
    let started = fixture.call("run.process.start", yes.clone()).unwrap();
    assert!(fixture.output(&started).contains("[rm]"));
    let runs = fixture.session.state::<crate::run::Runs>().unwrap();
    assert!(runs.is_person_only(started["runId"].as_u64().unwrap()));
    assert!(runs.person_question().is_none(), "the question is answered");
    yes["waitMs"] = json!(0);
    assert_eq!(code(fixture.call("run.process.start", yes).unwrap_err()), PERSON_CONFIRMATION, "a yes runs once");
}

#[test]
fn a_person_only_run_never_starts_past_the_start_command() {
    let fixture = fixture(true);
    let configuration: Configuration = serde_json::from_value(removal()["configuration"].clone()).unwrap();
    let context = RunContext { root: fixture.project.clone(), ..RunContext::default() };
    let mut runs = crate::run::Runs::default();
    runs.set_providers(
        vec![Arc::new(PluginRun::in_folders(vec![fixture._dir.path().join("bin")]))],
        zen_run::RunEnv::default(),
    );
    let refused = runs.start(configuration, &context).unwrap_err();
    assert_eq!(refused.code.as_deref(), Some(crate::plugin_manager::USER_ONLY), "{refused:?}");
}

#[test]
fn a_target_whose_output_is_a_document_opens_it_read_only_outside_the_project() {
    let mut fixture = fixture(true);
    let configuration = configuration(
        "dockerfile",
        "compose.yaml",
        &RunOffer {
            line: 1,
            name: "Containers".to_owned(),
            id: "ps".to_owned(),
            program: "docker".to_owned(),
            args: vec!["ps".to_owned()],
            document: Some("yaml".to_owned()),
            ..RunOffer::default()
        },
    );
    fixture.call("run.process.start", json!({ "configuration": configuration, "waitMs": 30000 })).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let ready = loop {
        let ready = crate::plugin_document::take(&mut fixture.session);
        if !ready.is_empty() || std::time::Instant::now() > deadline {
            break ready;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    assert_eq!(ready.len(), 1, "one document for one run");
    assert_eq!(ready[0].text, "c1 web running\nc2 db exited\n");
    assert_eq!(ready[0].asked.command, "docker ps");
    let path = crate::plugin_document::open(&mut fixture.session, &ready[0]).unwrap();
    assert!(!path.starts_with(&fixture.project), "never a project file: {}", path.display());
    assert_eq!(path.extension().and_then(|extension| extension.to_str()), Some("yaml"));
    let workbench = fixture.session.state::<crate::Workbench>().unwrap();
    let document = workbench.active_document().unwrap();
    assert_eq!(document.path(), path);
    let reason = crate::virtual_documents::reason(&path).unwrap();
    assert!(reason.contains("docker ps") && reason.contains("cannot be edited"), "{reason}");
}
