use std::sync::Arc;

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use zen_run::fake::{FakeProvider, KIND};
use zen_run::{
    CommandLine, FieldSpec, OutputParser, PlainParser, RunConfigurations, RunContext, RunEnv, RunProvider, RunTarget,
};
use zen_workbench::run::Runs;

use super::*;

const FIELDS: &str = "fields.all";

struct FieldsProvider;

impl RunProvider for FieldsProvider {
    fn id(&self) -> &str {
        "fields"
    }

    fn kinds(&self) -> Vec<KindSpec> {
        let field = |key: &str, kind| FieldSpec { key: key.to_owned(), label_message: format!("label-{key}"), kind };
        vec![KindSpec {
            id: FIELDS.to_owned(),
            name_message: "fields-kind".to_owned(),
            fields: vec![
                field("text", FieldKind::Text),
                field("path", FieldKind::Path),
                field("mode", FieldKind::Choice(vec!["a".to_owned(), "b".to_owned()])),
                field("flag", FieldKind::Bool),
                field("vars", FieldKind::Env),
            ],
            defaults: json!({ "text": "hello", "mode": "a" }),
        }]
    }

    fn targets(&self, _context: &RunContext, _path: &Path, _text: &str) -> Vec<RunTarget> {
        Vec::new()
    }

    fn context(&self, _root: &Path, _path: &Path, _text: &str, _offset: usize) -> Option<Configuration> {
        None
    }

    fn command_lines(&self, _configuration: &Configuration, _context: &RunContext) -> Result<Vec<CommandLine>, String> {
        Ok(Vec::new())
    }

    fn parser(&self, _configuration: &Configuration, _context: &RunContext) -> Box<dyn OutputParser> {
        Box::new(PlainParser)
    }
}

struct Harness {
    ctx: egui::Context,
    registry: Registry,
    session: Session,
    dialog: RunConfigurationsDialog,
    saved: usize,
    dir: tempfile::TempDir,
}

impl Harness {
    fn new() -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::set_language(Some("en"));
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        session
            .state_mut::<Runs>()
            .set_providers(vec![Arc::new(FakeProvider::new()), Arc::new(FieldsProvider)], RunEnv::default());
        let ctx = egui::Context::default();
        theme::install(&ctx);
        Self { ctx, registry, session, dialog: RunConfigurationsDialog::default(), saved: 0, dir }
    }

    fn create(&mut self, configuration: Configuration) -> String {
        let params = json!({ "configuration": configuration });
        let created = self.registry.dispatch(&mut self.session, "run.configuration.create", params).unwrap();
        created["id"].as_str().unwrap().to_owned()
    }

    fn file(&self) -> RunConfigurations {
        RunConfigurations::read(self.dir.path()).unwrap()
    }

    fn frame_with(&mut self, events: Vec<Event>) {
        let input = egui::RawInput {
            events,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))),
            ..Default::default()
        };
        let Self { ctx, registry, session, dialog, saved, .. } = self;
        let mut output = ctx.run_ui(input, |ui| {
            if dialog.show(ui, &IdePalette::zen_dark(), registry, session) {
                *saved += 1;
            }
        });
        output.textures_delta.clear();
    }

    fn open(&mut self) {
        self.dialog.open();
        self.frame_with(Vec::new());
        self.frame_with(Vec::new());
    }

    fn click(&mut self, control: Control) {
        self.frame_with(Vec::new());
        self.frame_with(Vec::new());
        let rect = self.dialog.control(&control).unwrap_or_else(|| panic!("{control:?} is not shown"));
        let at = rect.center();
        self.frame_with(vec![Event::PointerMoved(at)]);
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        self.frame_with(vec![button(true)]);
        self.frame_with(vec![button(false)]);
        self.frame_with(Vec::new());
    }

    fn type_into(&mut self, control: Control, text: &str) {
        self.click(control);
        let select_all =
            Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND };
        self.frame_with(vec![select_all]);
        let typed = if text.is_empty() {
            Event::Key {
                key: Key::Backspace,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }
        } else {
            Event::Text(text.to_owned())
        };
        self.frame_with(vec![typed]);
        self.frame_with(Vec::new());
    }

    fn draft(&self) -> &Draft {
        self.dialog.draft().expect("the dialog is open")
    }

    fn current(&self) -> &Configuration {
        &self.draft().current_entry().expect("a configuration is shown").configuration
    }

    fn kind_index(&self, kind: &str) -> usize {
        self.draft().kinds.iter().position(|known| known.spec.id == kind).unwrap()
    }
}

