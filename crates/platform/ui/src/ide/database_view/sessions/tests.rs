use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::{Event, PointerButton, pos2, vec2};
use serde_json::json;
use zen_db::memory::MemoryDriver;
use zen_db::sessions::{ServerLock, ServerSession};
use zen_workbench::workspaces::Workspaces;

use super::super::super::workspace_host::WorkspaceView;
use super::super::DatabaseView;
use super::*;

struct Setup {
    registry: Registry,
    session: Session,
    sql: Arc<MemoryDriver>,
    _dir: tempfile::TempDir,
    shop: String,
}

fn others() -> (Vec<ServerSession>, Vec<ServerLock>) {
    let session =
        |id: &str, user: &str, application: &str, state: &str, blocked_by: &[&str], query: &str| ServerSession {
            id: id.to_owned(),
            user: Some(user.to_owned()),
            host: Some("10.0.0.7:51544".to_owned()),
            database: Some("shop".to_owned()),
            application: Some(application.to_owned()),
            state: Some(state.to_owned()),
            query_start: Some("2026-10-27 14:05:09".to_owned()),
            seconds: Some(42),
            query: Some(query.to_owned()),
            wait: (!blocked_by.is_empty()).then(|| "Lock: transactionid".to_owned()),
            blocked_by: blocked_by.iter().map(|id| (*id).to_owned()).collect(),
            ..ServerSession::default()
        };
    let sessions = vec![
        session("4241", "ann", "psql", "idle in transaction", &[], "UPDATE orders SET paid = true WHERE id = 7"),
        session(
            "4242",
            "billing",
            "billing-service",
            "active",
            &["4241"],
            "UPDATE orders SET paid = false WHERE id = 7",
        ),
        session(
            "4243",
            "report",
            "metabase",
            "active",
            &["4242"],
            "SELECT sum(total) FROM orders WHERE paid FOR UPDATE",
        ),
        session("4250", "ann", "pgcli", "idle", &[], "SELECT now()"),
    ];
    let lock = |session: &str, mode: &str, granted: bool, blocked_by: &[&str]| ServerLock {
        session: session.to_owned(),
        kind: "transactionid".to_owned(),
        mode: mode.to_owned(),
        granted,
        object: Some("770".to_owned()),
        blocked_by: blocked_by.iter().map(|id| (*id).to_owned()).collect(),
    };
    let locks = vec![
        lock("4242", "ShareLock", false, &["4241"]),
        lock("4243", "ShareLock", false, &["4242"]),
        lock("4241", "ExclusiveLock", true, &[]),
    ];
    (sessions, locks)
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
    let sql = Arc::new(MemoryDriver::sql());
    let drivers: Vec<Arc<dyn zen_db::driver::Driver>> = vec![Arc::clone(&sql) as _];
    session.state_mut::<zen_workbench::database::Database>().set_drivers(drivers);
    let (sessions, locks) = others();
    sql.server.set_sessions(sessions, locks);
    let shop = registry
        .dispatch(
            &mut session,
            "db.dataSource.add",
            json!({ "name": "Shop", "driver": zen_db::memory::SQL, "password": "pw", "type": "dev" }),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    Setup { registry, session, sql, _dir: dir, shop }
}

fn connect(setup: &mut Setup) {
    let started = setup
        .registry
        .dispatch(&mut setup.session, "db.connection.connect", json!({ "connection": setup.shop }))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let state = setup
            .registry
            .dispatch(&mut setup.session, "db.operation.state", json!({ "operation": started["operation"] }))
            .unwrap();
        if state["status"] != "running" {
            assert_eq!(state["status"], "done", "{state}");
            return;
        }
        assert!(Instant::now() < deadline, "{state}");
        std::thread::sleep(Duration::from_millis(5));
    }
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

fn tab_state<'a>(view: &'a DatabaseView, tab: &InnerTab) -> &'a SessionsTab {
    view.sessions.tabs.get(&tab.id).unwrap()
}

