use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use egui::{Event, Key, vec2};
use serde_json::json;
use zen_core::{Registry, Session};
use zen_db::history::{HistorySink, Provenance, Ran, Source};
use zen_db::schema::{Catalog, Column, Level, Object, ObjectKind, Schema, Snapshot};
use zen_workbench::workspaces::Workspaces;

use super::*;

struct Setup {
    registry: Registry,
    session: Session,
    _dir: tempfile::TempDir,
    id: String,
}

fn object(kind: ObjectKind, name: &str, columns: &[&str]) -> Object {
    let mut object = Object::new(kind, name);
    object.detailed = true;
    object.columns = columns
        .iter()
        .zip(1..)
        .map(|(name, position)| Column {
            name: (*name).into(),
            position,
            type_name: "bigint".into(),
            ..Column::default()
        })
        .collect();
    object
}

fn snapshot(id: &str) -> Snapshot {
    let objects = vec![
        object(ObjectKind::Table, "orders", &["id", "customer_id", "order_total"]),
        object(ObjectKind::Table, "order_items", &["order_id", "quantity"]),
        object(ObjectKind::View, "big_orders", &["id"]),
        object(ObjectKind::Function, "recalc_orders", &[]),
        object(ObjectKind::Table, "customers", &["id", "name"]),
    ];
    Snapshot {
        taken: 1,
        catalogs: vec![Catalog {
            name: "shop".into(),
            current: true,
            schemas_read: true,
            schemas: vec![Schema { name: "public".into(), level: Level::Details, objects, ..Schema::default() }],
            ..Catalog::default()
        }],
        ..Snapshot::new(id, "postgresql")
    }
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
    session.set_state(SearchHistory::default());
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    session.state_mut::<Workspaces>().set_data_dir(dir.path().join("data"));
    zen_workbench::database::install(&mut session);
    session.set_state(zen_workbench::database::Secrets(Arc::new(zen_secrets::MemoryStore::new())));
    let drivers: Vec<Arc<dyn zen_db::driver::Driver>> = vec![Arc::new(zen_db::postgres::Postgres)];
    session.state_mut::<zen_workbench::database::Database>().set_drivers(drivers);
    let added = registry
        .dispatch(
            &mut session,
            "db.dataSource.add",
            json!({ "name": "Shop", "url": "postgresql://ann@db.example.com:5432/shop", "type": "prod" }),
        )
        .unwrap();
    let id = added["id"].as_str().unwrap().to_owned();
    let file = dir.path().join(format!("data/caches/databases/global/{id}.snapshot"));
    zen_db::schema::write_snapshot(&file, &snapshot(&id)).unwrap();
    let journal = zen_workbench::database::history::journal(&session).unwrap();
    journal.record(Ran {
        connection: id.clone(),
        context: zen_db::driver::ContextKind::Main,
        schema: Some("public".into()),
        text: "SELECT * FROM orders WHERE order_total > 100".into(),
        started: SystemTime::now(),
        duration: Duration::from_millis(4),
        rows: Some(3),
        affected: None,
        error: None,
        provenance: Provenance::of(Source::Statement),
        params: Vec::new(),
    });
    assert!(journal.flush(Duration::from_secs(10)));
    let deadline = Instant::now() + Duration::from_secs(10);
    while registry.dispatch(&mut session, "db.object.goTo", json!({ "query": "x" })).unwrap()["loading"] == true {
        assert!(Instant::now() < deadline, "the snapshot was never read");
        std::thread::sleep(Duration::from_millis(10));
    }
    Setup { registry, session, _dir: dir, id }
}

#[test]
fn the_database_tab_lists_objects_columns_and_history_in_light_and_dark_and_enter_opens_the_object() {
    let mut s = setup();
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let mut popup = EverywherePopup::default();
    let mut picks = Vec::new();
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        let mut screen = crate::offscreen::Offscreen::new(vec2(900.0, 520.0), 2.0);
        palette.apply(screen.context());
        screen.set_background(palette.editor_background);
        let Setup { registry, session, .. } = &mut s;
        popup = EverywherePopup::default();
        popup.toggle(Tab::DATABASE);
        screen.step(Vec::new(), |ui| picks.extend(popup.show(ui, registry, session, &palette, &|_| None)));
        screen.step(vec![Event::Text("order".to_owned())], |ui| {
            picks.extend(popup.show(ui, registry, session, &palette, &|_| None));
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            screen.step(Vec::new(), |ui| picks.extend(popup.show(ui, registry, session, &palette, &|_| None)));
            let answer = popup.answer(session);
            if !answer.pending && !answer.held {
                break;
            }
            assert!(Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut draw = |ui: &mut Ui| picks.extend(popup.show(ui, registry, session, &palette, &|_| None));
        if let Some(dir) = &snapshots {
            let image = screen.render(&mut draw);
            crate::offscreen::write_png(&dir.join(format!("se-database-{theme_name}.png")), &image).unwrap();
        }
        let tree = screen.accessibility_tree(&mut draw);
        let labels: Vec<String> = tree.nodes.iter().filter_map(|(_, node)| node.label().map(str::to_owned)).collect();
        assert!(labels.iter().any(|label| label == "Database"), "the tab: {labels:?}");
        let answer = popup.answer(session);
        let names: Vec<(&str, &str, Option<&str>)> =
            answer.items().map(|item| (item.name.as_str(), item.detail.as_str(), item.hint.as_deref())).collect();
        assert_eq!(names[0], ("order_items", "public · Shop", Some("Table")), "prefix matches first: {names:?}");
        assert_eq!(names[1], ("orders", "public · Shop", Some("Table")), "objects before columns: {names:?}");
        assert!(names.contains(&("recalc_orders", "public · Shop", Some("Function"))), "{names:?}");
        assert!(names.contains(&("order_total", "public.orders · Shop", Some("Column"))), "{names:?}");
        assert!(names.iter().any(|(_, _, hint)| *hint == Some("History")), "{names:?}");
    }
    let Setup { registry, session, id, .. } = &mut s;
    let palette = IdePalette::zen_dark();
    let mut screen = crate::offscreen::Offscreen::new(vec2(900.0, 520.0), 1.0);
    screen.step(key(Key::Enter), |ui| picks.extend(popup.show(ui, registry, session, &palette, &|_| None)));
    let Some(Pick::Action(binding)) = picks.pop() else { panic!("Enter chose nothing") };
    assert_eq!(binding.command, "db.object.open");
    let opened = registry.dispatch(session, &binding.command, binding.params).unwrap();
    assert_eq!(opened["tab"]["id"], format!("object:{id}:shop/public/tables/order_items"));
}

fn key(key: Key) -> Vec<Event> {
    [true, false]
        .into_iter()
        .map(|pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE })
        .collect()
}
