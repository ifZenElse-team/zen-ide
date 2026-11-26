use std::path::PathBuf;
use std::sync::Arc;

use egui::{Event, PointerButton, Pos2, pos2, vec2};
use zen_workbench::workspaces::Workspaces;

use super::*;
use crate::offscreen::Offscreen;

struct Setup {
    registry: Registry,
    session: Session,
    _dir: tempfile::TempDir,
    path: PathBuf,
    cache: String,
}

fn setup() -> Setup {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir_all(project.join("queries")).unwrap();
    let path = project.join("queries/report.sql");
    std::fs::write(&path, "SET a 1\nGET a\n").unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    zen_workbench::database::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    session.state_mut::<Workspaces>().set_data_dir(dir.path().join("data"));
    zen_workbench::database::install(&mut session);
    session.set_state(zen_workbench::database::Secrets(Arc::new(zen_secrets::MemoryStore::new())));
    let drivers: Vec<Arc<dyn zen_db::driver::Driver>> = vec![Arc::new(zen_db::memory::MemoryDriver::new(false))];
    session.state_mut::<zen_workbench::database::Database>().set_drivers(drivers);
    let added = registry
        .dispatch(
            &mut session,
            "db.dataSource.add",
            json!({ "name": "Cache", "driver": zen_db::memory::ID, "password": "pw", "type": "test" }),
        )
        .unwrap();
    let cache = added["id"].as_str().unwrap().to_owned();
    registry
        .dispatch(
            &mut session,
            "db.dataSource.add",
            json!({ "name": "Reports", "driver": zen_db::memory::ID, "password": "pw" }),
        )
        .unwrap();
    registry.dispatch(&mut session, "file.document.open", json!({ "path": path })).unwrap();
    Setup { registry, session, _dir: dir, path, cache }
}

fn center_of(screen: &mut Offscreen, draw: &mut impl FnMut(&mut Ui), label: &str) -> Pos2 {
    let tree = screen.accessibility_tree(draw);
    let bounds = tree
        .nodes
        .iter()
        .find(|(_, node)| node.label().is_some_and(|known| known == label))
        .and_then(|(_, node)| node.bounds())
        .unwrap_or_else(|| {
            let labels: Vec<&str> = tree.nodes.iter().filter_map(|(_, node)| node.label()).collect();
            panic!("no {label} in {labels:?}")
        });
    pos2((bounds.x0 + bounds.x1) as f32 / 2.0, (bounds.y0 + bounds.y1) as f32 / 2.0)
}

fn click(screen: &mut Offscreen, draw: &mut impl FnMut(&mut Ui), at: Pos2) {
    let press = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    screen.step(vec![Event::PointerMoved(at)], &mut *draw);
    screen.step(vec![press(true)], &mut *draw);
    screen.step(vec![press(false)], &mut *draw);
}

fn snapshot(screen: &mut Offscreen, draw: &mut impl FnMut(&mut Ui), name: &str) {
    let image = screen.render(draw);
    if let Some(dir) = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from) {
        crate::offscreen::write_png(&dir.join(format!("{name}.png")), image.image()).unwrap();
    }
}

fn themed(screen: &mut Offscreen, draw: &mut impl FnMut(&mut Ui), name: &str, palette: &std::cell::Cell<IdePalette>) {
    for (theme_name, theme) in
        [("light", zen_theme::ResolvedTheme::zen_light()), ("dark", zen_theme::ResolvedTheme::zen_dark())]
    {
        let colors = IdePalette::from_theme(&theme);
        colors.apply(screen.context());
        palette.set(colors);
        snapshot(screen, draw, &format!("{name}-{theme_name}"));
    }
}

