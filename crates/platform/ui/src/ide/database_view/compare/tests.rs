use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::vec2;
use serde_json::json;
use zen_workbench::workspaces::Workspaces;

use super::super::super::workspace_host::WorkspaceView;
use super::super::DatabaseView;
use super::*;

struct Setup {
    registry: Registry,
    session: Session,
    _dir: tempfile::TempDir,
    ids: [String; 2],
}

fn sqlite(registry: &Registry, session: &mut Session, name: &str, file: PathBuf, sql: &str) -> String {
    std::fs::File::create(&file).unwrap();
    let added = registry
        .dispatch(session, "db.dataSource.add", json!({ "name": name, "driver": "sqlite", "database": file }))
        .unwrap();
    let id = added["id"].as_str().unwrap().to_owned();
    let ran =
        registry.dispatch(session, "db.query.run", json!({ "connection": id, "sql": sql, "waitMs": 30_000 })).unwrap();
    assert_eq!(ran["status"], "done", "{ran}");
    id
}

fn setup() -> Setup {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    zen_workbench::database::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    session.state_mut::<Workspaces>().set_data_dir(dir.path().join("data"));
    zen_workbench::database::install(&mut session);
    session.set_state(zen_workbench::database::Secrets(Arc::new(zen_secrets::MemoryStore::new())));
    let source = sqlite(
        &registry,
        &mut session,
        "Shop (dev)",
        dir.path().join("dev.sqlite"),
        "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT NOT NULL, email TEXT);\n\
         CREATE TABLE orders (id INTEGER PRIMARY KEY, customer INTEGER NOT NULL, total NUMERIC, status TEXT DEFAULT 'new', note TEXT);\n\
         CREATE INDEX orders_customer ON orders (customer);\n\
         CREATE VIEW big_orders AS SELECT id, total FROM orders WHERE total > 100;\n\
         INSERT INTO orders (id, customer, total, status) VALUES (1, 1, 10, 'new'), (2, 1, 250, 'paid'), (3, 2, 40, 'new'), (4, 3, 120, 'new');",
    );
    let target = sqlite(
        &registry,
        &mut session,
        "Shop (test)",
        dir.path().join("test.sqlite"),
        "CREATE TABLE orders (id INTEGER PRIMARY KEY, customer INTEGER, total NUMERIC, status TEXT);\n\
         CREATE TABLE audit (id INTEGER PRIMARY KEY, at TEXT);\n\
         CREATE VIEW big_orders AS SELECT id, total FROM orders WHERE total > 500;\n\
         INSERT INTO orders (id, customer, total, status) VALUES (1, 1, 10, 'new'), (2, 1, 250, 'new'), (9, 4, 75, 'new');",
    );
    Setup { registry, session, _dir: dir, ids: [source, target] }
}

fn settle(
    view: &mut DatabaseView,
    screen: &mut crate::offscreen::Offscreen,
    setup: &mut Setup,
    tab: &InnerTab,
    palette: &IdePalette,
    done: impl Fn(&CompareView) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(30);
    let (registry, session) = (&setup.registry, &mut setup.session);
    while !done(&view.compare) && Instant::now() < deadline {
        screen.render(|ui| view.tab(ui, palette, registry, session, tab));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(done(&view.compare), "{}", view.compare.state);
}

#[test]
fn the_compare_tab_shows_the_tree_the_ddl_side_by_side_and_the_rows_in_light_and_dark() {
    if zen_db::sqlite::process::find_program(None).is_err() {
        let _ = std::io::Write::write_all(&mut std::io::stderr(), b"skipped: the sqlite3 program is not installed\n");
        return;
    }
    let mut setup = setup();
    let [source, target] = setup.ids.clone();
    let tab: InnerTab = serde_json::from_value(
        setup
            .registry
            .dispatch(
                &mut setup.session,
                "db.compare.open",
                json!({ "source": source, "sourceSchema": "main", "target": target, "targetSchema": "main" }),
            )
            .unwrap(),
    )
    .unwrap();
    let mut view = DatabaseView::default();
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let mut screen = crate::offscreen::Offscreen::new(vec2(1280.0, 620.0), 2.0);
    let dark = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
    dark.apply(screen.context());
    {
        let (registry, session) = (&setup.registry, &mut setup.session);
        screen.render(|ui| view.tab(ui, &dark, registry, session, &tab));
    }
    assert_eq!(
        view.compare.source.connection.as_deref(),
        Some(source.as_str()),
        "the tab takes its sides from db.compare.open"
    );
    view.compare.compare(&setup.registry, &mut setup.session);
    settle(&mut view, &mut screen, &mut setup, &tab, &dark, |compare| compare.state["status"] == "done");
    let names: Vec<&str> = view.compare.state["objects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|object| object["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["audit", "customers", "orders", "big_orders"]);
    view.compare.selected = Some(("table".to_owned(), "orders".to_owned()));
    view.compare.expanded.insert("table/orders".to_owned());
    view.compare.start_data(&setup.registry, &mut setup.session, "orders");
    settle(&mut view, &mut screen, &mut setup, &tab, &dark, |compare| {
        compare.data.as_ref().is_some_and(|(_, state)| state["status"] == "done")
    });
    let counts = view.compare.data.as_ref().unwrap().1["data"]["counts"].clone();
    assert_eq!(
        (counts["same"].as_u64(), counts["changed"].as_u64(), counts["added"].as_u64(), counts["removed"].as_u64()),
        (Some(1), Some(1), Some(2), Some(1))
    );
    let ddl = view.compare.ddl.as_ref().map(|(key, _)| key.clone());
    assert_eq!(
        ddl.map(|(_, kind, key)| (kind, key)),
        Some(("table".to_owned(), "orders".to_owned())),
        "the selected object's DDL is shown"
    );
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        palette.apply(screen.context());
        let (registry, session) = (&setup.registry, &mut setup.session);
        screen.render(|ui| view.tab(ui, &palette, registry, session, &tab));
        let image = screen.frame(Vec::new(), |ui| view.tab(ui, &palette, registry, session, &tab));
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("db-compare-{theme_name}.png")), image.image()).unwrap();
        }
    }
    for id in [source, target] {
        setup
            .registry
            .dispatch(
                &mut setup.session,
                "db.connection.disconnect",
                json!({ "connection": id, "transactions": "rollback" }),
            )
            .unwrap();
    }
}

