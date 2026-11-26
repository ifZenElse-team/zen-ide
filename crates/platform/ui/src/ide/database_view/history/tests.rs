use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use egui::{Event, PointerButton, pos2, vec2};
use serde_json::json;
use zen_db::driver::{ContextKind, DbError, ErrorKind};
use zen_db::history::{HistorySink as _, Provenance, Ran, Source};
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

fn ran(connection: &str, text: &str, source: Source, minutes_ago: u64) -> Ran {
    Ran {
        connection: connection.to_owned(),
        context: ContextKind::Console("console.sql".to_owned()),
        schema: Some("public".to_owned()),
        text: text.to_owned(),
        started: SystemTime::now() - Duration::from_secs(minutes_ago * 60),
        duration: Duration::from_millis(14 + minutes_ago),
        rows: None,
        affected: None,
        error: None,
        provenance: Provenance { source, agent: false, place: Some("console.sql".to_owned()), confirmed: false },
        params: Vec::new(),
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
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    session.state_mut::<Workspaces>().set_data_dir(dir.path().join("data"));
    zen_workbench::database::install(&mut session);
    session.set_state(zen_workbench::database::Secrets(Arc::new(zen_secrets::MemoryStore::new())));
    let shop = registry
        .dispatch(
            &mut session,
            "db.dataSource.add",
            json!({ "name": "Shop", "url": "postgresql://ann@db.example.com:5432/shop", "type": "prod", "auth": "none" }),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let cache = registry
        .dispatch(
            &mut session,
            "db.dataSource.add",
            json!({ "name": "Cache", "url": "redis://cache.example.com:6379/0", "type": "test", "auth": "none" }),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let journal = zen_workbench::database::history::journal(&session).unwrap();
    let mut read = ran(
        &shop,
        "SELECT id, name, email\nFROM customers\nWHERE created_at > now() - interval '7 days'",
        Source::Statement,
        42,
    );
    read.rows = Some(128);
    journal.record(read);
    let mut script = ran(&shop, "UPDATE orders SET status = 'shipped' WHERE id = 1042", Source::Script, 30);
    script.affected = Some(1);
    journal.record(script);
    let mut failed = ran(&shop, "SELECT * FROM order_lines WHERE order = 7", Source::Statement, 21);
    failed.error = Some(DbError::new(ErrorKind::Server, "syntax error at or near \"order\"").with_code("42601"));
    journal.record(failed);
    let mut filter = ran(&shop, "SELECT * FROM orders WHERE (total > 100) ORDER BY id", Source::Filter, 15);
    filter.rows = Some(37);
    filter.context = ContextKind::Main;
    journal.record(filter);
    let mut agent = ran(&shop, "SELECT count(*) FROM orders", Source::Query, 9);
    agent.rows = Some(1);
    agent.provenance =
        Provenance { source: Source::Query, agent: true, place: Some("agent".to_owned()), confirmed: false };
    journal.record(agent);
    let mut key = ran(&cache, "HGETALL user:3", Source::Statement, 5);
    key.rows = Some(4);
    journal.record(key);
    let mut cancelled = ran(&shop, "SELECT pg_sleep(60)", Source::Statement, 2);
    cancelled.error = Some(DbError::new(ErrorKind::Cancelled, ""));
    journal.record(cancelled);
    journal.record(ran(&shop, "COMMIT", Source::Transaction, 1));
    assert!(journal.flush(Duration::from_secs(10)));
    Setup { registry, session, _dir: dir, shop }
}

fn shown(view: &DatabaseView) -> Vec<String> {
    view.history.entries().iter().map(|entry| entry["text"].as_str().unwrap().to_owned()).collect()
}

fn click(at: egui::Pos2) -> Vec<Event> {
    let press = |pressed| Event::PointerButton {
        pos: at,
        button: PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    vec![Event::PointerMoved(at), press(true), press(false)]
}

#[test]
fn the_history_tab_lists_filters_and_shows_a_statement_in_light_and_dark() {
    let mut setup = setup();
    let tab: InnerTab =
        serde_json::from_value(setup.registry.dispatch(&mut setup.session, "db.history.open", json!({})).unwrap())
            .unwrap();
    let mut view = DatabaseView::default();
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let mut screen = crate::offscreen::Offscreen::new(vec2(1180.0, 520.0), 2.0);
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        palette.apply(screen.context());
        let (registry, session) = (&setup.registry, &mut setup.session);
        let mut frame = |screen: &mut crate::offscreen::Offscreen, events: Vec<Event>| {
            screen.frame(events, |ui| view.tab(ui, &palette, registry, session, &tab))
        };
        frame(&mut screen, Vec::new());
        frame(&mut screen, click(pos2(600.0, BAR + ROW * 5.0 + ROW / 2.0)));
        let image = frame(&mut screen, vec![Event::PointerMoved(pos2(5.0, 515.0))]);
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("db-history-{theme_name}.png")), image.image()).unwrap();
        }
    }
    assert_eq!(
        shown(&view),
        [
            "COMMIT",
            "SELECT pg_sleep(60)",
            "HGETALL user:3",
            "SELECT count(*) FROM orders",
            "SELECT * FROM orders WHERE (total > 100) ORDER BY id",
            "SELECT * FROM order_lines WHERE order = 7",
            "UPDATE orders SET status = 'shipped' WHERE id = 1042",
            "SELECT id, name, email\nFROM customers\nWHERE created_at > now() - interval '7 days'",
        ]
    );
    let detail = view.history.detail.clone().expect("a click shows the statement whole");
    assert_eq!(detail["text"], "SELECT * FROM order_lines WHERE order = 7");
    assert!(detail["error"].as_str().unwrap().contains("syntax error"), "{detail}");

    let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
    let (registry, session) = (&setup.registry, &mut setup.session);
    let mut draw = |view: &mut DatabaseView, screen: &mut crate::offscreen::Offscreen| {
        screen.render(|ui| view.tab(ui, &palette, registry, session, &tab));
    };
    view.history.query = "ORDERS select".to_owned();
    draw(&mut view, &mut screen);
    assert_eq!(shown(&view), ["SELECT count(*) FROM orders", "SELECT * FROM orders WHERE (total > 100) ORDER BY id"]);
    assert!(view.history.detail.is_none(), "a statement no longer listed is no longer shown");
    view.history.query.clear();
    view.history.kind = Kind::Agent;
    draw(&mut view, &mut screen);
    assert_eq!(shown(&view), ["SELECT count(*) FROM orders"]);
    view.history.kind = Kind::All;
    view.history.ending = Ending::Failed;
    draw(&mut view, &mut screen);
    assert_eq!(shown(&view), ["SELECT * FROM order_lines WHERE order = 7"]);
    view.history.ending = Ending::Any;
    view.history.connection = Some(setup.shop.clone());
    view.history.when = When::Week;
    draw(&mut view, &mut screen);
    assert_eq!(shown(&view).len(), 7, "{:?}", shown(&view));
    assert!(!shown(&view).contains(&"HGETALL user:3".to_owned()));

    let id = detail["id"].as_u64().unwrap();
    let opened = setup.registry.dispatch(&mut setup.session, "db.history.openInConsole", json!({ "id": id })).unwrap();
    let text = std::fs::read_to_string(opened["path"].as_str().unwrap()).unwrap();
    assert_eq!(text, "SELECT * FROM order_lines WHERE order = 7");
    let status = setup.registry.dispatch(&mut setup.session, "db.connection.status", json!({})).unwrap();
    assert!(
        status["connections"].as_array().unwrap().iter().all(|connection| connection["state"] == "disconnected"),
        "drawing and opening connect nowhere"
    );
}

#[test]
fn the_history_tab_takes_its_search_from_db_history_open_and_says_when_nothing_matches() {
    let mut setup = setup();
    let tab: InnerTab = serde_json::from_value(
        setup.registry.dispatch(&mut setup.session, "db.history.open", json!({ "query": "no such words" })).unwrap(),
    )
    .unwrap();
    let mut view = DatabaseView::default();
    let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_light());
    let mut screen = crate::offscreen::Offscreen::new(vec2(900.0, 300.0), 1.0);
    let (registry, session) = (&setup.registry, &mut setup.session);
    screen.render(|ui| view.tab(ui, &palette, registry, session, &tab));
    assert_eq!(view.history.query, "no such words");
    assert!(shown(&view).is_empty());
    assert_eq!(view.history.empty_text(), "No statements match the search and filters.");
}