fn settle(
    view: &mut DatabaseView,
    screen: &mut crate::offscreen::Offscreen,
    draw: &mut dyn FnMut(&mut DatabaseView, &mut crate::offscreen::Offscreen),
    tab: &InnerTab,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        draw(view, screen);
        let state = tab_state(view, tab);
        if state.listing.is_none() && state.ending.is_none() && state.answer.is_some() {
            return;
        }
        assert!(Instant::now() < deadline, "the sessions were never listed");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_sessions_tab_lists_chains_and_locks_and_ends_a_session_after_a_question_in_light_and_dark() {
    let mut setup = setup();
    let tab: InnerTab = serde_json::from_value(
        setup.registry.dispatch(&mut setup.session, "db.session.open", json!({ "connection": setup.shop })).unwrap(),
    )
    .unwrap();
    let mut view = DatabaseView::default();
    let mut screen = crate::offscreen::Offscreen::new(vec2(1180.0, 560.0), 2.0);
    let dark = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
    {
        let (registry, session) = (&setup.registry, &mut setup.session);
        screen.render(|ui| view.tab(ui, &dark, registry, session, &tab));
    }
    assert!(tab_state(&view, &tab).answer.is_none(), "a disconnected connection is not listed");
    assert!(setup.sql.server.opened().is_empty(), "showing the tab connects nowhere");
    connect(&mut setup);
    view.listed.read = None;

    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        palette.apply(screen.context());
        let (registry, session) = (&setup.registry, &mut setup.session);
        let mut draw = |view: &mut DatabaseView, screen: &mut crate::offscreen::Offscreen| {
            screen.render(|ui| view.tab(ui, &palette, registry, session, &tab));
        };
        settle(&mut view, &mut screen, &mut draw, &tab);
        screen.frame(click(pos2(400.0, BAR + HEAD + ROW * 1.5)), |ui| view.tab(ui, &palette, registry, session, &tab));
        let image = screen
            .frame(vec![Event::PointerMoved(pos2(5.0, 555.0))], |ui| view.tab(ui, &palette, registry, session, &tab));
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("db-sessions-{theme_name}.png")), image.image()).unwrap();
        }
    }
    let state = tab_state(&view, &tab);
    assert_eq!(state.selected.as_deref(), Some("4242"));
    let listed = &state.answer.as_ref().unwrap()["sessions"];
    assert_eq!(listed["chains"], json!([["4243", "4242", "4241"]]));
    assert_eq!(listed["locks"].as_array().unwrap().len(), 3);
    assert!(
        state.sessions().iter().any(|session| session["own"] == true),
        "the IDE's own sessions are listed and marked"
    );

    let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_light());
    palette.apply(screen.context());
    let state = view.sessions.tabs.get_mut(&tab.id).unwrap();
    state.ask(&setup.registry, &mut setup.session, (End::Terminate, "4241".to_owned(), false), false);
    let question = state.question.as_ref().expect("terminating asks first");
    assert!(question.dangerous && question.statement == "SELECT pg_terminate_backend(4241)", "{}", question.statement);
    assert!(setup.sql.server.executed().iter().all(|text| !text.contains("pg_terminate_backend")));
    {
        let (registry, session) = (&setup.registry, &mut setup.session);
        let image = screen.frame(Vec::new(), |ui| view.tab(ui, &palette, registry, session, &tab));
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join("db-sessions-question-light.png"), image.image()).unwrap();
        }
    }
    let state = view.sessions.tabs.get_mut(&tab.id).unwrap();
    state.question.as_mut().unwrap().understood = true;
    state.confirm(&setup.registry, &mut setup.session);
    assert!(state.ending.is_some());
    {
        let (registry, session) = (&setup.registry, &mut setup.session);
        let mut draw = |view: &mut DatabaseView, screen: &mut crate::offscreen::Offscreen| {
            screen.render(|ui| view.tab(ui, &palette, registry, session, &tab));
        };
        settle(&mut view, &mut screen, &mut draw, &tab);
    }
    let state = tab_state(&view, &tab);
    assert!(state.notice.as_deref().unwrap_or_default().contains("pg_terminate_backend(4241)"), "{:?}", state.notice);
    assert!(state.sessions().iter().all(|session| session["id"] != "4241"), "the ended session is gone");
    assert!(state.sessions().iter().any(|session| session["id"] == "4242"), "the others stay");
    let ended: Vec<String> =
        setup.sql.server.executed().into_iter().filter(|text| text.contains("pg_terminate_backend")).collect();
    assert_eq!(ended, ["SELECT pg_terminate_backend(4241)"]);
}

#[test]
fn auto_refresh_lists_again_only_while_the_tab_is_shown_and_connected() {
    let mut setup = setup();
    connect(&mut setup);
    let tab: InnerTab = serde_json::from_value(
        setup
            .registry
            .dispatch(&mut setup.session, "db.session.open", json!({ "connection": setup.shop, "autoRefresh": 2 }))
            .unwrap(),
    )
    .unwrap();
    let mut view = DatabaseView::default();
    let mut screen = crate::offscreen::Offscreen::new(vec2(900.0, 400.0), 1.0);
    let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
    let (registry, session) = (&setup.registry, &mut setup.session);
    let mut draw = |view: &mut DatabaseView, screen: &mut crate::offscreen::Offscreen| {
        screen.render(|ui| view.tab(ui, &palette, registry, session, &tab));
    };
    settle(&mut view, &mut screen, &mut draw, &tab);
    let first = tab_state(&view, &tab).read.unwrap();
    draw(&mut view, &mut screen);
    assert_eq!(tab_state(&view, &tab).read, Some(first), "not again before the interval");
    view.sessions.tabs.get_mut(&tab.id).unwrap().read = Some(first - Duration::from_secs(3));
    draw(&mut view, &mut screen);
    assert!(
        tab_state(&view, &tab).listing.is_some() || tab_state(&view, &tab).read != Some(first - Duration::from_secs(3))
    );
}
