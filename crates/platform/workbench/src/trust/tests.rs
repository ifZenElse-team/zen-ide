use serde_json::{Value, json};
use zen_core::DispatchError;

use super::*;
use crate::notifications::Notifications;

struct Fixture {
    project: PathBuf,
    data: PathBuf,
    registry: Registry,
    session: Session,
    _dir: tempfile::TempDir,
}

fn fixture() -> Fixture {
    zen_i18n::register(crate::LOCALES);
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("shop");
    std::fs::create_dir_all(project.join("module")).unwrap();
    std::fs::write(project.join("pom.xml"), "<project/>").unwrap();
    let data = dir.path().join("data");
    let env = BuildEnv::default();
    env.set_data_dir(Some(data.clone()));
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(env);
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    Fixture { _dir: dir, project, data, registry, session }
}

impl Fixture {
    fn run(&mut self, command: &str, params: Value) -> Result<Value, DispatchError> {
        self.registry.dispatch(&mut self.session, command, params)
    }

    fn as_agent(&mut self, command: &str, params: Value) -> Result<Value, DispatchError> {
        let registry = &self.registry;
        zen_core::macros::as_agent(&mut self.session, |session| registry.dispatch(session, command, params))
    }

    fn trusted(&mut self) -> bool {
        self.run("project.trust.state", json!({})).unwrap()["trusted"] == true
    }
}