#[test]
fn the_list_shows_the_selected_configuration_and_the_kinds() {
    let mut harness = Harness::new();
    harness.create(FakeProvider::configuration("first"));
    let second = harness.create(FakeProvider::configuration("second"));
    harness.registry.dispatch(&mut harness.session, "run.configuration.select", json!({ "id": second })).unwrap();
    harness.open();
    assert!(harness.dialog.is_open());
    let draft = harness.draft();
    assert_eq!(draft.kinds.len(), 3);
    assert_eq!(draft.kinds[0].provider, "fake");
    assert_eq!(draft.kinds[2].spec.id, zen_workbench::compound::KIND, "a compound can always be made");
    assert_eq!(draft.entries.len(), 2);
    assert_eq!(harness.current().name, "second");
    assert!(harness.dialog.control(&Control::Row(0)).is_some());
    harness.click(Control::Row(0));
    assert_eq!(harness.current().name, "first");
}

#[test]
fn a_new_configuration_is_made_from_the_plus_menu_and_saved_by_apply() {
    let mut harness = Harness::new();
    harness.open();
    assert!(harness.draft().entries.is_empty());
    harness.click(Control::Add);
    let index = harness.kind_index(KIND);
    harness.click(Control::AddKind(index));
    assert_eq!(harness.current().kind, KIND);
    let defaults = json!({ "target": "", "fail": null, "noDebug": null, "noDebugger": null, "views": null });
    assert_eq!(harness.current().options, defaults, "the kind's defaults");
    harness.type_into(Control::Name, "app");
    harness.type_into(Control::Field("target".to_owned()), "./cmd/app");
    harness.type_into(Control::ProgramArguments, "--port 8080");
    harness.type_into(Control::ToolArguments, "-race");
    harness.type_into(Control::WorkingDirectory, "cmd");
    assert!(harness.file().configurations.is_empty(), "nothing is saved before Apply");
    harness.click(Control::Apply);
    assert_eq!(harness.saved, 1);
    let file = harness.file();
    assert_eq!(file.configurations.len(), 1);
    let saved = &file.configurations[0];
    assert_eq!(saved.name, "app");
    assert_eq!(saved.options["target"], "./cmd/app");
    assert_eq!(saved.common.program_arguments, "--port 8080");
    assert_eq!(saved.common.tool_arguments, "-race");
    assert_eq!(saved.common.working_directory.as_deref(), Some(Path::new("cmd")));
    assert_eq!(harness.draft().entries[0].origin.as_deref(), Some(saved.id.as_str()));
    assert!(!harness.draft().is_modified());
    assert!(harness.dialog.is_open(), "Apply keeps the dialog open");
}

#[test]
fn cancel_discards_the_draft_and_ok_saves_and_selects() {
    let mut harness = Harness::new();
    let first = harness.create(FakeProvider::configuration("first"));
    let second = harness.create(FakeProvider::configuration("second"));
    harness.open();
    harness.type_into(Control::Name, "renamed");
    assert!(harness.draft().is_modified());
    harness.click(Control::Cancel);
    assert!(!harness.dialog.is_open());
    assert_eq!(harness.file().get(&first).unwrap().name, "first");
    assert_eq!(harness.saved, 0);
    harness.open();
    assert_eq!(harness.current().name, "first", "a new draft is read");
    harness.click(Control::Row(1));
    harness.type_into(Control::Name, "renamed");
    harness.click(Control::Ok);
    assert!(!harness.dialog.is_open());
    let file = harness.file();
    assert_eq!(file.get(&second).unwrap().name, "renamed");
    let selected = harness.session.state::<Runs>().unwrap().selected().map(str::to_owned);
    assert_eq!(selected.as_deref(), Some(second.as_str()), "OK selects the configuration shown");
}

