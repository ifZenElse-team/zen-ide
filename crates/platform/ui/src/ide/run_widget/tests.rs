use std::sync::Arc;

use egui::{Event, PointerButton, Pos2, Rect, ViewportId, ViewportInfo};
use zen_run::fake::FakeProvider;
use zen_run::{RunConfigurations, RunEnv};

use super::*;

struct Harness {
    ctx: egui::Context,
    registry: Registry,
    session: Session,
    widget: RunWidget,
    actions: Vec<WidgetAction>,
    fake: FakeProvider,
    dir: tempfile::TempDir,
}

impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        registry.dispatch(&mut session, "file.project.open", json!({ "path": dir.path() })).unwrap();
        session.set_state(zen_workbench::build_tools::BuildEnv::default());
        registry.dispatch(&mut session, "project.trust.grant", json!({})).unwrap();
        let fake = FakeProvider::new();
        session.state_mut::<Runs>().set_providers(vec![Arc::new(fake.clone())], RunEnv::default());
        let ctx = egui::Context::default();
        theme::install(&ctx);
        Self { ctx, registry, session, widget: RunWidget::default(), actions: Vec::new(), fake, dir }
    }

    fn create(&mut self, name: &str, temporary: bool, select: bool) -> String {
        let configuration = Configuration { temporary, ..FakeProvider::configuration(name) };
        let params = json!({ "configuration": configuration, "select": select });
        let created = self.registry.dispatch(&mut self.session, "run.configuration.create", params).unwrap();
        created["id"].as_str().unwrap().to_owned()
    }

    fn frame_with(&mut self, events: Vec<Event>, focused: Option<bool>) {
        let mut input = egui::RawInput {
            events,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(900.0, 600.0))),
            ..Default::default()
        };
        input.viewports.insert(ViewportId::ROOT, ViewportInfo { focused, ..Default::default() });
        let Self { ctx, registry, session, widget, actions, .. } = self;
        let mut output = ctx.run_ui(input, |ui| {
            let (_, action) = widget.show(ui, &IdePalette::zen_dark(), registry, session, 800.0, 20.0);
            actions.extend(action);
        });
        output.textures_delta.clear();
    }

    fn frame(&mut self) {
        self.frame_with(Vec::new(), Some(true));
    }

    fn click(&mut self, at: Pos2) {
        self.frame_with(vec![Event::PointerMoved(at)], Some(true));
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        self.frame_with(vec![button(true)], Some(true));
        self.frame_with(vec![button(false)], Some(true));
        self.frame();
    }

    fn click_button(&mut self, name: &str) {
        let rect = self.widget.button(name).unwrap_or_else(|| panic!("no {name} button"));
        self.click(rect.center());
    }

    fn take_action(&mut self) -> Option<WidgetAction> {
        let action = self.actions.pop();
        self.actions.clear();
        action
    }

    fn perform(&mut self, action: WidgetAction) -> Value {
        let commands = match action {
            WidgetAction::Dispatch(command, params) => vec![(command, params)],
            WidgetAction::Rerun(_, id) => rerun_commands(&id).to_vec(),
            WidgetAction::EditConfigurations => panic!("not a command"),
        };
        let mut answer = Value::Null;
        for (command, params) in commands {
            answer = self.registry.dispatch(&mut self.session, command, params).unwrap();
        }
        self.widget.invalidate();
        self.frame();
        answer
    }

    fn wait_ended(&mut self, run: u64) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            let params = json!({ "runId": run, "from": 0, "waitMs": 200 });
            let output = self.registry.dispatch(&mut self.session, "run.output.get", params).unwrap();
            if output["state"] != "running" {
                return;
            }
            assert!(std::time::Instant::now() < deadline, "run {run} does not end");
        }
    }

    fn pretend_running(&mut self, run: u64, id: &str) {
        self.widget.running = vec![(run, id.to_owned())];
        self.frame();
    }
}

#[test]
fn without_configurations_the_widget_offers_to_add_one() {
    let mut harness = Harness::new();
    harness.frame();
    assert_eq!(harness.widget.state(), WidgetState::Empty);
    assert!(harness.widget.button("stop").is_none());
    harness.click_button("run");
    assert_eq!(harness.take_action(), None, "Run does nothing without a configuration");
    harness.click_button("selector");
    assert_eq!(harness.take_action(), Some(WidgetAction::EditConfigurations));
    assert!(!harness.widget.popup_open());
}

