use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use egui::{Event, PointerButton, pos2, vec2};
use serde_json::json;
use zen_db::dialect::SqlDialect;
use zen_db::driver::ContextKind;
use zen_db::edit::{Changes, Deleted, EditColumn, Editing, KeyKind, RowKey, Staged, Updated};
use zen_db::recovery::{ConsoleText, Item, OpenTransaction, Recovery, StagedEdits, dialect_name};
use zen_db::value::Value as Cell;
use zen_workbench::workspaces::{InnerTab, Workspaces};

use super::super::super::workspace_host::WorkspaceView;
use super::super::DatabaseView;
use super::*;

const WAIT: Duration = Duration::from_secs(10);

fn edits() -> Item {
    let column = |index: usize, name: &str, type_name: &str| EditColumn {
        index,
        name: name.to_owned(),
        type_name: type_name.to_owned(),
        editable: true,
        nullable: index > 0,
        default: None,
        sourced: true,
    };
    Item::Edits(Box::new(StagedEdits {
        connection: "shop".to_owned(),
        connection_name: "Shop".to_owned(),
        dialect: dialect_name(SqlDialect::Postgres).to_owned(),
        editing: Editing {
            editable: true,
            reason: None,
            table: Some("public.customers".to_owned()),
            key: Some(RowKey { kind: KeyKind::PrimaryKey, columns: vec![0], names: vec!["id".to_owned()] }),
            columns: vec![column(0, "id", "bigint"), column(1, "email", "text")],
            dialect: SqlDialect::Postgres,
            documents: false,
            target: None,
        },
        changes: Changes {
            deleted: vec![Deleted {
                row: 4,
                original: vec![Cell::Integer(5), Cell::Text("eve@example.com".to_owned())],
            }],
            updated: vec![Updated {
                row: 0,
                original: vec![Cell::Integer(1), Cell::Text("ann@example.com".to_owned())],
                cells: vec![(1, Staged::Value(Cell::Text("ann@example.org".to_owned())))],
            }],
            ..Changes::default()
        },
        statement: "SELECT * FROM public.customers".to_owned(),
        place: Some("public.customers".to_owned()),
        at: zen_db::history::now() - 90_000,
    }))
}

#[test]
fn the_recovered_tab_lists_what_was_kept_and_restores_nothing_by_itself_in_light_and_dark() {
    zen_i18n::register(crate::toolbox::LOCALES);
    zen_i18n::register(zen_workbench::LOCALES);
    zen_i18n::set_language(Some("en"));
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let folder = dir.path().join("data/databases");
    let before = Recovery::open(&folder);
    before.put("edits:1", edits());
    before.put(
        "transaction:1",
        Item::Transaction(OpenTransaction {
            connection: "shop".to_owned(),
            connection_name: "Shop".to_owned(),
            context: ContextKind::Console("orders.sql".to_owned()),
            statements: 3,
            modifying: 2,
            log: vec![
                "UPDATE orders SET status = 'shipped' WHERE id = 1042".to_owned(),
                "DELETE FROM order_lines WHERE order_id = 1042".to_owned(),
            ],
            at: zen_db::history::now() - 60_000,
        }),
    );
    before.put(
        "console:x",
        Item::Console(ConsoleText {
            path: dir.path().join("data/consoles/elsewhere/local/report.sql"),
            text: "SELECT region, sum(total)\nFROM orders\nGROUP BY region;".to_owned(),
            at: zen_db::history::now() - 30_000,
        }),
    );
    assert!(before.flush(WAIT));
    drop(before);

    let mut registry = Registry::new();
    zen_workbench::register(&mut registry).unwrap();
    zen_workbench::database::register(&mut registry).unwrap();
    let mut session = Session::new();
    registry.dispatch(&mut session, "file.project.open", json!({ "path": project })).unwrap();
    session.state_mut::<Workspaces>().set_data_dir(dir.path().join("data"));
    zen_workbench::database::install(&mut session);
    session.set_state(zen_workbench::database::Secrets(Arc::new(zen_secrets::MemoryStore::new())));
    let recovery = Arc::new(Recovery::open(&folder));
    assert!(recovery.flush(WAIT));
    session.set_state(zen_workbench::database::recovery::Store(Arc::clone(&recovery)));
    let tab: InnerTab =
        serde_json::from_value(registry.dispatch(&mut session, "db.recovery.open", json!({})).unwrap()).unwrap();

    let mut view = DatabaseView::default();
    let snapshots = std::env::var_os("ZEN_UI_SNAPSHOTS").map(PathBuf::from);
    let mut screen = crate::offscreen::Offscreen::new(vec2(1080.0, 460.0), 2.0);
    let click = |at: egui::Pos2| {
        let press = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        vec![Event::PointerMoved(at), press(true), press(false)]
    };
    for (theme_name, theme) in
        [("dark", zen_theme::ResolvedTheme::zen_dark()), ("light", zen_theme::ResolvedTheme::zen_light())]
    {
        let palette = IdePalette::from_theme(&theme);
        palette.apply(screen.context());
        let mut frame = |screen: &mut crate::offscreen::Offscreen, events: Vec<Event>| {
            screen.frame(events, |ui| view.tab(ui, &palette, &registry, &mut session, &tab))
        };
        frame(&mut screen, Vec::new());
        frame(&mut screen, click(pos2(500.0, BAR + ROW * 2.0 + ROW / 2.0)));
        let image = frame(&mut screen, vec![Event::PointerMoved(pos2(5.0, 455.0))]);
        if let Some(dir) = &snapshots {
            crate::offscreen::write_png(&dir.join(format!("db-recovery-{theme_name}.png")), image.image()).unwrap();
        }
    }
    let kinds: Vec<&str> = view.recovery.items().iter().map(|item| item["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["console", "transaction", "edits"], "newest first");
    let detail = view.recovery.detail.clone().expect("a click shows the item");
    let sql = detail["text"].as_str().unwrap();
    assert!(sql.starts_with("DELETE FROM public.customers WHERE \"id\" = 5;"), "{sql}");
    assert!(sql.contains(
        "SET \"email\" = 'ann@example.org' WHERE \"id\" = 1 AND \"email\" IS NOT DISTINCT FROM 'ann@example.com'"
    ));

    let id = detail["id"].as_u64().unwrap();
    view.recovery.act(&registry, &mut session, Action::Discard(id));
    assert_eq!(view.recovery.question, Some(Action::Discard(id)), "discarding asks first");
    assert_eq!(recovery.recovered().len(), 3);
    view.recovery.answer(&registry, &mut session, false);
    assert_eq!((view.recovery.question, recovery.recovered().len()), (None, 3), "cancel keeps it");
    view.recovery.act(&registry, &mut session, Action::Discard(id));
    view.recovery.answer(&registry, &mut session, true);
    assert_eq!(recovery.recovered().len(), 2);
    view.recovery.act(&registry, &mut session, Action::DiscardAll);
    assert_eq!(recovery.recovered().len(), 2, "Discard All asks first");
    view.recovery.answer(&registry, &mut session, true);
    assert!(recovery.recovered().is_empty());
    assert!(recovery.flush(WAIT));
    assert!(!recovery.file().exists(), "nothing kept, no file");
    let status = registry.dispatch(&mut session, "db.connection.status", json!({})).unwrap();
    assert!(
        status["connections"].as_array().unwrap().iter().all(|connection| connection["state"] == "disconnected"),
        "the tab connects nowhere"
    );
}