#[test]
fn remove_copy_and_save_a_temporary_configuration() {
    let mut harness = Harness::new();
    let kept = harness.create(FakeProvider::configuration("kept"));
    let gone = harness.create(FakeProvider::configuration("gone"));
    let temporary = harness.create(Configuration { temporary: true, ..FakeProvider::configuration("TestOne") });
    harness.open();
    harness.click(Control::Row(1));
    harness.click(Control::Remove);
    assert_eq!(harness.draft().entries.len(), 2);
    harness.click(Control::Row(0));
    harness.click(Control::Copy);
    assert_eq!(harness.current().name, "kept (1)");
    assert!(harness.current().id.is_empty());
    let temporary_row = harness.draft().entries.iter().position(|entry| entry.configuration.temporary).unwrap();
    harness.click(Control::Row(temporary_row));
    harness.click(Control::Save);
    assert!(!harness.current().temporary);
    harness.click(Control::Ok);
    let file = harness.file();
    assert!(file.configurations.iter().all(|kept| kept.name != "gone"), "{gone} is removed");
    assert_eq!(file.configurations.len(), 3);
    assert!(file.configurations.iter().any(|kept| kept.name == "kept (1)" && kept.options["target"] == "kept"));
    assert!(!file.get(&temporary).unwrap().temporary);
    assert_eq!(file.get(&kept).unwrap().name, "kept");
}

#[test]
fn the_form_is_made_from_the_kind_fields() {
    let mut harness = Harness::new();
    harness.open();
    harness.click(Control::Add);
    let index = harness.kind_index(FIELDS);
    harness.click(Control::AddKind(index));
    assert_eq!(harness.current().options, json!({ "text": "hello", "mode": "a" }));
    for key in ["text", "path", "mode", "flag"] {
        assert!(harness.dialog.control(&Control::Field(key.to_owned())).is_some(), "{key}");
    }
    assert!(harness.dialog.control(&Control::Browse(Box::new(Control::Field("path".to_owned())))).is_some());
    assert!(harness.dialog.control(&Control::Browse(Box::new(Control::WorkingDirectory))).is_some());
    let vars = EnvTarget::Field("vars".to_owned());
    assert!(harness.dialog.control(&Control::EnvAdd(vars.clone())).is_some());
    harness.type_into(Control::Field("text".to_owned()), "typed");
    harness.type_into(Control::Field("path".to_owned()), "cmd/app");
    harness.click(Control::Field("flag".to_owned()));
    harness.click(Control::Field("mode".to_owned()));
    harness.click(Control::Choice("mode".to_owned(), "b".to_owned()));
    harness.click(Control::EnvAdd(vars.clone()));
    harness.type_into(Control::EnvName(vars.clone(), 0), "KEY");
    harness.type_into(Control::EnvValue(vars, 0), "value");
    assert_eq!(
        harness.current().options,
        json!({ "text": "typed", "path": "cmd/app", "mode": "b", "flag": true, "vars": [["KEY", "value"]] })
    );
}

#[test]
fn the_environment_table_adds_edits_and_removes_variables() {
    let mut harness = Harness::new();
    harness.create(FakeProvider::configuration("app"));
    harness.open();
    harness.click(Control::EnvAdd(EnvTarget::Common));
    harness.click(Control::EnvAdd(EnvTarget::Common));
    harness.type_into(Control::EnvName(EnvTarget::Common, 0), "A");
    harness.type_into(Control::EnvValue(EnvTarget::Common, 0), "1");
    harness.type_into(Control::EnvName(EnvTarget::Common, 1), "B");
    harness.type_into(Control::EnvValue(EnvTarget::Common, 1), "2");
    assert_eq!(harness.current().common.env, vec![("A".into(), "1".into()), ("B".into(), "2".into())]);
    harness.click(Control::EnvRemove(EnvTarget::Common, 0));
    assert_eq!(harness.current().common.env, vec![("B".to_owned(), "2".to_owned())]);
    harness.click(Control::Apply);
    assert_eq!(harness.file().configurations[0].common.env, vec![("B".to_owned(), "2".to_owned())]);
}