#[test]
fn run_starts_the_selected_configuration() {
    let mut harness = Harness::new();
    harness.create("first", false, false);
    let second = harness.create("second", false, true);
    harness.frame();
    assert_eq!(harness.widget.state(), WidgetState::Selected(second.clone()));
    assert!(harness.widget.button("stop").is_none());
    harness.click_button("run");
    let action = harness.take_action().unwrap();
    assert_eq!(action, WidgetAction::Dispatch("run.process.start", json!({ "configurationId": second })));
    harness.perform(action);
    assert_eq!(harness.fake.replays(), 1);
}

#[test]
fn debug_starts_the_selected_configuration_in_the_debugger() {
    let mut harness = Harness::new();
    harness.frame();
    harness.click_button("debug");
    assert_eq!(harness.take_action(), None, "Debug does nothing without a configuration");
    let id = harness.create("server", false, true);
    harness.widget.invalidate();
    harness.frame();
    let (run, debug) = (harness.widget.button("run").unwrap(), harness.widget.button("debug").unwrap());
    assert!(run.right() <= debug.left(), "Debug is to the right of Run");
    harness.click_button("debug");
    assert_eq!(
        harness.take_action(),
        Some(WidgetAction::Dispatch("debug.session.start", json!({ "configurationId": id })))
    );
    harness.pretend_running(3, &id);
    let stop = harness.widget.button("stop").unwrap();
    assert!(harness.widget.button("debug").unwrap().right() <= stop.left(), "Stop comes last");
    harness.click_button("debug");
    assert_eq!(
        harness.take_action(),
        Some(WidgetAction::Dispatch("debug.session.start", json!({ "configurationId": id }))),
        "a running configuration can be debugged too"
    );
}

#[test]
fn more_actions_runs_the_selected_configuration_with_coverage_when_its_provider_can() {
    let mut harness = Harness::new();
    let id = harness.create("tests", false, true);
    harness.frame();
    let (debug, more) = (harness.widget.button("debug").unwrap(), harness.widget.button("more").unwrap());
    assert!(debug.right() <= more.left(), "More Actions follows Debug");
    harness.click_button("more");
    let row = harness.widget.more_row().expect("the menu is open");
    harness.click(row.center());
    assert_eq!(harness.take_action(), None, "the provider cannot measure coverage, so the item is disabled");
    harness.click(Pos2::new(20.0, 400.0));
    assert!(harness.widget.more_row().is_none(), "a click elsewhere closes the menu");

    harness.fake.set_coverage(Some("mode: set\na.go:1.1,1.5 1 1\n"));
    harness.click_button("more");
    let row = harness.widget.more_row().unwrap();
    harness.click(row.center());
    let action = harness.take_action().unwrap();
    assert_eq!(action, WidgetAction::Dispatch("coverage.run.start", json!({ "configurationId": id })));
    assert!(harness.widget.more_row().is_none(), "choosing closes the menu");
    harness.perform(action);
    assert_eq!(harness.fake.replays(), 1);
}

#[test]
fn the_popup_selects_a_configuration_and_opens_the_dialog() {
    let mut harness = Harness::new();
    let first = harness.create("first", false, false);
    let temporary = harness.create("TestThing", true, true);
    harness.frame();
    harness.click_button("selector");
    assert!(harness.widget.popup_open());
    assert!(harness.widget.popup_row(&PopupRow::Configuration(temporary.clone())).is_some());
    let row = harness.widget.popup_row(&PopupRow::Configuration(first.clone())).unwrap();
    harness.click(row.center());
    let action = harness.take_action().unwrap();
    assert_eq!(action, WidgetAction::Dispatch("run.configuration.select", json!({ "id": first })));
    assert!(!harness.widget.popup_open());
    harness.perform(action);
    assert_eq!(harness.session.state::<Runs>().unwrap().selected(), Some(first.as_str()));
    assert_eq!(harness.widget.state(), WidgetState::Selected(first));
    harness.click_button("selector");
    let edit = harness.widget.popup_row(&PopupRow::Edit).unwrap();
    harness.click(edit.center());
    assert_eq!(harness.take_action(), Some(WidgetAction::EditConfigurations));
    harness.click_button("selector");
    assert!(harness.widget.popup_open());
    harness.click(Pos2::new(20.0, 400.0));
    assert!(!harness.widget.popup_open(), "a click elsewhere closes the popup");
}