fn code(answer: Result<Value, DispatchError>) -> String {
    match answer {
        Err(DispatchError::Failed { error, .. }) => error.code.unwrap_or_default(),
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
fn a_project_with_a_build_is_untrusted_until_the_person_trusts_it() {
    let mut f = fixture();
    assert!(!f.trusted());
    let offered: Vec<Value> = f
        .session
        .state::<Notifications>()
        .unwrap()
        .newest_first()
        .map(|item| serde_json::to_value(item).unwrap())
        .collect();
    let prompt = offered.iter().find(|item| item["actions"][0]["command"] == GrantTrust::ID).expect("the prompt");
    assert!(prompt["title"].as_str().unwrap().contains("shop"), "{prompt}");

    f.run("project.trust.grant", json!({})).unwrap();
    assert!(f.trusted());
    let inside = f.run("project.trust.state", json!({ "path": "module" })).unwrap();
    assert_eq!(inside["trusted"], true, "folders inside a trusted project are trusted");
    let kept: String = std::fs::read_dir(&f.data)
        .unwrap()
        .filter_map(Result::ok)
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .collect();
    assert!(kept.contains(TOOL) && kept.contains("shop"), "{kept}");
    let in_project = std::fs::read_dir(&f.project).unwrap().count();
    assert_eq!(in_project, 2, "nothing is written into the project: pom.xml and module only");
}

#[test]
fn agents_never_trust_a_project_directly_or_through_the_prompt() {
    let mut f = fixture();
    assert_eq!(code(f.as_agent("project.trust.grant", json!({}))), AGENT_NOT_ALLOWED);
    let prompt = f
        .session
        .state::<Notifications>()
        .unwrap()
        .newest_first()
        .find(|item| item.actions.iter().any(|action| action.command == GrantTrust::ID))
        .map(|item| item.id)
        .unwrap();
    let pressed = f.as_agent("ui.notifications.action", json!({ "id": prompt, "action": "trust" }));
    assert_eq!(code(pressed), AGENT_NOT_ALLOWED);
    assert!(f.session.state_mut::<Notifications>().take_queued().is_empty(), "nothing queued for the window");
    let posted = f
        .as_agent(
            "ui.notifications.post",
            json!({ "body": "x", "actions": [{ "id": "t", "label": "T", "command": GrantTrust::ID, "params": {} }] }),
        )
        .ok()
        .and_then(|posted| posted["id"].as_u64());
    if let Some(id) = posted {
        assert_eq!(code(f.as_agent("ui.notifications.action", json!({ "id": id, "action": "t" }))), AGENT_NOT_ALLOWED);
    }
    assert!(!f.trusted());
}

#[test]
fn agents_may_not_run_the_build_of_an_untrusted_project_and_may_revoke_trust() {
    let mut f = fixture();
    let refused = zen_core::macros::as_agent(&mut f.session, |session| guard_agent_project(session));
    assert_eq!(refused.unwrap_err().code.as_deref(), Some(NOT_TRUSTED));
    assert!(guard_agent_project(&f.session).is_ok(), "the person runs it");
    f.run("project.trust.grant", json!({})).unwrap();
    assert!(zen_core::macros::as_agent(&mut f.session, |session| guard_agent_project(session)).is_ok());
    let revoked = f.as_agent("project.trust.revoke", json!({})).unwrap();
    assert_eq!(revoked["trusted"], false);
    assert!(!f.trusted());
}

#[test]
fn an_untrusted_java_project_starts_without_lombok_processors_or_gradle_import() {
    let f = fixture();
    let env = f.session.state::<BuildEnv>().cloned().unwrap();
    assert_eq!(crate::java::processing::launch(&env, &f.project), crate::java::processing::JavaLaunch::default());
    let options = json!({ "settings": { "java": { "import": { "gradle": { "enabled": true }, "maven": { "enabled": true } } } } });
    let safe = crate::java::processing::without_build_scripts(options);
    assert_eq!(safe["settings"]["java"]["import"]["gradle"]["enabled"], false);
    assert_eq!(safe["settings"]["java"]["import"]["maven"]["enabled"], true);
}

#[test]
fn without_a_data_folder_a_project_is_trusted_only_for_this_run() {
    let env = BuildEnv::default();
    let dir = tempfile::tempdir().unwrap();
    assert!(!is_trusted(&env, dir.path()));
    trust_in_tests(&env, dir.path());
    assert!(is_trusted(&env, &dir.path().join("inside")));
    assert!(!is_trusted(&BuildEnv::default(), dir.path()));
}

#[test]
fn safe_mode_runs_and_debugs_nothing_and_says_why_for_the_person_and_for_agents() {
    let mut f = fixture();
    let root = f.project.clone();
    let saved = zen_run::Configuration {
        name: "Deploy".to_owned(),
        kind: "fake".to_owned(),
        temporary: false,
        ..zen_run::Configuration::default()
    };
    let at_the_caret = zen_run::Configuration { temporary: true, ..saved.clone() };
    for configuration in [&saved, &at_the_caret] {
        let refused = guard_run(&mut f.session, &root, configuration).unwrap_err();
        assert_eq!(refused.code.as_deref(), Some(NOT_TRUSTED));
        assert!(refused.message.contains("Deploy") && refused.message.contains("safe mode"), "{}", refused.message);
    }
    let by_agent = zen_core::macros::as_agent(&mut f.session, |session| guard_run(session, &root, &saved));
    assert_eq!(by_agent.unwrap_err().code.as_deref(), Some(NOT_TRUSTED));
    f.run("project.trust.grant", json!({})).unwrap();
    assert!(guard_run(&mut f.session, &root, &saved).is_ok());
    assert!(guard_run(&mut f.session, &root, &at_the_caret).is_ok());
}

#[test]
fn a_task_the_person_starts_runs_in_safe_mode_unless_a_program_runs_before_it() {
    let mut f = fixture();
    let root = f.project.clone();
    let task = zen_run::Configuration { name: "Clean".to_owned(), kind: "fake".to_owned(), ..Default::default() };
    assert!(guard_configuration(&mut f.session, &root, &task).is_ok());
    let mut quiet = task.clone();
    quiet
        .common
        .before_launch
        .push(zen_run::BeforeLaunchTask { kind: crate::before_launch::SAVE_FILES.to_owned(), ..Default::default() });
    assert!(guard_configuration(&mut f.session, &root, &quiet).is_ok(), "saving files runs nothing");
    let mut with_tool = task.clone();
    with_tool
        .common
        .before_launch
        .push(zen_run::BeforeLaunchTask { kind: crate::before_launch::EXTERNAL_TOOL.to_owned(), ..Default::default() });
    let refused = guard_configuration(&mut f.session, &root, &with_tool).unwrap_err();
    assert_eq!(refused.code.as_deref(), Some(NOT_TRUSTED));
    f.run("project.trust.grant", json!({})).unwrap();
    assert!(guard_configuration(&mut f.session, &root, &with_tool).is_ok());
}

#[test]
fn a_trusted_location_trusts_the_projects_under_it_and_removing_it_asks_again() {
    let mut f = fixture();
    let parent = f.project.parent().unwrap().to_owned();
    let listed = f.run("project.trustedLocations.set", json!({ "locations": [parent] })).unwrap();
    assert_eq!(listed["locations"][0]["path"], json!(canonical(&parent)));
    assert_eq!(f.run("project.trustedLocations.get", json!({})).unwrap(), listed);
    assert!(f.trusted());
    let kept = std::fs::read_to_string(f.data.join(crate::build_tools::SETTINGS_FILE)).unwrap();
    assert!(kept.contains(LOCATIONS), "beside the trusted projects: {kept}");
    assert!(kept.contains(TOOL) || !kept.contains("\"true\""), "{kept}");
    f.run("project.trustedLocations.set", json!({ "locations": [] })).unwrap();
    assert!(!f.trusted());
}

#[test]
fn the_list_of_locations_keeps_its_order_once_each() {
    let mut f = fixture();
    let (first, second) = (f.project.clone(), f.project.join("module"));
    let list = |f: &mut Fixture, locations: Value| f.run("project.trustedLocations.set", locations).unwrap();
    let both = list(&mut f, json!({ "locations": [second, first, second] }));
    let paths: Vec<&Value> = both["locations"].as_array().unwrap().iter().map(|row| &row["path"]).collect();
    assert_eq!(paths, [&json!(canonical(&second)), &json!(canonical(&first))]);
    let swapped = list(&mut f, json!({ "locations": [first, second] }));
    assert_eq!(swapped["locations"][0]["path"], json!(canonical(&first)));
}

#[test]
fn agents_may_remove_and_reorder_locations_but_never_add_one() {
    let mut f = fixture();
    let (first, second) = (f.project.clone(), f.project.join("module"));
    let added = f.as_agent("project.trustedLocations.set", json!({ "locations": [first] }));
    assert_eq!(code(added), AGENT_NOT_ALLOWED);
    assert!(!f.trusted());
    f.run("project.trustedLocations.set", json!({ "locations": [first, second] })).unwrap();
    f.as_agent("project.trustedLocations.set", json!({ "locations": [second, first] })).unwrap();
    let swapped = f.as_agent("project.trustedLocations.set", json!({ "locations": [second] })).unwrap();
    assert_eq!(swapped["locations"].as_array().unwrap().len(), 1);
    let back = f.as_agent("project.trustedLocations.set", json!({ "locations": [second, first] }));
    assert_eq!(code(back), AGENT_NOT_ALLOWED, "what was removed is a new folder again");
}

#[test]
fn only_real_absolute_folders_become_locations() {
    let mut f = fixture();
    assert!(f.run("project.trustedLocations.set", json!({ "locations": ["relative"] })).is_err());
    let missing = f.project.join("no-such-folder");
    assert!(f.run("project.trustedLocations.set", json!({ "locations": [missing] })).is_err());
}

#[test]
fn a_location_that_is_home_or_above_it_is_marked() {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap();
    assert!(covers_home(&home));
    assert!(covers_home(home.parent().unwrap()));
    assert!(!covers_home(&home.join("zen-no-such-folder")));
}

#[test]
fn a_project_moved_elsewhere_asks_again() {
    let mut f = fixture();
    f.run("project.trust.grant", json!({})).unwrap();
    let moved = f.project.with_file_name("moved");
    std::fs::rename(&f.project, &moved).unwrap();
    let answer = f.run("project.trust.state", json!({ "path": moved })).unwrap();
    assert_eq!(answer["trusted"], false);
}

#[cfg(unix)]
#[test]
fn a_project_reached_through_a_link_is_the_same_project() {
    let mut f = fixture();
    f.run("project.trust.grant", json!({})).unwrap();
    let link = f.project.with_file_name("link");
    std::os::unix::fs::symlink(&f.project, &link).unwrap();
    assert_eq!(f.run("project.trust.state", json!({ "path": link })).unwrap()["trusted"], true);
}

#[test]
fn trust_changed_by_another_ide_on_the_same_data_folder_is_seen() {
    let f = fixture();
    let env = f.session.state::<BuildEnv>().cloned().unwrap();
    assert!(!is_trusted(&env, &f.project));
    let other = BuildEnv::default();
    other.set_data_dir(Some(f.data.clone()));
    trust_in_tests(&other, &f.project);
    assert!(is_trusted(&env, &f.project));
}

#[test]
fn a_project_the_person_answered_for_is_not_asked_again_by_a_notification() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
    zen_i18n::register(crate::LOCALES);
    let mut registry = Registry::new();
    crate::register(&mut registry).unwrap();
    let mut session = Session::new();
    session.set_state(BuildEnv::default());
    session.set_state(Answered);
    registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
    assert_eq!(session.state::<Notifications>().map_or(0, |store| store.newest_first().count()), 0);
}

#[test]
fn a_program_of_a_project_formats_nothing_in_safe_mode() {
    let f = fixture();
    assert!(crate::code_quality::formatter_for(&f.session, &f.project.join("a.ts"), false).is_none());
}

#[test]
fn artifacts_of_an_untrusted_project_wait_and_the_person_is_asked_once() {
    let mut f = fixture();
    f.session.state_mut::<Notifications>().mark_read(None);
    let root = f.project.clone();
    let refused = guard_project_build(&mut f.session, &root).unwrap_err();
    assert_eq!(refused.code.as_deref(), Some(NOT_TRUSTED));
    assert!(guard_project_build(&mut f.session, &root).is_err());
    let asking = f
        .session
        .state::<Notifications>()
        .unwrap()
        .newest_first()
        .filter(|item| !item.read && item.actions.iter().any(|action| action.command == GrantTrust::ID))
        .count();
    assert_eq!(asking, 1, "one prompt while it is unread");
    f.run("project.trust.grant", json!({})).unwrap();
    assert!(guard_project_build(&mut f.session, &root).is_ok());
}

fn with_fake_runs(f: &mut Fixture) -> zen_run::fake::FakeProvider {
    let fake = zen_run::fake::FakeProvider::new();
    f.session
        .state_mut::<crate::run::Runs>()
        .set_providers(vec![std::sync::Arc::new(fake.clone())], zen_run::RunEnv::default());
    fake
}

fn scratch_in_data(f: &mut Fixture, fake: &zen_run::fake::FakeProvider) -> (PathBuf, zen_run::Configuration) {
    f.session.state_mut::<crate::scratch::Scratches>().set_data_dir(f.data.clone());
    let folder = f.data.join("scratches");
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join("scratch.fake");
    std::fs::write(&file, "main\n").unwrap();
    let configuration = zen_run::Configuration {
        options: json!({ "target": file }),
        ..zen_run::fake::FakeProvider::configuration("scratch.fake")
    };
    fake.set_single_file(vec![zen_run::RunTarget {
        line: 0,
        kind: zen_run::TargetKind::Main,
        name: "scratch.fake".to_owned(),
        package: String::new(),
        configuration: configuration.clone(),
    }]);
    (file, configuration)
}

#[test]
fn a_jvm_scratch_runs_in_safe_mode_only_without_the_projects_classpath() {
    let mut f = fixture();
    let provider: std::sync::Arc<dyn zen_run::RunProvider> =
        std::sync::Arc::new(crate::java_run::JavaRun::default().with_env(crate::java_run::tests::kotlin_reader()));
    f.session.state_mut::<crate::run::Runs>().set_providers(vec![provider.clone()], zen_run::RunEnv::default());
    f.session.state_mut::<crate::scratch::Scratches>().set_data_dir(f.data.clone());
    let folder = f.data.join("scratches");
    std::fs::create_dir_all(&folder).unwrap();
    let root = f.project.clone();
    for (name, text) in [
        ("scratch.java", "public class scratch {\n    public static void main(String[] args) {\n    }\n}\n"),
        ("scratch.kt", "fun main() {\n}\n"),
    ] {
        let file = folder.join(name);
        std::fs::write(&file, text).unwrap();
        let own = provider.single_file(&file, text).remove(0).configuration;
        assert!(guard_run(&mut f.session, &root, &own).is_ok(), "{name}");
        let mut with_classes = own.clone();
        with_classes.options["classpath"] = json!(true);
        let refused = guard_run(&mut f.session, &root, &with_classes).unwrap_err();
        assert_eq!(refused.code.as_deref(), Some(NOT_TRUSTED), "{name}: the classpath option builds the project");
    }
}

#[test]
fn a_scratch_file_runs_in_safe_mode_in_its_own_folder_and_nothing_else_does() {
    let mut f = fixture();
    let fake = with_fake_runs(&mut f);
    let (file, own) = scratch_in_data(&mut f, &fake);
    let started =
        f.run("file.scratch.run", json!({ "path": file, "waitMs": 5000 })).expect("a scratch runs in safe mode");
    let run_id = started["runId"].as_u64().unwrap();
    let run = f.session.state::<crate::run::Runs>().and_then(|runs| runs.run(run_id)).unwrap();
    let folder = file.parent().unwrap();
    assert_eq!(run.command_lines[0].cwd, folder, "the project's toolchain files never choose its program");

    let mut tampered = own.clone();
    tampered.common.env.push(("GOFLAGS".to_owned(), "-toolexec=evil".to_owned()));
    let mut with_a_task = own.clone();
    with_a_task
        .common
        .before_launch
        .push(zen_run::BeforeLaunchTask { kind: crate::before_launch::EXTERNAL_TOOL.to_owned(), ..Default::default() });
    let mut elsewhere = own.clone();
    elsewhere.options = json!({ "target": f.project.join("pom.xml") });
    for configuration in [tampered, with_a_task, elsewhere] {
        assert_eq!(
            code(f.run("run.process.start", json!({ "configuration": configuration }))),
            NOT_TRUSTED,
            "{configuration:?}"
        );
    }
    assert_eq!(
        code(f.run("debug.session.start", json!({ "configuration": own }))),
        NOT_TRUSTED,
        "debug has no exception"
    );
}

#[test]
fn a_rerun_and_a_rerun_of_failed_tests_wait_for_trust_too() {
    let mut f = fixture();
    let fake = with_fake_runs(&mut f);
    fake.set_script(
        &[(zen_run::Stream::Stdout, "@start app TestA"), (zen_run::Stream::Stdout, "@fail app TestA 4")],
        1,
    );
    f.run("project.trust.grant", json!({})).unwrap();
    let configuration = zen_run::fake::FakeProvider::configuration("Tests");
    let started = f.run("run.process.start", json!({ "configuration": configuration, "waitMs": 5000 })).unwrap();
    let run_id = started["runId"].clone();
    f.run("project.trust.revoke", json!({})).unwrap();
    assert_eq!(code(f.run("run.process.rerun", json!({ "runId": run_id }))), NOT_TRUSTED);
    assert_eq!(code(f.run("run.tests.rerunFailed", json!({ "runId": run_id }))), NOT_TRUSTED);
}

#[test]
fn a_refusal_tells_an_agent_what_the_person_does_and_a_person_is_asked() {
    let mut f = fixture();
    let root = f.project.clone();
    let configuration = zen_run::fake::FakeProvider::configuration("Tests");
    let refused = zen_core::macros::as_agent(&mut f.session, |session| guard_run(session, &root, &configuration));
    assert!(refused.unwrap_err().message.contains(&t!("workbench-trust-agent-how")));
    let refused = zen_core::macros::as_agent(&mut f.session, |session| guard_agent_project(session));
    assert!(refused.unwrap_err().message.contains(&t!("workbench-trust-agent-how")));
    let refused = guard_run(&mut f.session, &root, &configuration).unwrap_err();
    assert!(!refused.message.contains(&t!("workbench-trust-agent-how")), "{}", refused.message);
}

#[test]
fn trust_given_is_seen_at_once_after_a_read() {
    let f = fixture();
    let env = f.session.state::<BuildEnv>().cloned().unwrap();
    assert!(!is_trusted(&env, &f.project));
    trust_in_tests(&env, &f.project);
    assert!(is_trusted(&env, &f.project), "the settings read before the write are not kept");
}

#[test]
fn git_runs_at_the_root_of_a_repository_that_holds_a_trusted_folder_and_nowhere_else() {
    let env = BuildEnv::default();
    let dir = tempfile::tempdir().unwrap();
    let repository = dir.path().canonicalize().unwrap();
    for folder in ["services/shop", "services/other"] {
        std::fs::create_dir_all(repository.join(folder)).unwrap();
    }
    assert!(!git_allowed(&env, &repository));
    trust_in_tests(&env, &repository.join("services/shop"));
    assert!(git_allowed(&env, &repository.join("services/shop")));
    assert!(git_allowed(&env, &repository), "the root of the repository the trusted project is in");
    assert!(!git_allowed(&env, &repository.join("services/other")), "a project beside it stays off");
}

const IN_DATA: [&str; 2] = ["settings.json", crate::build_tools::SETTINGS_FILE];

fn agent_changes_of_the_data_folder(f: &mut Fixture, file: &str) -> Vec<String> {
    let path = f.data.join(file);
    let original = format!("{{\"keep\":\"{file}\"}}");
    let link = f.project.join("linked.json");
    let attempts = [
        ("editor.text.write", json!({ "path": path, "text": "{}" })),
        ("editor.text.write", json!({ "path": link, "text": "{}" })),
        ("editor.text.replace", json!({ "path": path, "oldText": "keep", "newText": "gone" })),
        ("editor.text.batch", json!({ "files": [{ "path": path, "text": "{}" }], "save": true })),
        ("file.fs.rename", json!({ "path": path, "name": "gone.json" })),
        ("file.fs.delete", json!({ "paths": [path] })),
        ("file.fs.newFile", json!({ "directory": f.data, "name": "new.json" })),
        ("file.document.open", json!({ "path": path })),
    ];
    let mut changed = Vec::new();
    for (command, params) in attempts {
        std::fs::create_dir_all(&f.data).unwrap();
        std::fs::write(&path, &original).unwrap();
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&path, &link).unwrap();
        let answer = f.as_agent(command, params.clone());
        let now = std::fs::read_to_string(&path).unwrap_or_default();
        let made = f.data.join("new.json");
        if now != original || made.exists() {
            changed.push(format!("{command} {params}"));
            let _ = std::fs::remove_file(&made);
        } else if let Ok(answer) = answer {
            changed.push(format!("{command} answered {answer} and left the file"));
        }
        let _ = f.run("file.document.closeAll", json!({}));
    }
    changed
}