#[test]
fn names_are_validated_before_saving() {
    let mut harness = Harness::new();
    let id = harness.create(FakeProvider::configuration("app"));
    harness.create(Configuration { kind: "other.kind".to_owned(), ..FakeProvider::configuration("orphan") });
    harness.open();
    harness.type_into(Control::Name, "");
    assert!(!harness.draft().is_valid());
    assert_eq!(harness.draft().problems(0)[0].0, t!("ide-run-configurations-error-name"));
    harness.click(Control::Ok);
    assert!(harness.dialog.is_open(), "OK does nothing while a name is missing");
    harness.type_into(Control::Name, "app");
    harness.click(Control::Copy);
    harness.type_into(Control::Name, "app");
    assert!(harness.draft().problems(1).iter().any(|(_, blocking)| *blocking));
    harness.type_into(Control::Name, "app 2");
    assert!(harness.draft().is_valid());
    let orphan = harness.draft().entries.iter().position(|entry| entry.configuration.kind == "other.kind").unwrap();
    let problems = harness.draft().problems(orphan);
    assert_eq!(problems.len(), 1);
    assert!(!problems[0].1, "an unknown kind does not keep the dialog from saving");
    harness.click(Control::Ok);
    assert!(!harness.dialog.is_open());
    assert_eq!(harness.file().configurations.len(), 3);
    assert_eq!(harness.file().get(&id).unwrap().name, "app");
}

#[test]
fn a_failed_command_is_shown_in_the_dialog() {
    let mut harness = Harness::new();
    harness.create(FakeProvider::configuration("app"));
    harness.open();
    harness.type_into(Control::Name, "renamed");
    std::fs::write(harness.dir.path().join(".zen/runConfigurations.json"), "{ broken").unwrap();
    harness.click(Control::Ok);
    assert!(harness.dialog.is_open());
    assert!(
        harness.draft().error.as_deref().is_some_and(
            |error| error.contains("workbench-run-file-broken") || error.contains("runConfigurations.json")
        )
    );
}

struct Shared;

impl zen_project::settings::SettingsProvider for Shared {
    fn id(&self) -> &str {
        "shared"
    }

    fn watches(&self, _: &Path) -> bool {
        false
    }

    fn load(
        &self,
        root: &Path,
        _: &dyn zen_project::settings::ReadFiles,
    ) -> Option<zen_project::settings::ProjectSettings> {
        let serve = zen_run::ForeignRunConfiguration {
            source_file: root.join(".run/Serve.run.xml"),
            name: "Serve".to_owned(),
            type_id: zen_run::fake::ADOPTED_TYPE.to_owned(),
            shared: true,
            ..Default::default()
        };
        Some(zen_project::settings::ProjectSettings { run_configurations: vec![serve], ..Default::default() })
    }
}

#[test]
fn an_adopted_configuration_is_read_only_until_it_is_saved_as_a_zen_one() {
    let mut harness = Harness::new();
    harness
        .session
        .state_mut::<zen_workbench::project_settings::ProjectSettingsState>()
        .set_providers(vec![Arc::new(Shared)]);
    harness.open();
    assert_eq!(harness.current().name, "Serve");
    assert!(harness.current().is_adopted());
    harness.click(Control::Remove);
    assert_eq!(harness.draft().entries.len(), 1, "an adopted configuration cannot be removed");
    harness.click(Control::Copy);
    assert!(!harness.current().is_adopted(), "a copy is a Zen configuration");
    harness.click(Control::Row(0));
    harness.click(Control::Save);
    harness.click(Control::Apply);
    assert!(harness.draft().error.is_none(), "{:?}", harness.draft().error);
    let file = harness.file();
    let names: Vec<&str> = file.configurations.iter().map(|kept| kept.name.as_str()).collect();
    assert_eq!(names, ["Serve", "Serve (1)"]);
    assert!(harness.draft().entries.iter().all(|entry| !entry.configuration.is_adopted()));
    assert!(!harness.dir.path().join(".run").exists());
}

impl Harness {
    fn task_kind(&self, kind: &str) -> usize {
        self.draft().task_kinds.iter().position(|known| known.spec.id == kind).unwrap()
    }