#[test]
fn a_comparison_started_by_a_command_shows_in_the_tab_with_its_sides() {
    if zen_db::sqlite::process::find_program(None).is_err() {
        let _ = std::io::Write::write_all(&mut std::io::stderr(), b"skipped: the sqlite3 program is not installed\n");
        return;
    }
    let mut setup = setup();
    let [source, target] = setup.ids.clone();
    let tab: InnerTab =
        serde_json::from_value(setup.registry.dispatch(&mut setup.session, "db.compare.open", json!({})).unwrap())
            .unwrap();
    let mut view = DatabaseView::default();
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let mut screen = crate::offscreen::Offscreen::new(vec2(1280.0, 620.0), 2.0);
    let dark = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
    dark.apply(screen.context());
    {
        let (registry, session) = (&setup.registry, &mut setup.session);
        screen.render(|ui| view.tab(ui, &dark, registry, session, &tab));
    }
    assert!(view.compare.id.is_none() && view.compare.source.connection.is_none(), "nothing to show yet");
    let side = |connection: &str| json!({ "kind": "connection", "connection": connection, "schema": "main" });
    let started = setup
        .registry
        .dispatch(&mut setup.session, "db.compare.schemas", json!({ "source": side(&source), "target": side(&target) }))
        .unwrap();
    settle(&mut view, &mut screen, &mut setup, &tab, &dark, |compare| compare.state["status"] == "done");
    assert_eq!(view.compare.id, started["id"].as_u64(), "the tab shows the command's comparison");
    assert_eq!(
        (view.compare.source.connection.as_deref(), view.compare.target.connection.as_deref()),
        (Some(source.as_str()), Some(target.as_str()))
    );
    assert_eq!((view.compare.source.schema.as_str(), view.compare.target.schema.as_str()), ("main", "main"));
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        palette.apply(screen.context());
        let (registry, session) = (&setup.registry, &mut setup.session);
        screen.render(|ui| view.tab(ui, &palette, registry, session, &tab));
        let image = screen.frame(Vec::new(), |ui| view.tab(ui, &palette, registry, session, &tab));
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("b6-compare-schemas-{theme_name}.png")), image.image())
                .unwrap();
        }
    }

    let table = |connection: &str| json!({ "connection": connection, "table": "orders" });
    let rows = setup
        .registry
        .dispatch(&mut setup.session, "db.compare.data", json!({ "source": table(&target), "target": table(&source) }))
        .unwrap();
    settle(&mut view, &mut screen, &mut setup, &tab, &dark, |compare| {
        compare.data.as_ref().is_some_and(|(_, state)| state["status"] == "done")
    });
    assert_eq!(view.compare.data.as_ref().map(|(id, _)| *id), rows["id"].as_u64());
    assert_eq!(
        (view.compare.source.connection.as_deref(), view.compare.target.connection.as_deref()),
        (Some(target.as_str()), Some(source.as_str())),
        "the pickers follow the data comparison's tables"
    );
    assert!(view.compare.id.is_none(), "the schema comparison of other sides is no longer shown");
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        palette.apply(screen.context());
        let (registry, session) = (&setup.registry, &mut setup.session);
        let image = screen.frame(Vec::new(), |ui| view.tab(ui, &palette, registry, session, &tab));
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("b6-compare-data-{theme_name}.png")), image.image()).unwrap();
        }
    }
    for id in [source, target] {
        setup
            .registry
            .dispatch(
                &mut setup.session,
                "db.connection.disconnect",
                json!({ "connection": id, "transactions": "rollback" }),
            )
            .unwrap();
    }
}