#[test]
fn a_running_configuration_shows_stop_and_rerun() {
    let mut harness = Harness::new();
    let id = harness.create("server", false, true);
    harness.frame();
    harness.pretend_running(7, &id);
    assert_eq!(harness.widget.state(), WidgetState::Running(id.clone(), 7));
    assert!(harness.widget.is_running());
    harness.click_button("stop");
    let stop = harness.take_action().unwrap();
    assert_eq!(stop, WidgetAction::Dispatch("run.process.stop", json!({ "runId": 7 })));
    harness.click_button("run");
    assert_eq!(harness.take_action(), Some(WidgetAction::Rerun(7, id.clone())));
    assert_eq!(
        rerun_commands(&id),
        [("run.process.start", json!({ "configurationId": id }))],
        "Rerun is a start, which stops the running instance"
    );
    harness.widget.invalidate();
    harness.frame();
    assert!(!harness.widget.is_running(), "reading again shows that nothing runs");
    assert!(harness.widget.button("stop").is_none());
}

#[test]
fn run_on_a_configuration_that_allows_multiple_instances_starts_another_one() {
    let mut harness = Harness::new();
    let id = harness.create("server", false, true);
    let mut configuration = FakeProvider::configuration("server");
    configuration.id = id.clone();
    configuration.common.allow_multiple_instances = true;
    let params = json!({ "configuration": configuration });
    harness.registry.dispatch(&mut harness.session, "run.configuration.update", params).unwrap();
    harness.widget.invalidate();
    harness.frame();
    harness.pretend_running(7, &id);
    harness.click_button("run");
    assert_eq!(
        harness.take_action(),
        Some(WidgetAction::Dispatch("run.process.start", json!({ "configurationId": id }))),
        "no rerun: a new instance"
    );
}

#[test]
fn the_widget_reads_again_on_focus_and_after_runs() {
    let mut harness = Harness::new();
    harness.frame();
    assert_eq!(harness.widget.state(), WidgetState::Empty);
    let id = harness.create("app", false, false);
    harness.frame();
    assert_eq!(harness.widget.state(), WidgetState::Empty, "nothing is read while nothing happens");
    harness.frame_with(Vec::new(), Some(false));
    harness.frame_with(Vec::new(), Some(true));
    assert_eq!(harness.widget.state(), WidgetState::Selected(id.clone()), "focus coming back reads again");
    let other = harness.create("other", false, false);
    harness.registry.dispatch(&mut harness.session, "run.process.start", json!({ "configurationId": other })).unwrap();
    harness.frame();
    assert_eq!(harness.widget.state(), WidgetState::Selected(other), "a new run reads again");
    assert!(harness.session.state::<Runs>().unwrap().temporary().is_empty());
}

#[test]
fn the_widget_shows_the_selection_the_session_keeps() {
    let mut harness = Harness::new();
    let id = harness.create("TestThing", true, false);
    let saved = harness.create("saved", false, true);
    harness.frame();
    assert_eq!(harness.widget.state(), WidgetState::Selected(saved));
    harness.session.state_mut::<Runs>().set_selected(Some(id.clone()));
    harness.widget.invalidate();
    harness.frame();
    assert_eq!(harness.widget.state(), WidgetState::Selected(id.clone()));
    assert!(harness.widget.selected_configuration().is_some_and(|configuration| configuration.temporary));
    assert_eq!(RunConfigurations::read(harness.dir.path()).unwrap().selected, None, "nothing goes to the file");
}