    fn tasks(&self) -> Vec<String> {
        self.current().common.before_launch.iter().map(|task| self.draft().task_label(task)).collect()
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        self.frame_with(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
        self.frame_with(Vec::new());
    }
}

#[test]
fn before_launch_tasks_are_added_edited_moved_and_removed_and_saved_by_apply() {
    let mut harness = Harness::new();
    harness.create(FakeProvider::configuration("build"));
    let app = harness.create(FakeProvider::configuration("app"));
    harness.registry.dispatch(&mut harness.session, "run.configuration.select", json!({ "id": app })).unwrap();
    harness.open();
    assert_eq!(harness.current().name, "app");
    assert!(harness.dialog.control(&Control::TaskRemove).is_some());
    harness.click(Control::TaskAdd);
    let kind = harness.task_kind(zen_workbench::before_launch::TOOL_WINDOW);
    harness.click(Control::TaskAddKind(kind));
    assert!(harness.draft().tasks.editing, "a new task with fields opens for editing");
    harness.click(Control::TaskField("toolWindow".to_owned()));
    harness.click(Control::TaskChoice("toolWindow".to_owned(), "terminal".to_owned()));
    harness.click(Control::TaskAdd);
    let kind = harness.task_kind(zen_workbench::before_launch::RUN_CONFIGURATION);
    harness.click(Control::TaskAddKind(kind));
    harness.click(Control::TaskField("name".to_owned()));
    harness.click(Control::TaskChoice("name".to_owned(), "build".to_owned()));
    assert_eq!(
        harness.tasks(),
        ["Activate Tool Window “Terminal”", "Run Another Configuration “build”"],
        "the list names each task with what it runs"
    );
    harness.click(Control::TaskUp);
    assert_eq!(harness.tasks()[0], "Run Another Configuration “build”");
    harness.click(Control::TaskRow(1));
    harness.click(Control::TaskRemove);
    assert_eq!(harness.tasks(), ["Run Another Configuration “build”"]);
    harness.click(Control::ShowPage);
    assert!(harness.file().get(&app).unwrap().common.before_launch.is_empty(), "nothing is saved before Apply");
    harness.click(Control::Apply);
    let saved = harness.file().get(&app).unwrap().common.clone();
    assert_eq!(saved.before_launch, vec![BeforeLaunchTask::new("runConfiguration", json!({ "name": "build" }))]);
    assert!(saved.show_page);
}

#[test]
fn the_before_launch_list_takes_its_keys_once_clicked() {
    let mut harness = Harness::new();
    harness.create(FakeProvider::configuration("app"));
    harness.open();
    harness.press(Key::N, Modifiers::COMMAND);
    assert!(!harness.draft().tasks.adding, "not before the list was used");
    harness.click(Control::TaskAdd);
    harness.click(Control::TaskAddKind(harness.task_kind(zen_workbench::before_launch::SAVE_FILES)));
    assert!(!harness.draft().tasks.editing, "a task without fields has nothing to edit");
    harness.press(Key::N, Modifiers::COMMAND);
    assert!(harness.draft().tasks.adding, "⌘N opens the Add menu");
    harness.click(Control::TaskAdd);
    assert!(!harness.draft().tasks.adding, "Add again closes the menu");
    harness.press(Key::Insert, Modifiers::ALT);
    assert!(harness.draft().tasks.adding, "Alt+Insert too");
    harness.click(Control::TaskAddKind(harness.task_kind(zen_workbench::before_launch::TOOL_WINDOW)));
    assert_eq!(harness.draft().selected_task(), Some(1));
    harness.press(Key::ArrowUp, Modifiers::ALT);
    assert_eq!(harness.tasks()[0], "Activate Tool Window “Run”");
    harness.press(Key::Delete, Modifiers::ALT);
    assert_eq!(harness.tasks(), ["Save Files"]);
}

#[test]
fn show_this_page_opens_the_dialog_whose_run_button_starts_the_configuration() {
    let mut harness = Harness::new();
    let mut configuration = FakeProvider::configuration("app");
    configuration.common.show_page = true;
    let id = harness.create(configuration);
    harness.create(FakeProvider::configuration("other"));
    let page = page_before_start(&harness.registry, &mut harness.session, &json!({ "configurationId": id }));
    assert_eq!(page.as_deref(), Some(id.as_str()));
    let context = json!({ "configurationId": id, "context": { "path": "main.go" } });
    assert_eq!(page_before_start(&harness.registry, &mut harness.session, &context), None);
    harness.dialog.open_to_launch(&id, "run.process.start");
    harness.frame_with(Vec::new());
    harness.frame_with(Vec::new());
    assert_eq!(harness.current().name, "app", "the dialog opens on the configuration");
    harness.click(Control::Ok);
    assert!(!harness.dialog.is_open());
    assert_eq!(harness.dialog.take_launch(), Some(("run.process.start".to_owned(), id.clone())));
    assert_eq!(harness.dialog.take_launch(), None);
    harness.open();
    harness.click(Control::Ok);
    assert_eq!(harness.dialog.take_launch(), None, "OK of the plain dialog starts nothing");
}
#[test]
fn the_inline_environment_field_and_env_files_are_lists_split_by_semicolons() {
    let env = parse_env_text("A=1;B=x=y;;C");
    assert_eq!(env, [("A".into(), "1".into()), ("B".into(), "x=y".into()), ("C".into(), String::new())]);
    assert_eq!(env_text(&env), "A=1;B=x=y;C=");
    let files = parse_env_files(" .env ; /etc/app.env;");
    assert_eq!(files, [PathBuf::from(".env"), PathBuf::from("/etc/app.env")]);
    assert_eq!(env_files_text(&files), ".env;/etc/app.env");
    assert_eq!(insert_macro("", "$FilePath$"), "$FilePath$");
    assert_eq!(insert_macro("-v", "$FilePath$"), "-v $FilePath$");
    assert_eq!(insert_macro("--dir=", "$FileDir$"), "--dir=$FileDir$");
}

#[test]
fn the_common_launch_options_are_edited_and_saved() {
    let mut harness = Harness::new();
    harness.create(FakeProvider::configuration("app"));
    harness.open();
    assert!(
        harness.draft().macros.iter().any(|item| item.token == "$ProjectFileDir$"),
        "Insert Macro lists the macros"
    );
    assert!(!harness.draft().system_env.is_empty(), "the system variables are shown read-only");
    harness.type_into(Control::EnvInline, "A=1;B=2");
    assert_eq!(harness.current().common.env, vec![("A".into(), "1".into()), ("B".into(), "2".into())]);
    assert!(harness.dialog.control(&Control::EnvName(EnvTarget::Common, 1)).is_some(), "the table shows them too");
    harness.type_into(Control::EnvFiles, ".env;conf/dev.env");
    harness.click(Control::AllowMultiple);
    harness.click(Control::ActivateToolWindow);
    harness.click(Control::FocusToolWindow);
    harness.click(Control::InsertMacro(Box::new(Control::ProgramArguments)));
    let project = harness.draft().macros.iter().position(|item| item.token == "$ProjectFileDir$").unwrap();
    harness.click(Control::Macro(Box::new(Control::ProgramArguments), project));
    assert_eq!(harness.current().common.program_arguments, "$ProjectFileDir$");
    harness.click(Control::Apply);
    let common = harness.file().configurations[0].common.clone();
    assert_eq!(common.env_files, [PathBuf::from(".env"), PathBuf::from("conf/dev.env")]);
    assert!(common.allow_multiple_instances && !common.activate_tool_window && common.focus_tool_window);
    assert_eq!(common.program_arguments, "$ProjectFileDir$");
}

#[test]
fn build_artifacts_lists_the_projects_artifacts_to_tick() {
    let mut harness = Harness::new();
    zen_workbench::before_launch::register_type(&mut harness.session, zen_workbench::artifacts::task_type());
    let kept = json!({ "artifacts": [{ "name": "shop:jar" }, { "name": "tools:jar" }] });
    std::fs::create_dir_all(harness.dir.path().join(".zen")).unwrap();
    std::fs::write(harness.dir.path().join(zen_workbench::artifacts::FILE), kept.to_string()).unwrap();
    let app = harness.create(FakeProvider::configuration("app"));
    harness.registry.dispatch(&mut harness.session, "run.configuration.select", json!({ "id": app })).unwrap();
    harness.open();
    assert_eq!(harness.draft().artifacts, ["shop:jar", "tools:jar"]);
    harness.click(Control::TaskAdd);
    let kind = harness.task_kind(zen_workbench::artifacts::TASK);
    harness.click(Control::TaskAddKind(kind));
    assert_eq!(harness.tasks(), ["Build Artifacts"]);
    harness.click(Control::TaskChoice("artifacts".to_owned(), "tools:jar".to_owned()));
    harness.click(Control::TaskChoice("artifacts".to_owned(), "shop:jar".to_owned()));
    assert_eq!(harness.tasks(), ["Build Artifacts “shop:jar, tools:jar”"], "ticked in the list's order");
    harness.click(Control::TaskChoice("artifacts".to_owned(), "tools:jar".to_owned()));
    harness.click(Control::Apply);
    let saved = harness.file().get(&app).unwrap().common.before_launch.clone();
    assert_eq!(saved, vec![BeforeLaunchTask::new("buildArtifacts", json!({ "artifacts": ["shop:jar"] }))]);
}

#[test]
fn run_file_watchers_offers_all_or_one_watcher_and_the_files() {
    let mut harness = Harness::new();
    zen_workbench::before_launch::register_type(&mut harness.session, zen_workbench::file_watchers::task_type());
    let watcher = json!({ "name": "Prettier", "program": "prettier" });
    let saved = harness.registry.dispatch(&mut harness.session, "run.fileWatchers.save", json!({ "watcher": watcher }));
    assert_eq!(saved.unwrap()["id"], "prettier");
    let app = harness.create(FakeProvider::configuration("app"));
    harness.registry.dispatch(&mut harness.session, "run.configuration.select", json!({ "id": app })).unwrap();
    harness.open();
    harness.click(Control::TaskAdd);
    let kind = harness.task_kind(zen_workbench::file_watchers::TASK);
    harness.click(Control::TaskAddKind(kind));
    assert_eq!(harness.tasks(), ["Run File Watchers “Changed files”"], "every watcher on the changed files at first");
    harness.click(Control::TaskField("watcher".to_owned()));
    harness.click(Control::TaskChoice("watcher".to_owned(), "project:prettier".to_owned()));
    harness.click(Control::TaskField("files".to_owned()));
    harness.click(Control::TaskChoice("files".to_owned(), "open".to_owned()));
    assert_eq!(harness.tasks(), ["Run File Watchers “Prettier”"]);
    harness.click(Control::Apply);
    let saved = harness.file().get(&app).unwrap().common.before_launch.clone();
    let task = json!({ "watcher": "project:prettier", "files": "open" });
    assert_eq!(saved, vec![BeforeLaunchTask::new("fileWatchers", task)]);
}

impl Harness {
    fn drag(&mut self, from: Control, to: Control) {
        self.frame_with(Vec::new());
        let start = self.dialog.control(&from).unwrap_or_else(|| panic!("{from:?} is not shown")).center();
        let end = self.dialog.control(&to).unwrap_or_else(|| panic!("{to:?} is not shown")).center();
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        self.frame_with(vec![Event::PointerMoved(start)]);
        self.frame_with(vec![button(start, true)]);
        for step in 1..=6 {
            let at = start + (end - start) * (step as f32 / 6.0);
            self.frame_with(vec![Event::PointerMoved(at)]);
        }
        self.frame_with(vec![button(end, false)]);
        self.frame_with(Vec::new());
    }

