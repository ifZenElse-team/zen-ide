use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Event, PointerButton, vec2};
use serde_json::json;
use zen_workbench::workspaces::Workspaces;

use super::super::super::workspace_host::WorkspaceView;
use super::super::DatabaseView;
use super::*;

struct Setup {
    registry: Registry,
    session: Session,
    _dir: tempfile::TempDir,
    shop: String,
}

fn setup() -> Option<Setup> {
    if zen_db::sqlite::process::find_program(None).is_err() {
        let _ = std::io::Write::write_all(&mut std::io::stderr(), b"skipped: the sqlite3 program is not installed\n");
        return None;
    }
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
    let file = dir.path().join("shop.db");
    std::fs::File::create(&file).unwrap();
    let shop = registry
        .dispatch(
            &mut session,
            "db.dataSource.add",
            json!({ "name": "Shop", "driver": "sqlite", "database": file.display().to_string(), "type": "prod" }),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let mut sql = String::from(
        "CREATE TABLE customers (id INTEGER PRIMARY KEY, name TEXT, email TEXT, city TEXT);\n\
         CREATE TABLE orders (id INTEGER PRIMARY KEY, customer INTEGER, note TEXT);\n\
         CREATE TABLE products (id INTEGER PRIMARY KEY, title TEXT, sku TEXT);\n\
         CREATE TABLE audit_log (id INTEGER PRIMARY KEY, message TEXT);\n",
    );
    for n in 1..=140 {
        let mail = if n % 3 == 0 { format!("user{n}@zen.dev") } else { format!("user{n}@example.org") };
        sql.push_str(&format!("INSERT INTO customers (name, email, city) VALUES ('User {n}', '{mail}', 'Zenica');\n"));
    }
    sql.push_str("INSERT INTO orders (customer, note) VALUES (3, 'gift for zen garden'), (4, 'rush');\n");
    sql.push_str("INSERT INTO products (title, sku) VALUES ('Tea', 'T-1'), ('Mat', 'M-2');\n");
    sql.push_str("INSERT INTO audit_log (message) VALUES ('login from zen.dev'), ('export');\n");
    let ran = registry
        .dispatch(
            &mut session,
            "db.query.run",
            json!({ "connection": shop, "sql": sql, "waitMs": 30_000, "confirmed": null }),
        )
        .or_else(|error| match error {
            zen_core::DispatchError::Failed { error, .. } if error.code.as_deref() == Some("confirmExecute") => {
                let retry = error.data.unwrap()["retry"].clone();
                registry.dispatch(&mut session, retry["command"].as_str().unwrap(), retry["params"].clone())
            }
            other => Err(other),
        })
        .unwrap();
    assert_eq!(ran["status"], "done", "{ran}");
    registry
        .dispatch(
            &mut session,
            "db.schema.refresh",
            json!({ "connection": shop, "path": ["shop.db", "main"], "force": true }),
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        zen_workbench::database::connections::step(&mut session);
        let children = registry
            .dispatch(
                &mut session,
                "db.schema.children",
                json!({ "connection": shop, "path": ["shop.db", "main", "tables"] }),
            )
            .unwrap();
        if children["nodes"].as_array().unwrap().iter().any(|node| node["label"] == "customers") {
            break;
        }
        assert!(Instant::now() < deadline, "{children}");
        std::thread::sleep(Duration::from_millis(20));
    }
    Some(Setup { registry, session, _dir: dir, shop })
}

fn double_click(at: egui::Pos2) -> Vec<Event> {
    let press = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    vec![Event::PointerMoved(at), press(true), press(false), press(true), press(false)]
}

fn settle(view: &mut DatabaseView, screen: &mut crate::offscreen::Offscreen, setup: &mut Setup, tab: &InnerTab) {
    let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        std::thread::sleep(POLL);
        screen.render(|ui| view.tab(ui, &palette, &setup.registry, &mut setup.session, tab));
        if view.find.state.is_some() && !view.find.running() {
            return;
        }
        assert!(Instant::now() < deadline, "{:?}", view.find.state);
    }
}

#[test]
fn the_search_tab_finds_text_shows_tables_and_opens_their_rows_in_light_and_dark() {
    let Some(mut setup) = setup() else { return };
    let tab: InnerTab = serde_json::from_value(
        setup
            .registry
            .dispatch(
                &mut setup.session,
                "db.search.open",
                json!({ "connection": setup.shop, "schema": "main", "text": "zen" }),
            )
            .unwrap(),
    )
    .unwrap();
    let mut view = DatabaseView::default();
    let mut screen = crate::offscreen::Offscreen::new(vec2(1180.0, 420.0), 2.0);
    let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
    screen.render(|ui| view.tab(ui, &palette, &setup.registry, &mut setup.session, &tab));
    assert_eq!(view.find.text, "zen", "db.search.open hands its text over");
    assert_eq!(view.find.schema.as_deref(), Some("main"));
    view.find.metadata = true;
    view.find.start(&setup.registry, &mut setup.session);
    assert!(view.find.error.is_none(), "{:?}", view.find.error);
    settle(&mut view, &mut screen, &mut setup, &tab);
    let names: Vec<String> =
        view.find.tables().iter().map(|table| table["table"].as_str().unwrap().to_owned()).collect();
    assert_eq!(names, ["audit_log", "customers", "orders"]);

    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        palette.apply(screen.context());
        view.find.selected = Some("customers".to_owned());
        let image = screen.render(|ui| view.tab(ui, &palette, &setup.registry, &mut setup.session, &tab));
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("db-search-{theme_name}.png")), image.image()).unwrap();
        }
    }

    let warnings = view.find.targets()["warnings"].clone();
    assert_eq!(warnings[0]["code"], "production", "{warnings}");
    let top = BAR * 3.0 + 24.0 + 28.0;
    let customers = pos2(400.0, top + ROW + ROW / 2.0);
    screen.frame(double_click(customers), |ui| view.tab(ui, &palette, &setup.registry, &mut setup.session, &tab));
    assert!(view.find.error.is_none(), "{:?}", view.find.error);
    let active = setup.session.state::<Workspaces>().unwrap().active_tab("database").unwrap().clone();
    assert_eq!(active.kind, "object", "the table opened: {active:?}");
    assert_eq!(active.data["path"], json!(["shop.db", "main", "tables", "customers"]));
    let filter = setup.registry.dispatch(&mut setup.session, "db.object.state", json!({ "tab": active.id })).unwrap();
    assert!(filter["data"]["statement"].as_str().unwrap().contains("LIKE '%zen%'"), "{filter}");
}
