use egui::{Event, Key, Modifiers, Pos2};
use zen_workbench::local_history::{Limits, LocalHistory};

use super::*;

struct Harness {
    ctx: egui::Context,
    registry: Registry,
    session: Session,
    dialog: LocalHistoryDialog,
    outcomes: Vec<HistoryOutcome>,
    project: tempfile::TempDir,
    _data: tempfile::TempDir,
}

impl Harness {
    fn new() -> Self {
        zen_i18n::register(crate::toolbox::LOCALES);
        zen_i18n::register(zen_workbench::LOCALES);
        zen_i18n::set_language(Some("en"));
        let project = tempfile::tempdir().unwrap();
        let data = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(project.path().join("pkg")).unwrap();
        std::fs::write(project.path().join("pkg/main.go"), "package main\n").unwrap();
        let mut registry = Registry::new();
        zen_workbench::register(&mut registry).unwrap();
        let mut session = Session::new();
        session.set_state(LocalHistory::open(data.path().join("local-history"), Limits::default()));
        registry.dispatch(&mut session, "file.project.open", json!({ "path": project.path() })).unwrap();
        let ctx = egui::Context::default();
        theme::install(&ctx);
        Self {
            ctx,
            registry,
            session,
            dialog: LocalHistoryDialog::default(),
            outcomes: Vec::new(),
            project,
            _data: data,
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.project.path().join(name)
    }

    fn save(&mut self, text: &str) {
        let path = self.path("pkg/main.go");
        self.registry.dispatch(&mut self.session, "file.document.open", json!({ "path": path })).unwrap();
        let workbench = self.session.state_mut::<Workbench>();
        let document = workbench.active_document_mut().unwrap();
        let length = document.text().chars().count();
        document.replace_ranges(vec![(0..length, text.to_owned())]);
        self.registry.dispatch(&mut self.session, "file.document.save", json!({})).unwrap();
    }

    fn frame(&mut self, events: Vec<Event>) {
        let input = egui::RawInput {
            events,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))),
            ..Default::default()
        };
        let Self { ctx, registry, session, dialog, outcomes, .. } = self;
        let mut output = ctx.run_ui(input, |ui| {
            outcomes.extend(dialog.update(ui, registry, session, &IdePalette::zen_dark(), &|_| None));
        });
        output.textures_delta.clear();
    }

    fn key(&mut self, key: Key) {
        let event =
            |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        self.frame(vec![event(true)]);
        self.frame(vec![event(false)]);
    }

    fn window(&self) -> &HistoryWindow {
        self.dialog.window.as_ref().expect("Show History is open")
    }
}

#[test]
fn show_history_lists_the_versions_and_compares_the_newest_with_the_current_text() {
    let mut harness = Harness::new();
    harness.save("package main\n\nfunc one() {}\n");
    harness.save("package main\n\nfunc two() {}\n");
    harness.dialog.open(harness.path("pkg/main.go"));
    harness.frame(Vec::new());
    let window = harness.window();
    assert!(!window.folder);
    let titles: Vec<&str> = window.versions.iter().map(|version| version.title.as_str()).collect();
    assert_eq!(titles, ["Save", "Save", "External change"]);
    assert_eq!(window.selected, Some(0));
    let diff = window.diff.as_ref().expect("the newest version is compared");
    assert!(diff.view.hunks.is_empty(), "the newest save is the current text");
    assert!(diff.view.before_title.starts_with("Today "), "{}", diff.view.before_title);

    harness.key(Key::ArrowDown);
    harness.key(Key::ArrowDown);
    let window = harness.window();
    assert_eq!(window.selected, Some(2));
    let diff = window.diff.as_ref().unwrap();
    assert_eq!(
        (diff.view.before.as_str(), diff.view.after.as_str()),
        ("package main\n", "package main\n\nfunc two() {}\n")
    );
}

#[test]
fn revert_restores_the_chosen_version_and_closes_with_a_notice() {
    let mut harness = Harness::new();
    harness.save("package main\n\nfunc one() {}\n");
    harness.save("package main\n\nfunc two() {}\n");
    harness.dialog.open(harness.path("pkg/main.go"));
    harness.frame(Vec::new());
    let (registry, session) = (&harness.registry, &mut harness.session);
    assert!(harness.dialog.act(&HistoryAction::Select(1), registry, session).is_empty());
    let outcomes = harness.dialog.act(&HistoryAction::Revert, registry, session);
    assert_eq!(outcomes, [HistoryOutcome::Notice("1 file reverted".to_owned()), HistoryOutcome::Closed]);
    assert_eq!(std::fs::read_to_string(harness.path("pkg/main.go")).unwrap(), "package main\n\nfunc one() {}\n");
    assert!(!harness.dialog.is_open());
}

#[test]
fn folder_history_lists_the_changed_files() {
    let mut harness = Harness::new();
    harness.save("package main\n\nfunc one() {}\n");
    harness.dialog.open(harness.path("pkg"));
    harness.frame(Vec::new());
    let (registry, session) = (&harness.registry, &mut harness.session);
    harness.dialog.act(&HistoryAction::Select(1), registry, session);
    let window = harness.window();
    assert!(window.folder);
    assert_eq!(window.files.iter().map(|file| file.relative.as_str()).collect::<Vec<_>>(), ["pkg/main.go"]);
    assert_eq!(window.files[0].status, FileStatus::Modified);
    assert_eq!(window.selected_file, Some(0));
    assert_eq!(window.diff.as_ref().unwrap().view.before, "package main\n");
}

#[test]
fn escape_closes_and_an_unknown_place_is_a_notice() {
    let mut harness = Harness::new();
    harness.dialog.open(harness.path("pkg/main.go"));
    harness.frame(Vec::new());
    assert!(harness.window().versions.is_empty());
    harness.key(Key::Escape);
    assert!(!harness.dialog.is_open());
    assert_eq!(harness.outcomes, [HistoryOutcome::Closed]);

    harness.outcomes.clear();
    harness.dialog.open(PathBuf::from("/elsewhere/file.txt"));
    harness.frame(Vec::new());
    assert!(!harness.dialog.is_open());
    assert!(
        matches!(harness.outcomes.as_slice(), [HistoryOutcome::Notice(message)] if message.contains("not in the project"))
    );
}

#[test]
fn put_label_adds_a_label_to_the_history() {
    let mut harness = Harness::new();
    harness.save("package main\n// 1\n");
    harness.dialog.open_label();
    harness.frame(Vec::new());
    assert!(harness.dialog.is_label_open());
    harness.frame(vec![Event::Text("Before the demo".to_owned())]);
    harness.key(Key::Enter);
    assert!(!harness.dialog.is_label_open());
    let path = harness.path("pkg/main.go");
    let list =
        harness.registry.dispatch(&mut harness.session, "localHistory.versions.list", json!({ "path": path })).unwrap();
    assert_eq!(list["versions"][0]["title"], "Before the demo");
    assert_eq!(list["versions"][0]["kind"], "label");
}

#[test]
fn dates_are_today_yesterday_or_the_day() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::set_language(Some("en"));
    let now = zen_workbench::local_history::now();
    assert!(when(now, 0).starts_with("Today "));
    assert!(when(now - 24 * 3600 * 1000, 0).starts_with("Yesterday "));
    assert_eq!(when(1_760_000_000_000, 0), "09.10.2025 08:53");
    assert_eq!(civil(0), (1970, 1, 1));
}