#[cfg(unix)]
#[test]
fn an_agent_writes_nothing_into_the_data_folder_through_the_ide_s_commands() {
    let mut f = fixture();
    let mut changed = Vec::new();
    for file in IN_DATA {
        changed.extend(agent_changes_of_the_data_folder(&mut f, file));
    }
    assert!(changed.is_empty(), "an agent's command reached the data folder:\n{}", changed.join("\n"));
}

#[test]
fn an_agent_saves_no_file_of_the_data_folder_a_person_has_open() {
    let mut f = fixture();
    let path = f.data.join("settings.json");
    std::fs::create_dir_all(&f.data).unwrap();
    std::fs::write(&path, "{}").unwrap();
    f.run("file.document.open", json!({ "path": path })).unwrap();
    f.run("editor.edit.type", json!({ "text": " " })).unwrap();
    for command in ["file.document.save", "file.document.saveAll"] {
        assert_eq!(code(f.as_agent(command, json!({}))), AGENT_NOT_ALLOWED, "{command}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}", "{command}");
    }
    f.run("file.document.save", json!({})).expect("the person's save");
    assert_ne!(std::fs::read_to_string(&path).unwrap(), "{}");
}

#[test]
fn an_agent_still_edits_a_scratch_in_the_data_folder() {
    let mut f = fixture();
    let fake = with_fake_runs(&mut f);
    let (file, _) = scratch_in_data(&mut f, &fake);
    let answer = f.as_agent("editor.text.write", json!({ "path": file, "text": "changed\n", "save": true }));
    assert!(answer.is_ok(), "{answer:?}");
}