#[cfg(unix)]
#[test]
fn stop_and_rerun_reach_a_real_process_and_its_end_is_noticed_without_polling() {
    let mut harness = Harness::new();
    let providers: Vec<Arc<dyn zen_run::RunProvider>> =
        vec![Arc::new(harness.fake.clone()), Arc::new(zen_run::fake::ShellProvider)];
    harness.session.state_mut::<Runs>().set_providers(providers, RunEnv::default());
    let configuration = zen_run::fake::ShellProvider::configuration("server", "echo ready; sleep 60");
    let params = json!({ "configuration": configuration, "select": true });
    let created = harness.registry.dispatch(&mut harness.session, "run.configuration.create", params).unwrap();
    let id = created["id"].as_str().unwrap().to_owned();
    harness.frame();
    harness.click_button("run");
    let action = harness.take_action().unwrap();
    let first = harness.perform(action)["runId"].as_u64().unwrap();
    assert_eq!(harness.widget.state(), WidgetState::Running(id.clone(), first));

    harness.click_button("run");
    let action = harness.take_action().unwrap();
    assert_eq!(action, WidgetAction::Rerun(first, id.clone()));
    let second = harness.perform(action)["runId"].as_u64().unwrap();
    assert_ne!(second, first);
    harness.wait_ended(first);
    let state = |harness: &mut Harness, run: u64| {
        let list = harness.registry.dispatch(&mut harness.session, "run.process.list", json!({})).unwrap();
        list["runs"].as_array().unwrap().iter().find(|info| info["runId"] == run).unwrap()["state"].clone()
    };
    assert_eq!(state(&mut harness, first), "stopped", "Rerun stopped the running program");
    assert_eq!(state(&mut harness, second), "running");
    harness.frame();
    assert_eq!(harness.widget.state(), WidgetState::Running(id.clone(), second));

    harness.click_button("stop");
    let action = harness.take_action().unwrap();
    assert_eq!(action, WidgetAction::Dispatch("run.process.stop", json!({ "runId": second })));
    let WidgetAction::Dispatch(command, params) = action else { unreachable!() };
    harness.registry.dispatch(&mut harness.session, command, params).unwrap();
    harness.wait_ended(second);
    harness.frame();
    assert_eq!(harness.widget.state(), WidgetState::Selected(id), "the end is seen without reading on a timer");
    assert!(harness.widget.button("stop").is_none());
}

#[cfg(unix)]
#[test]
fn a_running_compound_shows_stop_which_stops_every_configuration_it_started() {
    let mut harness = Harness::new();
    let providers: Vec<Arc<dyn zen_run::RunProvider>> =
        vec![Arc::new(harness.fake.clone()), Arc::new(zen_run::fake::ShellProvider)];
    harness.session.state_mut::<Runs>().set_providers(providers, RunEnv::default());
    let mut ids = Vec::new();
    for name in ["server", "worker"] {
        let configuration = zen_run::fake::ShellProvider::configuration(name, "sleep 30");
        let params = json!({ "configuration": configuration });
        let created = harness.registry.dispatch(&mut harness.session, "run.configuration.create", params).unwrap();
        ids.push(created["id"].as_str().unwrap().to_owned());
    }
    let compound = json!({ "name": "all", "kind": "compound", "options": { "configurations": ids } });
    let params = json!({ "configuration": compound, "select": true });
    let created = harness.registry.dispatch(&mut harness.session, "run.configuration.create", params).unwrap();
    let id = created["id"].as_str().unwrap().to_owned();
    harness.frame();
    harness.click_button("run");
    let started = harness.take_action().unwrap();
    assert_eq!(started, WidgetAction::Dispatch("run.process.start", json!({ "configurationId": id })));
    let answer = harness.perform(started);
    assert!(matches!(harness.widget.state(), WidgetState::Running(shown, _) if shown == id), "{answer}");
    harness.click_button("run");
    assert_eq!(
        harness.take_action(),
        Some(WidgetAction::Dispatch("run.process.start", json!({ "configurationId": id }))),
        "a compound starts again as a whole"
    );
    harness.click_button("stop");
    let stop = harness.take_action().unwrap();
    assert_eq!(stop, WidgetAction::Dispatch("run.process.stop", json!({ "configurationId": id })));
    let stopped = harness.perform(stop);
    let runs: Vec<u64> = stopped["runs"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
    assert_eq!(runs.len(), 2, "{stopped}");
    for run in runs {
        harness.wait_ended(run);
    }
}
