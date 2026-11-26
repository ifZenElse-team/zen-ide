use std::path::PathBuf;
use std::sync::Arc;

use egui::{Event, PointerButton, pos2, vec2};
use serde_json::json;
use zen_db::explain::{PlanFormat, parse};
use zen_db::value::{Column, Execution, Value};
use zen_workbench::database::plans::{PlanStatus, PlanTab, Plans};
use zen_workbench::workspaces::Workspaces;

use super::super::super::workspace_host::WorkspaceView;
use super::super::DatabaseView;
use super::*;

struct Setup {
    registry: Registry,
    session: Session,
    tab: InnerTab,
    _dir: tempfile::TempDir,
}

fn plan() -> Plan {
    let execution = Execution {
        columns: vec![Column::new("QUERY PLAN".to_owned(), "json".to_owned())],
        rows: vec![vec![Value::Json(include_str!("analyzed.json").to_owned())]],
        ..Execution::default()
    };
    parse(PlanFormat::PostgresJson, &execution, true).unwrap()
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
            json!({ "name": "Shop", "url": "postgresql://ann@db.example.com:5432/shop", "type": "dev", "auth": "none" }),
        )
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let statement = "SELECT c.country, count(*), sum(o.total)\nFROM orders o JOIN customers c ON c.id = o.customer\nWHERE o.total > 100 AND c.country <> 'FR'\nGROUP BY c.country ORDER BY 3 DESC";
    let options = ExplainOptions { analyze: true, ..ExplainOptions::default() };
    let tab: InnerTab = serde_json::from_value(
        registry
            .dispatch(
                &mut session,
                "workspace.tab.open",
                json!({
                    "workspace": "database",
                    "id": "plan:console:shop/console",
                    "kind": "plan",
                    "title": "Plan: console",
                    "data": { "connection": shop, "statement": statement, "options": options },
                }),
            )
            .unwrap(),
    )
    .unwrap();
    let plan = plan();
    let state = PlanTab {
        connection: shop,
        statement: statement.to_owned(),
        options,
        supported: zen_db::explain::postgres_options(),
        executes: true,
        rolled_back: true,
        status: PlanStatus::Done,
        selected: Some(2),
        plan: Some(Arc::new(plan)),
        ..PlanTab::default()
    };
    session.state_mut::<Plans>().set(&tab.id, state);
    Setup { registry, session, tab, _dir: dir }
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

fn selected(setup: &Setup) -> Option<usize> {
    setup.session.state::<Plans>().and_then(|plans| plans.get(&setup.tab.id)).and_then(|state| state.selected)
}

#[test]
fn the_plan_tab_shows_the_tree_with_hot_nodes_and_details_in_light_and_dark() {
    let mut setup = setup();
    let mut view = DatabaseView::default();
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let mut screen = crate::offscreen::Offscreen::new(vec2(1280.0, 420.0), 2.0);
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        palette.apply(screen.context());
        let tab = setup.tab.clone();
        let (registry, session) = (&setup.registry, &mut setup.session);
        let mut frame = |screen: &mut crate::offscreen::Offscreen, events: Vec<Event>| {
            screen.frame(events, |ui| view.tab(ui, &palette, registry, session, &tab))
        };
        frame(&mut screen, Vec::new());
        let image = frame(&mut screen, vec![Event::PointerMoved(pos2(5.0, 415.0))]);
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("db-plan-{theme_name}.png")), image.image()).unwrap();
        }
    }
    let top = BAR + STRIP + ROW;
    let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_light());
    let tab = setup.tab.clone();
    {
        let (registry, session) = (&setup.registry, &mut setup.session);
        screen.frame(click(pos2(200.0, top + ROW * 3.0 + ROW / 2.0)), |ui| {
            view.tab(ui, &palette, registry, session, &tab)
        });
    }
    assert_eq!(selected(&setup), Some(3), "a click chooses the node for the details pane");
    let plan = plan();
    let shares: Vec<bool> = plan.nodes.iter().map(|node| node.hot).collect();
    assert_eq!(plan.nodes[2].node, "Hash Join");
    assert!(shares.iter().any(|hot| *hot) && !shares.iter().all(|hot| *hot), "{shares:?}");
    assert!(heat(&palette, 0.6).is_some() && heat(&palette, 0.01).is_none());
}

#[test]
fn the_raw_view_and_the_analyze_warning_follow_the_tab() {
    let mut setup = setup();
    setup.registry.dispatch(&mut setup.session, "db.plan.view", json!({ "view": "raw" })).unwrap();
    let mut view = DatabaseView::default();
    let palette = IdePalette::from_theme(&zen_theme::ResolvedTheme::zen_dark());
    let mut screen = crate::offscreen::Offscreen::new(vec2(900.0, 300.0), 1.0);
    let tab = setup.tab.clone();
    let (registry, session) = (&setup.registry, &mut setup.session);
    screen.render(|ui| view.tab(ui, &palette, registry, session, &tab));
    let state = session.state::<Plans>().unwrap().get(&tab.id).unwrap().clone();
    assert_eq!(state.view, Shown::Raw);
    assert!(view.plan.options(&tab.id, &state).analyze, "the bar shows ANALYZE as the plan was explained");
    let status = registry.dispatch(session, "db.connection.status", json!({})).unwrap();
    assert!(
        status["connections"].as_array().unwrap().iter().all(|connection| connection["state"] == "disconnected"),
        "drawing a plan connects nowhere"
    );
}