    fn row_of(&self, name: &str) -> usize {
        self.draft().entries.iter().position(|entry| entry.configuration.name == name).unwrap()
    }
}

#[test]
fn a_compound_lists_the_configurations_it_starts_and_one_that_contains_itself_cannot_be_saved() {
    let mut harness = Harness::new();
    let app = harness.create(FakeProvider::configuration("app"));
    let api = harness.create(FakeProvider::configuration("api"));
    harness.open();
    harness.click(Control::Add);
    let compound = harness.kind_index(zen_workbench::compound::KIND);
    harness.click(Control::AddKind(compound));
    harness.type_into(Control::Name, "both");
    assert!(harness.dialog.control(&Control::ProgramArguments).is_none(), "a compound has only its members");
    harness.click(Control::MemberAdd);
    let app_row = harness.row_of("app");
    harness.click(Control::MemberAddItem(app_row));
    harness.click(Control::MemberAdd);
    let api_row = harness.row_of("api");
    harness.click(Control::MemberAddItem(api_row));
    assert_eq!(zen_workbench::compound::members(harness.current()), [app.clone(), api.clone()]);
    harness.click(Control::MemberRow(1));
    harness.click(Control::MemberRemove);
    assert_eq!(zen_workbench::compound::members(harness.current()), std::slice::from_ref(&app));
    harness.click(Control::Apply);
    let file = harness.file();
    let saved = file.configurations.iter().find(|kept| kept.name == "both").unwrap().clone();
    assert_eq!(saved.options, json!({ "configurations": [app] }));

    let outer = json!({ "name": "outer", "kind": "compound", "options": { "configurations": [saved.id] } });
    let outer =
        harness.registry.dispatch(&mut harness.session, "run.configuration.create", json!({ "configuration": outer }));
    let outer = outer.unwrap()["id"].as_str().unwrap().to_owned();
    harness.open();
    let both = harness.row_of("both");
    harness.click(Control::Row(both));
    harness.click(Control::MemberAdd);
    let outer_row = harness.row_of("outer");
    harness.click(Control::MemberAddItem(outer_row));
    assert!(!harness.draft().is_valid(), "the dialog shows the cycle and does not save");
    let problems = harness.draft().problems(both);
    assert!(problems.iter().any(|(problem, blocking)| *blocking && problem.contains("both")), "{problems:?}");
    harness.click(Control::Ok);
    assert!(harness.dialog.is_open());
    assert_eq!(harness.file().get(&outer).map(|kept| kept.name.as_str()), Some("outer"));
}

#[test]
fn templates_are_edited_in_the_dialog_and_new_configurations_start_from_them() {
    let mut harness = Harness::new();
    harness.open();
    harness.click(Control::Templates);
    assert!(harness.draft().editing_templates);
    let kinds: Vec<String> =
        harness.draft().templates.iter().map(|template| template.configuration.kind.clone()).collect();
    assert_eq!(kinds, [KIND, FIELDS], "a compound has no template");
    harness.click(Control::Template(0));
    assert!(harness.dialog.control(&Control::Name).is_none(), "a template has no name");
    harness.type_into(Control::ToolArguments, "-race");
    harness.type_into(Control::Field("target".to_owned()), "./...");
    assert!(harness.draft().is_modified());
    harness.click(Control::Apply);
    let file = harness.file();
    let template = file.template(KIND).expect("the template is saved");
    assert_eq!(
        (template.common.tool_arguments.as_str(), template.options["target"].as_str()),
        ("-race", Some("./..."))
    );
    harness.click(Control::Templates);
    assert!(!harness.draft().editing_templates);
    harness.click(Control::Add);
    let index = harness.kind_index(KIND);
    harness.click(Control::AddKind(index));
    assert_eq!(harness.current().common.tool_arguments, "-race");
    assert_eq!(harness.current().options["target"], "./...");
    harness.type_into(Control::ToolArguments, "");
    harness.click(Control::Apply);
    let file = harness.file();
    assert_eq!(file.configurations[0].common.tool_arguments, "", "what the dialog cleared stays cleared");
}

#[test]
fn folders_group_configurations_and_the_search_field_filters_the_tree() {
    let mut harness = Harness::new();
    harness.create(FakeProvider::configuration("server"));
    harness.create(FakeProvider::configuration("client"));
    harness.create(FakeProvider::configuration("tool"));
    harness.open();
    harness.click(Control::Row(0));
    harness.click(Control::NewFolder);
    let folder = t!("ide-run-configurations-new-folder-name");
    assert_eq!(harness.draft().folder, Some((KIND.to_owned(), folder.clone())));
    harness.type_into(Control::FolderName, "Servers");
    assert_eq!(harness.draft().entries[0].configuration.folder, "Servers");
    harness.drag(Control::Row(1), Control::Folder(KIND.to_owned(), "Servers".to_owned()));
    assert_eq!(harness.draft().entries[1].configuration.folder, "Servers", "a row dropped on a folder moves into it");
    harness.click(Control::Apply);
    let folders: Vec<String> = harness.file().configurations.iter().map(|kept| kept.folder.clone()).collect();
    assert_eq!(folders, ["Servers", "Servers", ""]);

    harness.type_into(Control::Search, "too");
    assert!(harness.dialog.control(&Control::Row(2)).is_some());
    assert!(harness.dialog.control(&Control::Row(0)).is_none(), "rows that do not match are not shown");
    assert!(harness.dialog.control(&Control::Folder(KIND.to_owned(), "Servers".to_owned())).is_none());
    harness.type_into(Control::Search, "");
    harness.click(Control::Folder(KIND.to_owned(), "Servers".to_owned()));
    harness.click(Control::Remove);
    assert!(
        harness.draft().entries.iter().all(|entry| entry.configuration.folder.is_empty()),
        "the folder goes, its configurations stay"
    );
    assert_eq!(harness.draft().entries.len(), 3);
}