#[test]
fn the_bar_asks_for_a_connection_binds_the_file_and_runs_it_in_the_workspace() {
    let mut setup = setup();
    let mut bar = ScriptBar::default();
    let mut screen = Offscreen::new(vec2(900.0, 240.0), 2.0);
    let palette = std::cell::Cell::new(IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark()));
    let path = setup.path.clone();
    let (registry, session) = (&setup.registry, &mut setup.session);
    let mut draw = |ui: &mut Ui| {
        let p = palette.get();
        ui.painter().rect_filled(ui.max_rect(), 0, p.editor_background);
        bar.show(ui, &p, registry, session, &path);
    };
    themed(&mut screen, &mut draw, "sql-script-bar-unbound", &palette);

    let execute = center_of(&mut screen, &mut draw, "Execute SQL Script");
    click(&mut screen, &mut draw, execute);
    screen.step(vec![Event::PointerMoved(pos2(880.0, 230.0))], &mut draw);
    themed(&mut screen, &mut draw, "sql-script-bar-choose", &palette);
    let cache = center_of(&mut screen, &mut draw, "Cache");
    click(&mut screen, &mut draw, cache);
    themed(&mut screen, &mut draw, "sql-script-bar-bound", &palette);
    drop(draw);

    assert!(bar.pending.is_none(), "the run went on once the connection was chosen");
    let state = setup.registry.dispatch(&mut setup.session, "db.script.state", json!({ "path": setup.path })).unwrap();
    assert_eq!(state["connection"], setup.cache.as_str(), "{state}");
    assert_eq!(state["inWorkspace"], true, "the run opened the file in the Database workspace");
    let console =
        setup.registry.dispatch(&mut setup.session, "db.console.state", json!({ "path": setup.path })).unwrap();
    let query = console["last"]["query"].clone();
    let done = setup
        .registry
        .dispatch(&mut setup.session, "db.query.state", json!({ "query": query, "waitMs": 10_000 }))
        .unwrap();
    assert_eq!(done["status"], "done", "{done}");
    assert_eq!(done["statements"].as_array().map(Vec::len), Some(2), "Execute Script ran the whole file");
    let host =
        setup.registry.dispatch(&mut setup.session, "workspace.host.state", json!({ "id": "database" })).unwrap();
    assert_eq!(host["expanded"], true);
    assert_eq!(std::fs::read_to_string(&setup.path).unwrap(), "SET a 1\nGET a\n", "the binding is not in the file");
}

#[test]
fn a_refused_run_from_the_gutter_or_keys_opens_the_chooser_and_closing_it_drops_the_run() {
    let mut setup = setup();
    let mut bar = ScriptBar::default();
    let mut screen = Offscreen::new(vec2(900.0, 240.0), 1.0);
    let p = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_light());
    p.apply(screen.context());
    let refused =
        setup.registry.dispatch(&mut setup.session, "db.script.run", json!({ "path": setup.path })).unwrap_err();
    assert!(not_bound(&refused));
    bar.ask_binding(json!({ "path": setup.path, "offset": 8 }));
    let path = setup.path.clone();
    let (registry, session) = (&setup.registry, &mut setup.session);
    let mut draw = |ui: &mut Ui| bar.show(ui, &p, registry, session, &path);
    screen.render(&mut draw);
    let tree = screen.accessibility_tree(&mut draw);
    let labels: Vec<&str> = tree.nodes.iter().filter_map(|(_, node)| node.label()).collect();
    assert!(labels.contains(&"Cache") && labels.contains(&"Reports"), "the chooser lists the connections: {labels:?}");
    click(&mut screen, &mut draw, pos2(880.0, 230.0));
    screen.render(&mut draw);
    drop(draw);
    assert!(bar.pending.is_none(), "closing the chooser drops the run");
    let state = setup.registry.dispatch(&mut setup.session, "db.script.state", json!({ "path": setup.path })).unwrap();
    assert_eq!(state["bound"], false, "nothing is bound by itself");
    let status = setup.registry.dispatch(&mut setup.session, "db.connection.status", json!({})).unwrap();
    assert!(
        status["connections"].as_array().into_iter().flatten().all(|connection| connection["state"] == "disconnected"),
        "nothing connects by itself: {status}"
    );
}
